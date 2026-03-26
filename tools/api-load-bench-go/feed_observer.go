package main

import (
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/gorilla/websocket"
)

type wsBookLevel struct {
	Price    uint64 `json:"price"`
	Quantity uint64 `json:"quantity"`
}

type wsL3Order struct {
	OrderID   string `json:"order_id"`
	Price     uint64 `json:"price"`
	Remaining uint64 `json:"remaining"`
	CreatedAt string `json:"created_at"`
}

type wsL3Event struct {
	Kind      string `json:"kind"`
	OrderID   string `json:"order_id"`
	Side      string `json:"side"`
	Price     uint64 `json:"price"`
	Remaining uint64 `json:"remaining"`
	Quantity  uint64 `json:"quantity"`
}

type l2BookState struct {
	Initialized bool
	Sequence    uint64
	Bids        map[uint64]uint64
	Asks        map[uint64]uint64
}

type l3OrderState struct {
	Side      string
	Price     uint64
	Remaining uint64
}

type l3BookState struct {
	Initialized bool
	Sequence    uint64
	Orders      map[string]l3OrderState
}

type feedObserver struct {
	name      string
	channel   string
	market    string
	apiKey    string
	timeout   time.Duration
	conn      *websocket.Conn
	writeMu   sync.Mutex
	stateMu   sync.Mutex
	l2        l2BookState
	l3        l3BookState
	closing   uint32
	closeOnce sync.Once

	snapshotCount     int64
	deltaCount        int64
	deltaEventCount   int64
	batchedDeltaCount int64
	maxBatchSize      int64
	resyncCount       int64
	gapCount          int64
	systemErrors      int64
	validationErrors  []string
}

type feedObserverManager struct {
	cfg       config
	market    string
	observers []*feedObserver
}

func newFeedObserverManager(
	cfg config,
	market string,
	l3Users []userProfile,
) (*feedObserverManager, error) {
	manager := &feedObserverManager{
		cfg:       cfg,
		market:    market,
		observers: make([]*feedObserver, 0, cfg.ObserveL2Clients+cfg.ObserveL3Clients),
	}

	for index := 0; index < cfg.ObserveL2Clients; index++ {
		observer, err := newFeedObserver(
			cfg.BaseURL,
			market,
			"l2",
			"",
			cfg.RequestTimeout,
			fmt.Sprintf("l2-%d", index+1),
		)
		if err != nil {
			manager.Close()
			return nil, err
		}
		manager.observers = append(manager.observers, observer)
	}

	for index := 0; index < cfg.ObserveL3Clients; index++ {
		if index >= len(l3Users) {
			manager.Close()
			return nil, fmt.Errorf("not enough observer users provisioned for l3: need %d have %d", cfg.ObserveL3Clients, len(l3Users))
		}
		observer, err := newFeedObserver(
			cfg.BaseURL,
			market,
			"l3",
			l3Users[index].APIKey,
			cfg.RequestTimeout,
			fmt.Sprintf("l3-%d", index+1),
		)
		if err != nil {
			manager.Close()
			return nil, err
		}
		manager.observers = append(manager.observers, observer)
	}

	return manager, nil
}

func (m *feedObserverManager) Validate() (feedValidationResult, error) {
	if len(m.observers) == 0 {
		return feedValidationResult{summary: "observers disabled"}, nil
	}

	summaries := make([]string, 0, len(m.observers))
	reasons := make([]string, 0)
	for _, observer := range m.observers {
		summary, observerReasons, err := observer.validate(m.cfg.BaseURL)
		if err != nil {
			return feedValidationResult{}, err
		}
		summaries = append(summaries, summary)
		if m.cfg.RequireFeedAccuracy {
			reasons = append(reasons, observerReasons...)
		}
	}

	return feedValidationResult{
		summary: strings.Join(summaries, " | "),
		reasons: reasons,
	}, nil
}

func (m *feedObserverManager) Close() {
	for _, observer := range m.observers {
		observer.Close()
	}
}

