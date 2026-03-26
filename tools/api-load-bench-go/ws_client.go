package main

import (
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"github.com/gorilla/websocket"
)

type pendingRequest struct {
	startedAt time.Time
}

type wsServerMessage struct {
	Type             string            `json:"type"`
	Op               string            `json:"op"`
	RequestID        *string           `json:"request_id"`
	Code             string            `json:"code"`
	Message          string            `json:"message"`
	Status           string            `json:"status"`
	Channel          string            `json:"channel"`
	Market           string            `json:"market"`
	Sequence         uint64            `json:"sequence"`
	StartSequence    uint64            `json:"start_sequence"`
	ExpectedSequence *uint64           `json:"expected_sequence"`
	CurrentSequence  *uint64           `json:"current_sequence"`
	Bids             []json.RawMessage `json:"bids"`
	Asks             []json.RawMessage `json:"asks"`
	Events           []json.RawMessage `json:"events"`
	Fill             *fill             `json:"fill"`
	Order            *wsOrderState     `json:"order"`
}

type wsOrderState struct {
	Remaining uint64 `json:"remaining"`
}

type wsClient struct {
	user       userProfile
	conn       *websocket.Conn
	writeMu    sync.Mutex
	pendingMu  sync.Mutex
	pending    map[string]pendingRequest
	closing    uint32
	closeOnce  sync.Once
	closeErrMu sync.Mutex
	closeErr   error
}

type wsManager struct {
	cfg       config
	stats     *stats
	sem       chan struct{}
	requestWG *sync.WaitGroup
	clients   map[string]*wsClient
	requestID uint64
}

func newWSManager(
	cfg config,
	stats *stats,
	users []userProfile,
	sem chan struct{},
	requestWG *sync.WaitGroup,
) (*wsManager, error) {
	wsURL, err := websocketURL(cfg.BaseURL)
	if err != nil {
		return nil, err
	}

	manager := &wsManager{
		cfg:       cfg,
		stats:     stats,
		sem:       sem,
		requestWG: requestWG,
		clients:   make(map[string]*wsClient, len(users)),
	}

	headers := http.Header{}
	headers.Set("User-Agent", "exchange-api-load-bench-go")

	for _, user := range users {
		conn, response, err := websocket.DefaultDialer.Dial(wsURL, headers)
		if err != nil {
			if response != nil {
				return nil, fmt.Errorf("dial websocket for %s: %w (status %s)", user.Username, err, response.Status)
			}
			return nil, fmt.Errorf("dial websocket for %s: %w", user.Username, err)
		}

		client := &wsClient{
			user:    user,
			conn:    conn,
			pending: make(map[string]pendingRequest),
		}
		if err := authenticateWSClient(client, cfg.RequestTimeout); err != nil {
			_ = conn.Close()
			return nil, fmt.Errorf("authenticate websocket for %s: %w", user.Username, err)
		}

		manager.clients[user.APIKey] = client
		go manager.readLoop(client)
	}

	return manager, nil
}

func websocketURL(baseURL string) (string, error) {
	parsed, err := url.Parse(strings.TrimRight(baseURL, "/"))
	if err != nil {
		return "", err
	}

	switch parsed.Scheme {
	case "http":
		parsed.Scheme = "ws"
	case "https":
		parsed.Scheme = "wss"
	case "ws", "wss":
	default:
		return "", fmt.Errorf("unsupported base URL scheme %q", parsed.Scheme)
	}

	parsed.Path = strings.TrimRight(parsed.Path, "/") + "/ws"
	return parsed.String(), nil
}

func authenticateWSClient(client *wsClient, timeout time.Duration) error {
	request := map[string]any{
		"op":      "authenticate",
		"api_key": client.user.APIKey,
	}

	payload, err := json.Marshal(request)
	if err != nil {
		return err
	}

	client.writeMu.Lock()
	client.conn.SetWriteDeadline(time.Now().Add(timeout))
	err = client.conn.WriteMessage(websocket.TextMessage, payload)
	client.writeMu.Unlock()
	if err != nil {
		return err
	}

	deadline := time.Now().Add(timeout)
	for {
		client.conn.SetReadDeadline(deadline)
		_, frame, err := client.conn.ReadMessage()
		if err != nil {
			return err
		}

		var message wsServerMessage
		if err := json.Unmarshal(frame, &message); err != nil {
			return err
		}

		switch message.Type {
		case "heartbeat":
			continue
		case "authenticated":
			client.conn.SetReadDeadline(time.Time{})
			return nil
		case "error":
			return fmt.Errorf("authentication rejected: %s %s", message.Code, message.Message)
		default:
			return fmt.Errorf("unexpected authentication reply: %s", strings.TrimSpace(string(frame)))
		}
	}
}