func newFeedObserver(
	baseURL string,
	market string,
	channel string,
	apiKey string,
	timeout time.Duration,
	name string,
) (*feedObserver, error) {
	wsURL, err := websocketURL(baseURL)
	if err != nil {
		return nil, err
	}

	headers := http.Header{}
	headers.Set("User-Agent", "exchange-api-load-bench-go")
	conn, response, err := websocket.DefaultDialer.Dial(wsURL, headers)
	if err != nil {
		if response != nil {
			return nil, fmt.Errorf("dial %s observer websocket: %w (status %s)", name, err, response.Status)
		}
		return nil, fmt.Errorf("dial %s observer websocket: %w", name, err)
	}

	observer := &feedObserver{
		name:    name,
		channel: channel,
		market:  market,
		apiKey:  apiKey,
		timeout: timeout,
		conn:    conn,
		l2: l2BookState{
			Bids: make(map[uint64]uint64),
			Asks: make(map[uint64]uint64),
		},
		l3: l3BookState{
			Orders: make(map[string]l3OrderState),
		},
	}

	if apiKey != "" {
		if err := authenticateFeedObserver(observer); err != nil {
			observer.Close()
			return nil, fmt.Errorf("authenticate %s observer websocket: %w", name, err)
		}
	}
	if err := observer.subscribe(); err != nil {
		observer.Close()
		return nil, fmt.Errorf("subscribe %s observer websocket: %w", name, err)
	}
	if err := observer.readInitialSnapshot(); err != nil {
		observer.Close()
		return nil, fmt.Errorf("bootstrap %s observer websocket: %w", name, err)
	}

	go observer.readLoop()
	return observer, nil
}

func authenticateFeedObserver(observer *feedObserver) error {
	request := map[string]any{
		"op":      "authenticate",
		"api_key": observer.apiKey,
	}
	return observer.writeJSON(request, true)
}

func (o *feedObserver) subscribe() error {
	request := map[string]any{
		"op":            "subscribe",
		"channel":       o.channel,
		"market":        o.market,
		"last_sequence": nil,
	}
	return o.writeJSON(request, false)
}

func (o *feedObserver) writeJSON(request map[string]any, extendReadDeadline bool) error {
	payload, err := json.Marshal(request)
	if err != nil {
		return err
	}

	o.writeMu.Lock()
	defer o.writeMu.Unlock()

	writeDeadline := time.Now().Add(o.timeout)
	o.conn.SetWriteDeadline(writeDeadline)
	if err := o.conn.WriteMessage(websocket.TextMessage, payload); err != nil {
		return err
	}
	if extendReadDeadline {
		o.conn.SetReadDeadline(writeDeadline)
	}
	return nil
}

func (o *feedObserver) readInitialSnapshot() error {
	deadline := time.Now().Add(o.timeout)
	for {
		o.conn.SetReadDeadline(deadline)
		_, frame, err := o.conn.ReadMessage()
		if err != nil {
			return err
		}
		var message wsServerMessage
		if err := json.Unmarshal(frame, &message); err != nil {
			return err
		}

		switch message.Type {
		case "heartbeat", "authenticated":
			continue
		case "snapshot":
			if o.channel != "l2" || message.Channel != "l2" || message.Market != o.market {
				continue
			}
			if err := o.applyL2Snapshot(message); err != nil {
				return err
			}
			o.conn.SetReadDeadline(time.Time{})
			return nil
		case "l3_snapshot":
			if o.channel != "l3" || message.Channel != "l3" || message.Market != o.market {
				continue
			}
			if err := o.applyL3Snapshot(message); err != nil {
				return err
			}
			o.conn.SetReadDeadline(time.Time{})
			return nil
		case "error":
			return fmt.Errorf("%s %s", message.Code, message.Message)
		default:
			continue
		}
	}
}