func (m *wsManager) Dispatch(user userProfile, marketID, side string, price, quantity uint64) {
	client, ok := m.clients[user.APIKey]
	if !ok {
		m.stats.recordAttempt()
		m.stats.recordReject(fmt.Sprintf("missing websocket client for %s", user.Username))
		return
	}

	m.sem <- struct{}{}
	m.requestWG.Add(1)
	m.stats.recordAttempt()

	requestID := fmt.Sprintf("ws-%d", atomic.AddUint64(&m.requestID, 1))
	startedAt := time.Now()

	client.pendingMu.Lock()
	client.pending[requestID] = pendingRequest{startedAt: startedAt}
	client.pendingMu.Unlock()

	request := map[string]any{
		"op":         "submit_order",
		"request_id": requestID,
		"market":     marketID,
		"side":       side,
		"price":      price,
		"quantity":   quantity,
	}

	payload, err := json.Marshal(request)
	if err != nil {
		m.failPending(client, requestID, startedAt, err)
		return
	}

	client.writeMu.Lock()
	client.conn.SetWriteDeadline(time.Now().Add(m.cfg.RequestTimeout))
	err = client.conn.WriteMessage(websocket.TextMessage, payload)
	client.writeMu.Unlock()
	if err != nil {
		m.failPending(client, requestID, startedAt, err)
	}
}

func (m *wsManager) readLoop(client *wsClient) {
	for {
		_, frame, err := client.conn.ReadMessage()
		if err != nil {
			if atomic.LoadUint32(&client.closing) == 1 {
				return
			}
			m.failAll(client, err)
			return
		}

		var message wsServerMessage
		if err := json.Unmarshal(frame, &message); err != nil {
			continue
		}

		switch message.Type {
		case "heartbeat", "authenticated", "snapshot", "delta", "unsubscribed", "admin_message", "resync_required":
			continue
		case "ack":
			if message.RequestID != nil {
				m.completePending(client, *message.RequestID, true, "")
			}
		case "reject":
			if message.RequestID != nil {
				reason := strings.TrimSpace(strings.Join([]string{message.Code, message.Message}, " "))
				m.completePending(client, *message.RequestID, false, reason)
			}
		case "fill":
			if message.Fill != nil {
				m.stats.recordWSFill(*message.Fill)
			}
		case "order_state":
			if message.Status == "open" && message.Order != nil && message.Order.Remaining > 0 {
				m.stats.recordWSResting()
			}
		case "error":
			m.stats.recordSystemError(strings.TrimSpace(strings.Join([]string{message.Code, message.Message}, " ")))
		}
	}
}

func (m *wsManager) completePending(client *wsClient, requestID string, accepted bool, rejectReason string) {
	client.pendingMu.Lock()
	pending, ok := client.pending[requestID]
	if ok {
		delete(client.pending, requestID)
	}
	client.pendingMu.Unlock()
	if !ok {
		return
	}

	m.stats.recordLatency(time.Since(pending.startedAt))
	if accepted {
		m.stats.recordWSSuccess()
	} else {
		m.stats.recordReject(rejectReason)
	}
	<-m.sem
	m.requestWG.Done()
}

func (m *wsManager) failPending(client *wsClient, requestID string, startedAt time.Time, err error) {
	client.pendingMu.Lock()
	delete(client.pending, requestID)
	client.pendingMu.Unlock()

	m.stats.recordLatency(time.Since(startedAt))
	m.stats.recordReject(fmt.Sprintf("ws send failed: %v", err))
	<-m.sem
	m.requestWG.Done()
}

func (m *wsManager) failAll(client *wsClient, err error) {
	client.pendingMu.Lock()
	pending := make([]pendingRequest, 0, len(client.pending))
	for _, request := range client.pending {
		pending = append(pending, request)
	}
	client.pending = make(map[string]pendingRequest)
	client.pendingMu.Unlock()

	for _, request := range pending {
		m.stats.recordLatency(time.Since(request.startedAt))
		m.stats.recordReject(fmt.Sprintf("ws connection failed: %v", err))
		<-m.sem
		m.requestWG.Done()
	}

	m.stats.recordSystemError(fmt.Sprintf("ws connection for %s closed: %v", client.user.Username, err))
	client.storeCloseErr(err)
}

func (client *wsClient) storeCloseErr(err error) {
	client.closeErrMu.Lock()
	defer client.closeErrMu.Unlock()
	client.closeErr = err
}

func (m *wsManager) Close() error {
	var firstErr error
	for _, client := range m.clients {
		client.closeOnce.Do(func() {
			atomic.StoreUint32(&client.closing, 1)
			if err := client.conn.Close(); err != nil && firstErr == nil {
				firstErr = err
			}
		})
	}
	return firstErr
}