func (o *feedObserver) readLoop() {
	for {
		_, frame, err := o.conn.ReadMessage()
		if err != nil {
			if atomic.LoadUint32(&o.closing) == 1 {
				return
			}
			o.recordValidationError(fmt.Sprintf("%s read failed: %v", o.name, err))
			atomic.AddInt64(&o.systemErrors, 1)
			return
		}

		var message wsServerMessage
		if err := json.Unmarshal(frame, &message); err != nil {
			o.recordValidationError(fmt.Sprintf("%s decode failed: %v", o.name, err))
			continue
		}

		resubscribe := false
		switch message.Type {
		case "heartbeat", "authenticated", "ack", "reject", "fill", "order_state", "admin_message", "unsubscribed":
			continue
		case "snapshot":
			if o.channel == "l2" && message.Channel == "l2" && message.Market == o.market {
				if err := o.applyL2Snapshot(message); err != nil {
					o.recordValidationError(fmt.Sprintf("%s snapshot apply failed: %v", o.name, err))
				}
			}
		case "delta":
			if o.channel == "l2" && message.Channel == "l2" && message.Market == o.market {
				needsResubscribe, err := o.applyL2Delta(message)
				if err != nil {
					o.recordValidationError(fmt.Sprintf("%s delta apply failed: %v", o.name, err))
				}
				resubscribe = needsResubscribe
			}
		case "l3_snapshot":
			if o.channel == "l3" && message.Channel == "l3" && message.Market == o.market {
				if err := o.applyL3Snapshot(message); err != nil {
					o.recordValidationError(fmt.Sprintf("%s l3 snapshot apply failed: %v", o.name, err))
				}
			}
		case "l3_delta":
			if o.channel == "l3" && message.Channel == "l3" && message.Market == o.market {
				needsResubscribe, err := o.applyL3Delta(message)
				if err != nil {
					o.recordValidationError(fmt.Sprintf("%s l3 delta apply failed: %v", o.name, err))
				}
				resubscribe = needsResubscribe
			}
		case "resync_required":
			if message.Channel == o.channel && message.Market == o.market {
				atomic.AddInt64(&o.resyncCount, 1)
				o.clearState()
				resubscribe = true
			}
		case "error":
			atomic.AddInt64(&o.systemErrors, 1)
			o.recordValidationError(fmt.Sprintf("%s protocol error: %s %s", o.name, message.Code, message.Message))
		}

		if resubscribe {
			if err := o.subscribe(); err != nil && atomic.LoadUint32(&o.closing) == 0 {
				o.recordValidationError(fmt.Sprintf("%s resubscribe failed: %v", o.name, err))
			}
		}
	}
}

func (o *feedObserver) applyL2Snapshot(message wsServerMessage) error {
	bids, err := decodeBookLevels(message.Bids)
	if err != nil {
		return err
	}
	asks, err := decodeBookLevels(message.Asks)
	if err != nil {
		return err
	}

	o.stateMu.Lock()
	defer o.stateMu.Unlock()

	replaceL2Snapshot(&o.l2, bids, asks, message.Sequence)
	atomic.AddInt64(&o.snapshotCount, 1)
	return nil
}

func (o *feedObserver) applyL2Delta(message wsServerMessage) (bool, error) {
	events, err := decodeL2Events(message.Events)
	if err != nil {
		return false, err
	}

	o.stateMu.Lock()
	defer o.stateMu.Unlock()

	if !o.l2.Initialized {
		o.recordValidationErrorLocked(fmt.Sprintf("%s received l2 delta before snapshot", o.name))
		return true, nil
	}
	expectedSequence := o.l2.Sequence + 1
	if message.StartSequence != expectedSequence {
		atomic.AddInt64(&o.gapCount, 1)
		o.recordValidationErrorLocked(
			fmt.Sprintf("%s l2 gap expected=%d got=%d", o.name, expectedSequence, message.StartSequence),
		)
		o.l2 = l2BookState{Bids: make(map[uint64]uint64), Asks: make(map[uint64]uint64)}
		return true, nil
	}

	applyL2Events(&o.l2, events, message.Sequence)
	o.recordBatchMetrics(len(events))
	return false, nil
}

func (o *feedObserver) applyL3Snapshot(message wsServerMessage) error {
	bids, err := decodeL3Orders(message.Bids)
	if err != nil {
		return err
	}
	asks, err := decodeL3Orders(message.Asks)
	if err != nil {
		return err
	}

	o.stateMu.Lock()
	defer o.stateMu.Unlock()

	replaceL3Snapshot(&o.l3, bids, asks, message.Sequence)
	atomic.AddInt64(&o.snapshotCount, 1)
	return nil
}

func (o *feedObserver) applyL3Delta(message wsServerMessage) (bool, error) {
	events, err := decodeL3Events(message.Events)
	if err != nil {
		return false, err
	}

	o.stateMu.Lock()
	defer o.stateMu.Unlock()

	if !o.l3.Initialized {
		o.recordValidationErrorLocked(fmt.Sprintf("%s received l3 delta before snapshot", o.name))
		return true, nil
	}
	expectedSequence := o.l3.Sequence + 1
	if message.StartSequence != expectedSequence {
		atomic.AddInt64(&o.gapCount, 1)
		o.recordValidationErrorLocked(
			fmt.Sprintf("%s l3 gap expected=%d got=%d", o.name, expectedSequence, message.StartSequence),
		)
		o.l3 = l3BookState{Orders: make(map[string]l3OrderState)}
		return true, nil
	}

	applyL3Events(&o.l3, events, message.Sequence)
	o.recordBatchMetrics(len(events))
	return false, nil
}

func (o *feedObserver) recordBatchMetrics(eventCount int) {
	atomic.AddInt64(&o.deltaCount, 1)
	atomic.AddInt64(&o.deltaEventCount, int64(eventCount))
	if eventCount > 1 {
		atomic.AddInt64(&o.batchedDeltaCount, 1)
	}
	for {
		currentMax := atomic.LoadInt64(&o.maxBatchSize)
		if int64(eventCount) <= currentMax {
			break
		}
		if atomic.CompareAndSwapInt64(&o.maxBatchSize, currentMax, int64(eventCount)) {
			break
		}
	}
}

func (o *feedObserver) clearState() {
	o.stateMu.Lock()
	defer o.stateMu.Unlock()

	o.l2 = l2BookState{Bids: make(map[uint64]uint64), Asks: make(map[uint64]uint64)}
	o.l3 = l3BookState{Orders: make(map[string]l3OrderState)}
}

func (o *feedObserver) recordValidationError(message string) {
	o.stateMu.Lock()
	defer o.stateMu.Unlock()
	o.recordValidationErrorLocked(message)
}

func (o *feedObserver) recordValidationErrorLocked(message string) {
	o.validationErrors = append(o.validationErrors, message)
}

func (o *feedObserver) validate(baseURL string) (string, []string, error) {
	o.stateMu.Lock()
	currentL2 := cloneL2Book(o.l2)
	currentL3 := cloneL3Book(o.l3)
	validationErrors := append([]string(nil), o.validationErrors...)
	o.stateMu.Unlock()

	reasons := make([]string, 0)
	if atomic.LoadInt64(&o.gapCount) > 0 {
		reasons = append(reasons, fmt.Sprintf("%s observed %d feed gaps", o.name, atomic.LoadInt64(&o.gapCount)))
	}
	if atomic.LoadInt64(&o.resyncCount) > 0 {
		reasons = append(reasons, fmt.Sprintf("%s observed %d resyncs", o.name, atomic.LoadInt64(&o.resyncCount)))
	}
	if len(validationErrors) > 0 {
		reasons = append(reasons, validationErrors...)
	}

	switch o.channel {
	case "l2":
		fresh, err := fetchFreshL2Snapshot(baseURL, o.market, o.timeout)
		if err != nil {
			return "", nil, err
		}
		if !equalL2Book(currentL2, fresh) {
			reasons = append(reasons, fmt.Sprintf("%s final l2 book diverged from fresh snapshot", o.name))
		}
	case "l3":
		freshL3, err := fetchFreshL3Snapshot(baseURL, o.market, o.apiKey, o.timeout)
		if err != nil {
			return "", nil, err
		}
		if !equalL3Book(currentL3, freshL3) {
			reasons = append(reasons, fmt.Sprintf("%s final l3 book diverged from fresh snapshot", o.name))
		}
		freshL2, err := fetchFreshL2Snapshot(baseURL, o.market, o.timeout)
		if err != nil {
			return "", nil, err
		}
		if !equalL2Book(aggregateL3Book(currentL3), freshL2) {
			reasons = append(reasons, fmt.Sprintf("%s final l3 book did not aggregate to the fresh l2 snapshot", o.name))
		}
	}

	summary := fmt.Sprintf(
		"%s[%s] snapshots=%d deltas=%d events=%d batched=%d maxBatch=%d resyncs=%d gaps=%d errors=%d",
		o.name,
		o.channel,
		atomic.LoadInt64(&o.snapshotCount),
		atomic.LoadInt64(&o.deltaCount),
		atomic.LoadInt64(&o.deltaEventCount),
		atomic.LoadInt64(&o.batchedDeltaCount),
		atomic.LoadInt64(&o.maxBatchSize),
		atomic.LoadInt64(&o.resyncCount),
		atomic.LoadInt64(&o.gapCount),
		atomic.LoadInt64(&o.systemErrors),
	)

	return summary, reasons, nil
}

func (o *feedObserver) Close() {
	o.closeOnce.Do(func() {
		atomic.StoreUint32(&o.closing, 1)
		_ = o.conn.Close()
	})
}

func fetchFreshL2Snapshot(baseURL, market string, timeout time.Duration) (l2BookState, error) {
	observer, err := newFeedObserver(baseURL, market, "l2", "", timeout, "fresh-l2")
	if err != nil {
		return l2BookState{}, err
	}
	defer observer.Close()

	observer.stateMu.Lock()
	defer observer.stateMu.Unlock()
	return cloneL2Book(observer.l2), nil
}

func fetchFreshL3Snapshot(
	baseURL string,
	market string,
	apiKey string,
	timeout time.Duration,
) (l3BookState, error) {
	observer, err := newFeedObserver(baseURL, market, "l3", apiKey, timeout, "fresh-l3")
	if err != nil {
		return l3BookState{}, err
	}
	defer observer.Close()

	observer.stateMu.Lock()
	defer observer.stateMu.Unlock()
	return cloneL3Book(observer.l3), nil
}

func decodeBookLevels(rawLevels []json.RawMessage) ([]wsBookLevel, error) {
	levels := make([]wsBookLevel, 0, len(rawLevels))
	for _, raw := range rawLevels {
		var level wsBookLevel
		if err := json.Unmarshal(raw, &level); err != nil {
			return nil, err
		}
		levels = append(levels, level)
	}
	return levels, nil
}

func decodeL3Orders(rawOrders []json.RawMessage) ([]wsL3Order, error) {
	orders := make([]wsL3Order, 0, len(rawOrders))
	for _, raw := range rawOrders {
		var order wsL3Order
		if err := json.Unmarshal(raw, &order); err != nil {
			return nil, err
		}
		orders = append(orders, order)
	}
	return orders, nil
}

func decodeL2Events(rawEvents []json.RawMessage) ([]BookEvent, error) {
	events := make([]BookEvent, 0, len(rawEvents))
	for _, raw := range rawEvents {
		var event BookEvent
		if err := json.Unmarshal(raw, &event); err != nil {
			return nil, err
		}
		events = append(events, event)
	}
	return events, nil
}

func decodeL3Events(rawEvents []json.RawMessage) ([]wsL3Event, error) {
	events := make([]wsL3Event, 0, len(rawEvents))
	for _, raw := range rawEvents {
		var event wsL3Event
		if err := json.Unmarshal(raw, &event); err != nil {
			return nil, err
		}
		events = append(events, event)
	}
	return events, nil
}

type BookEvent struct {
	Kind     string `json:"kind"`
	Side     string `json:"side"`
	Price    uint64 `json:"price"`
	Quantity uint64 `json:"quantity"`
}

func replaceL2Snapshot(state *l2BookState, bids []wsBookLevel, asks []wsBookLevel, sequence uint64) {
	state.Initialized = true
	state.Sequence = sequence
	state.Bids = make(map[uint64]uint64, len(bids))
	state.Asks = make(map[uint64]uint64, len(asks))
	for _, level := range bids {
		if level.Quantity > 0 {
			state.Bids[level.Price] = level.Quantity
		}
	}
	for _, level := range asks {
		if level.Quantity > 0 {
			state.Asks[level.Price] = level.Quantity
		}
	}
}

func applyL2Events(state *l2BookState, events []BookEvent, sequence uint64) {
	for _, event := range events {
		switch event.Kind {
		case "level_updated":
			target := state.Bids
			if strings.EqualFold(event.Side, "SELL") {
				target = state.Asks
			}
			if event.Quantity == 0 {
				delete(target, event.Price)
			} else {
				target[event.Price] = event.Quantity
			}
		case "trade":
			continue
		}
	}
	state.Sequence = sequence
}

func replaceL3Snapshot(state *l3BookState, bids []wsL3Order, asks []wsL3Order, sequence uint64) {
	state.Initialized = true
	state.Sequence = sequence
	state.Orders = make(map[string]l3OrderState, len(bids)+len(asks))
	for _, order := range bids {
		state.Orders[order.OrderID] = l3OrderState{
			Side:      "BUY",
			Price:     order.Price,
			Remaining: order.Remaining,
		}
	}
	for _, order := range asks {
		state.Orders[order.OrderID] = l3OrderState{
			Side:      "SELL",
			Price:     order.Price,
			Remaining: order.Remaining,
		}
	}
}

func applyL3Events(state *l3BookState, events []wsL3Event, sequence uint64) {
	for _, event := range events {
		switch event.Kind {
		case "order_added", "order_updated":
			state.Orders[event.OrderID] = l3OrderState{
				Side:      strings.ToUpper(event.Side),
				Price:     event.Price,
				Remaining: event.Remaining,
			}
		case "order_removed":
			delete(state.Orders, event.OrderID)
		case "trade":
			continue
		}
	}
	state.Sequence = sequence
}

func aggregateL3Book(state l3BookState) l2BookState {
	aggregate := l2BookState{
		Initialized: state.Initialized,
		Sequence:    state.Sequence,
		Bids:        make(map[uint64]uint64),
		Asks:        make(map[uint64]uint64),
	}

	for _, order := range state.Orders {
		if order.Remaining == 0 {
			continue
		}
		if strings.EqualFold(order.Side, "SELL") {
			aggregate.Asks[order.Price] += order.Remaining
		} else {
			aggregate.Bids[order.Price] += order.Remaining
		}
	}

	return aggregate
}

func cloneL2Book(state l2BookState) l2BookState {
	clone := l2BookState{
		Initialized: state.Initialized,
		Sequence:    state.Sequence,
		Bids:        make(map[uint64]uint64, len(state.Bids)),
		Asks:        make(map[uint64]uint64, len(state.Asks)),
	}
	for price, quantity := range state.Bids {
		clone.Bids[price] = quantity
	}
	for price, quantity := range state.Asks {
		clone.Asks[price] = quantity
	}
	return clone
}

func cloneL3Book(state l3BookState) l3BookState {
	clone := l3BookState{
		Initialized: state.Initialized,
		Sequence:    state.Sequence,
		Orders:      make(map[string]l3OrderState, len(state.Orders)),
	}
	for orderID, order := range state.Orders {
		clone.Orders[orderID] = order
	}
	return clone
}

func equalL2Book(left, right l2BookState) bool {
	if len(left.Bids) != len(right.Bids) || len(left.Asks) != len(right.Asks) {
		return false
	}
	for price, quantity := range left.Bids {
		if right.Bids[price] != quantity {
			return false
		}
	}
	for price, quantity := range left.Asks {
		if right.Asks[price] != quantity {
			return false
		}
	}
	return true
}

func equalL3Book(left, right l3BookState) bool {
	if len(left.Orders) != len(right.Orders) {
		return false
	}
	for orderID, order := range left.Orders {
		if right.Orders[orderID] != order {
			return false
		}
	}
	return true
}
