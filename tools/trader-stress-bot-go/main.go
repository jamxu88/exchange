package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"math/rand"
	"net/http"
	"net/url"
	"os"
	"sort"
	"strings"
	"sync"
	"sync/atomic"
	"time"
)

type config struct {
	BaseURL              string
	AdminToken           string
	Market               string
	Traders              int
	DurationSeconds      int
	OpsPerSecond         int
	Quantity             uint64
	LowerQuantity        uint64
	UpperQuantity        uint64
	LowerPrice           uint64
	UpperPrice           uint64
	PricePattern         string
	MinSpreadTicks       int
	MaxSpreadTicks       int
	MaxDepthTicks        int
	CrossIntervalTicks   int
	RequestTimeout       time.Duration
	ProgressInterval     time.Duration
	ProvisionConcurrency int
	MaxInFlight          int
	Prefix               string
}

type adminStateResponse struct {
	Controls struct {
		TradingEnabled bool `json:"trading_enabled"`
	} `json:"controls"`
}

type marketDefinition struct {
	MarketID         string `json:"market_id"`
	TickSize         uint64 `json:"tick_size"`
	MinOrderQuantity uint64 `json:"min_order_quantity"`
	Status           string `json:"status"`
}

type apiError struct {
	Error string `json:"error"`
}

type provisionUserRequest struct {
	Username string `json:"username"`
	Role     string `json:"role,omitempty"`
}

type provisionUserResponse struct {
	Profile userProfile `json:"profile"`
}

type userProfile struct {
	TraderID string `json:"trader_id"`
	Username string `json:"username"`
	APIKey   string `json:"api_key"`
	Role     string `json:"role"`
}

type submitOrderRequest struct {
	Market   string `json:"market"`
	Side     string `json:"side"`
	Price    uint64 `json:"price"`
	Quantity uint64 `json:"quantity"`
}

type fill struct {
	Quantity uint64 `json:"quantity"`
}

type submitOrderResponse struct {
	Fills   []fill `json:"fills"`
	Resting bool   `json:"resting"`
}

type position struct {
	Market      string `json:"market"`
	NetQuantity int64  `json:"net_quantity"`
}

type portfolioSnapshot struct {
	Positions []position `json:"positions"`
}

type httpError struct {
	Status  int
	Message string
}

func (err *httpError) Error() string {
	return fmt.Sprintf("%d %s", err.Status, err.Message)
}

type stats struct {
	attempted      int64
	succeeded      int64
	rejected       int64
	resting        int64
	fillCount      int64
	filledQuantity int64
	latencyCount   int64
	latencyTotalNS int64

	mu               sync.Mutex
	rejects          map[string]int64
	latencySamplesNS []int64
	latencySeen      int64
}

func newStats() *stats {
	return &stats{
		rejects:          make(map[string]int64),
		latencySamplesNS: make([]int64, 0, 100000),
	}
}

func (s *stats) recordAttempt() {
	atomic.AddInt64(&s.attempted, 1)
}

func (s *stats) recordSuccess(response submitOrderResponse) {
	atomic.AddInt64(&s.succeeded, 1)
	if response.Resting {
		atomic.AddInt64(&s.resting, 1)
	}
	atomic.AddInt64(&s.fillCount, int64(len(response.Fills)))
	var filled uint64
	for _, execution := range response.Fills {
		filled += execution.Quantity
	}
	atomic.AddInt64(&s.filledQuantity, int64(filled))
}

func (s *stats) recordReject(message string) {
	atomic.AddInt64(&s.rejected, 1)
	s.mu.Lock()
	s.rejects[message]++
	s.mu.Unlock()
}

func (s *stats) recordLatency(latency time.Duration) {
	atomic.AddInt64(&s.latencyCount, 1)
	atomic.AddInt64(&s.latencyTotalNS, latency.Nanoseconds())

	s.mu.Lock()
	defer s.mu.Unlock()

	s.latencySeen++
	if len(s.latencySamplesNS) < cap(s.latencySamplesNS) {
		s.latencySamplesNS = append(s.latencySamplesNS, latency.Nanoseconds())
	}
}

func (s *stats) rejectSummary() string {
	s.mu.Lock()
	defer s.mu.Unlock()

	if len(s.rejects) == 0 {
		return "none"
	}

	type rejectEntry struct {
		Message string
		Count   int64
	}

	entries := make([]rejectEntry, 0, len(s.rejects))
	for message, count := range s.rejects {
		entries = append(entries, rejectEntry{Message: message, Count: count})
	}
	sort.Slice(entries, func(i, j int) bool {
		if entries[i].Count == entries[j].Count {
			return entries[i].Message < entries[j].Message
		}
		return entries[i].Count > entries[j].Count
	})

	parts := make([]string, 0, len(entries))
	for _, entry := range entries {
		parts = append(parts, fmt.Sprintf("%dx %s", entry.Count, entry.Message))
	}
	return strings.Join(parts, "; ")
}

func (s *stats) latencySnapshot() []int64 {
	s.mu.Lock()
	defer s.mu.Unlock()

	snapshot := make([]int64, len(s.latencySamplesNS))
	copy(snapshot, s.latencySamplesNS)
	return snapshot
}

func parseConfig() (config, error) {
	var cfg config
	var requestTimeoutMS int
	var progressIntervalMS int

	flag.StringVar(&cfg.BaseURL, "base-url", "http://localhost:8080", "Exchange HTTP base URL")
	flag.StringVar(&cfg.AdminToken, "admin-token", "", "Admin bearer token used for provisioning")
	flag.StringVar(&cfg.Market, "market", "BTC-USD", "Market symbol to trade")
	flag.IntVar(&cfg.Traders, "traders", 150, "Number of simulated traders to provision")
	flag.IntVar(&cfg.DurationSeconds, "duration-seconds", 120, "Wall-clock test duration in seconds")
	flag.IntVar(&cfg.OpsPerSecond, "ops-per-second", 100, "Per-user order submission rate")
	flag.Uint64Var(&cfg.Quantity, "quantity", 1, "Quantity per limit order")
	flag.Uint64Var(&cfg.LowerQuantity, "lower-quantity", 0, "Lower bound for random order quantities; set with --upper-quantity to randomize size")
	flag.Uint64Var(&cfg.UpperQuantity, "upper-quantity", 0, "Upper bound for random order quantities; set with --lower-quantity to randomize size")
	flag.Uint64Var(&cfg.LowerPrice, "lower-price", 90, "Lower bound for random order prices")
	flag.Uint64Var(&cfg.UpperPrice, "upper-price", 110, "Upper bound for random order prices")
	flag.StringVar(&cfg.PricePattern, "price-pattern", "uniform", "Price selection pattern: uniform or random-walk")
	flag.IntVar(&cfg.MinSpreadTicks, "min-spread-ticks", 0, "Minimum inside spread in ticks; 0 preserves same-price crossing behavior")
	flag.IntVar(&cfg.MaxSpreadTicks, "max-spread-ticks", 0, "Maximum inside spread in ticks; set with --min-spread-ticks")
	flag.IntVar(&cfg.MaxDepthTicks, "max-depth-ticks", 0, "Maximum additional depth from the inside quote on each side in ticks")
	flag.IntVar(&cfg.CrossIntervalTicks, "cross-interval-ticks", 0, "Force same-price internal crosses every N scheduler ticks to rebalance inventory; 0 disables")
	flag.IntVar(&requestTimeoutMS, "request-timeout-ms", 10000, "Per-request timeout in milliseconds")
	flag.IntVar(&progressIntervalMS, "progress-interval-ms", 5000, "Progress log interval in milliseconds")
	flag.IntVar(&cfg.ProvisionConcurrency, "provision-concurrency", 25, "Parallelism for user provisioning")
	flag.IntVar(&cfg.MaxInFlight, "max-in-flight", 50000, "Global cap on in-flight HTTP requests")
	flag.StringVar(&cfg.Prefix, "prefix", fmt.Sprintf("go-stress-%d", time.Now().Unix()), "Username prefix for provisioned accounts")
	flag.Parse()

	cfg.RequestTimeout = time.Duration(requestTimeoutMS) * time.Millisecond
	cfg.ProgressInterval = time.Duration(progressIntervalMS) * time.Millisecond

	if cfg.AdminToken == "" {
		return cfg, errors.New("--admin-token is required")
	}
	if cfg.Traders <= 0 {
		return cfg, errors.New("--traders must be positive")
	}
	if cfg.Traders%2 != 0 {
		return cfg, errors.New("--traders must be even so users can be paired")
	}
	if cfg.DurationSeconds <= 0 {
		return cfg, errors.New("--duration-seconds must be positive")
	}
	if cfg.OpsPerSecond <= 0 {
		return cfg, errors.New("--ops-per-second must be positive")
	}
	if cfg.Quantity == 0 {
		return cfg, errors.New("--quantity must be positive")
	}
	if (cfg.LowerQuantity == 0) != (cfg.UpperQuantity == 0) {
		return cfg, errors.New("--lower-quantity and --upper-quantity must be provided together")
	}
	if cfg.LowerQuantity > 0 && cfg.LowerQuantity > cfg.UpperQuantity {
		return cfg, errors.New("--lower-quantity must be less than or equal to --upper-quantity")
	}
	if cfg.LowerPrice == 0 || cfg.UpperPrice == 0 {
		return cfg, errors.New("--lower-price and --upper-price must be positive")
	}
	if cfg.LowerPrice >= cfg.UpperPrice {
		return cfg, errors.New("--lower-price must be below --upper-price")
	}
	if cfg.PricePattern != "uniform" && cfg.PricePattern != "random-walk" {
		return cfg, errors.New("--price-pattern must be one of: uniform, random-walk")
	}
	if cfg.MinSpreadTicks < 0 || cfg.MaxSpreadTicks < 0 {
		return cfg, errors.New("--min-spread-ticks and --max-spread-ticks must be non-negative")
	}
	if cfg.MaxDepthTicks < 0 {
		return cfg, errors.New("--max-depth-ticks must be non-negative")
	}
	if cfg.CrossIntervalTicks < 0 {
		return cfg, errors.New("--cross-interval-ticks must be non-negative")
	}
	if cfg.MinSpreadTicks > cfg.MaxSpreadTicks {
		return cfg, errors.New("--min-spread-ticks must be less than or equal to --max-spread-ticks")
	}
	if cfg.ProvisionConcurrency <= 0 {
		return cfg, errors.New("--provision-concurrency must be positive")
	}
	if cfg.MaxInFlight <= 0 {
		return cfg, errors.New("--max-in-flight must be positive")
	}
	if cfg.RequestTimeout <= 0 {
		return cfg, errors.New("--request-timeout-ms must be positive")
	}
	if cfg.ProgressInterval <= 0 {
		return cfg, errors.New("--progress-interval-ms must be positive")
	}

	if _, err := url.Parse(cfg.BaseURL); err != nil {
		return cfg, fmt.Errorf("invalid --base-url: %w", err)
	}

	return cfg, nil
}

func main() {
	cfg, err := parseConfig()
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}

	if err := run(cfg); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func run(cfg config) error {
	poolSize := recommendedConnPoolSize(cfg.MaxInFlight)
	client := &http.Client{
		Timeout: cfg.RequestTimeout,
		Transport: &http.Transport{
			Proxy:                 http.ProxyFromEnvironment,
			ForceAttemptHTTP2:     false,
			MaxIdleConns:          poolSize,
			MaxIdleConnsPerHost:   poolSize,
			IdleConnTimeout:       90 * time.Second,
			TLSHandshakeTimeout:   10 * time.Second,
			ExpectContinueTimeout: 1 * time.Second,
			DisableCompression:    true,
		},
	}

	adminState, err := getAdminState(client, cfg)
	if err != nil {
		return err
	}
	if !adminState.Controls.TradingEnabled {
		return errors.New("trading is disabled on the target exchange")
	}

	markets, err := getMarkets(client, cfg)
	if err != nil {
		return err
	}
	market, err := findMarket(markets, cfg.Market)
	if err != nil {
		return err
	}
	if cfg.Quantity < market.MinOrderQuantity {
		return fmt.Errorf("configured quantity %d is below market minimum %d", cfg.Quantity, market.MinOrderQuantity)
	}
	quantityLower, quantityUpper := quantityBounds(cfg)
	if quantityLower < market.MinOrderQuantity {
		return fmt.Errorf("configured lower quantity %d is below market minimum %d", quantityLower, market.MinOrderQuantity)
	}

	prices, err := buildPrices(cfg.LowerPrice, cfg.UpperPrice, market.TickSize)
	if err != nil {
		return err
	}

	fmt.Printf(
		"Provisioning %d traders on %s. Duration=%ds Rate=%d/s PriceBand=%d-%d Prices=%d Qty=%d-%d Pattern=%s Spread=%d-%d ticks Depth=0-%d ticks CrossEvery=%d ticks MaxInFlight=%d\n",
		cfg.Traders,
		market.MarketID,
		cfg.DurationSeconds,
		cfg.OpsPerSecond,
		prices[0],
		prices[len(prices)-1],
		len(prices),
		quantityLower,
		quantityUpper,
		cfg.PricePattern,
		cfg.MinSpreadTicks,
		cfg.MaxSpreadTicks,
		cfg.MaxDepthTicks,
		cfg.CrossIntervalTicks,
		cfg.MaxInFlight,
	)

	users, err := provisionUsers(client, cfg)
	if err != nil {
		return err
	}
	fmt.Printf("Provisioned %d trader accounts.\n", len(users))

	runStats := newStats()
	expectedOrders := int64(cfg.Traders * cfg.DurationSeconds * cfg.OpsPerSecond)
	startedAt := time.Now()
	done := make(chan struct{})
	go logProgress(runStats, expectedOrders, startedAt, cfg.ProgressInterval, done)

	if err := runDurationTest(client, cfg, users, market, prices, runStats); err != nil {
		close(done)
		return err
	}
	close(done)

	elapsed := time.Since(startedAt)
	positionSummary, err := aggregatePositions(client, cfg, users)
	if err != nil {
		positionSummary = fmt.Sprintf("position summary unavailable: %v", err)
	}

	latenciesNS := runStats.latencySnapshot()
	fmt.Println("")
	fmt.Println("Run complete")
	fmt.Printf("  Market: %s\n", market.MarketID)
	fmt.Printf("  Provisioned users: %d\n", len(users))
	fmt.Printf("  Target orders: %d\n", expectedOrders)
	fmt.Printf("  Orders attempted: %d\n", atomic.LoadInt64(&runStats.attempted))
	fmt.Printf("  Orders accepted: %d\n", atomic.LoadInt64(&runStats.succeeded))
	fmt.Printf("  Orders rejected: %d\n", atomic.LoadInt64(&runStats.rejected))
	fmt.Printf("  Resting trader orders: %d\n", atomic.LoadInt64(&runStats.resting))
	fmt.Printf("  Taker fill events: %d\n", atomic.LoadInt64(&runStats.fillCount))
	fmt.Printf("  Taker filled quantity: %d\n", atomic.LoadInt64(&runStats.filledQuantity))
	fmt.Printf("  Elapsed: %.2fs\n", elapsed.Seconds())
	fmt.Printf("  Throughput: %.1f req/s\n", float64(atomic.LoadInt64(&runStats.attempted))/elapsed.Seconds())
	latencyCount := atomic.LoadInt64(&runStats.latencyCount)
	if latencyCount > 0 {
		avgMS := float64(atomic.LoadInt64(&runStats.latencyTotalNS)) / float64(latencyCount) / float64(time.Millisecond)
		fmt.Printf("  Avg latency: %.2fms\n", avgMS)
		fmt.Printf("  P50 latency: %.2fms\n", durationPercentileMS(latenciesNS, 0.50))
		fmt.Printf("  P95 latency: %.2fms\n", durationPercentileMS(latenciesNS, 0.95))
		fmt.Printf("  P99 latency: %.2fms\n", durationPercentileMS(latenciesNS, 0.99))
	} else {
		fmt.Printf("  Avg latency: n/a\n")
		fmt.Printf("  P50 latency: n/a\n")
		fmt.Printf("  P95 latency: n/a\n")
		fmt.Printf("  P99 latency: n/a\n")
	}
	fmt.Printf("  Reject summary: %s\n", runStats.rejectSummary())
	fmt.Printf("  Aggregate end positions: %s\n", positionSummary)

	return nil
}

func getAdminState(client *http.Client, cfg config) (adminStateResponse, error) {
	var response adminStateResponse
	err := requestJSON(
		client,
		cfg.BaseURL,
		http.MethodGet,
		"/api/v1/admin/state",
		map[string]string{"Authorization": "Bearer " + cfg.AdminToken},
		nil,
		&response,
		cfg.RequestTimeout,
	)
	return response, err
}

func getMarkets(client *http.Client, cfg config) ([]marketDefinition, error) {
	var response []marketDefinition
	err := requestJSON(client, cfg.BaseURL, http.MethodGet, "/api/v1/markets", nil, nil, &response, cfg.RequestTimeout)
	return response, err
}

func provisionUsers(client *http.Client, cfg config) ([]userProfile, error) {
	type result struct {
		Index int
		User  userProfile
		Err   error
	}

	results := make(chan result, cfg.Traders)
	work := make(chan int)
	var wg sync.WaitGroup

	workerCount := cfg.ProvisionConcurrency
	if workerCount > cfg.Traders {
		workerCount = cfg.Traders
	}

	for worker := 0; worker < workerCount; worker++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for index := range work {
				username := fmt.Sprintf("%s-trader-%03d", cfg.Prefix, index+1)
				response, err := provisionUser(client, cfg, provisionUserRequest{
					Username: username,
					Role:     "trader",
				})
				results <- result{Index: index, User: response.Profile, Err: err}
			}
		}()
	}

	go func() {
		for index := 0; index < cfg.Traders; index++ {
			work <- index
		}
		close(work)
		wg.Wait()
		close(results)
	}()

	users := make([]userProfile, cfg.Traders)
	var firstErr error
	for item := range results {
		if item.Err != nil && firstErr == nil {
			firstErr = item.Err
			continue
		}
		users[item.Index] = item.User
	}
	if firstErr != nil {
		return nil, firstErr
	}
	return users, nil
}

func provisionUser(client *http.Client, cfg config, request provisionUserRequest) (provisionUserResponse, error) {
	var response provisionUserResponse
	err := requestJSON(
		client,
		cfg.BaseURL,
		http.MethodPost,
		"/api/v1/admin/users",
		map[string]string{"Authorization": "Bearer " + cfg.AdminToken},
		request,
		&response,
		cfg.RequestTimeout,
	)
	return response, err
}

func runDurationTest(
	client *http.Client,
	cfg config,
	users []userProfile,
	market marketDefinition,
	prices []uint64,
	runStats *stats,
) error {
	type traderPair struct {
		Left  userProfile
		Right userProfile
	}

	pairs := make([]traderPair, 0, len(users)/2)
	for index := 0; index < len(users); index += 2 {
		pairs = append(pairs, traderPair{Left: users[index], Right: users[index+1]})
	}

	startAt := time.Now().Add(500 * time.Millisecond)
	sem := make(chan struct{}, cfg.MaxInFlight)
	var requestWG sync.WaitGroup
	var schedulerWG sync.WaitGroup

	for pairIndex, pair := range pairs {
		schedulerWG.Add(1)
		go func(pairIndex int, pair traderPair) {
			defer schedulerWG.Done()

			rng := rand.New(rand.NewSource(time.Now().UnixNano() + int64(pairIndex+1)*7919))
			offset := pairOffset(pairIndex, len(pairs), cfg.OpsPerSecond)
			ticks := cfg.DurationSeconds * cfg.OpsPerSecond
			priceIndex := rng.Intn(len(prices))

			for tick := 0; tick < ticks; tick++ {
				target := startAt.Add(offset).Add(durationForTick(tick, cfg.OpsPerSecond))
				sleepUntil(target)

				priceIndex = nextPriceIndex(cfg.PricePattern, priceIndex, len(prices), rng)
				quantity := randomQuantity(cfg, rng)
				leftBuys := rng.Intn(2) == 0
				buyPrice, sellPrice := quotePrices(prices, priceIndex, cfg, tick, rng)
				dispatchOrder(client, cfg, market.MarketID, pair.Left.APIKey, sideString(leftBuys), sidePrice(leftBuys, buyPrice, sellPrice), quantity, runStats, sem, &requestWG)
				dispatchOrder(client, cfg, market.MarketID, pair.Right.APIKey, sideString(!leftBuys), sidePrice(!leftBuys, buyPrice, sellPrice), quantity, runStats, sem, &requestWG)
			}
		}(pairIndex, pair)
	}

	schedulerWG.Wait()
	fmt.Printf("Issued all %d scheduled orders. Waiting for completions.\n", cfg.Traders*cfg.DurationSeconds*cfg.OpsPerSecond)
	requestWG.Wait()
	return nil
}

func dispatchOrder(
	client *http.Client,
	cfg config,
	marketID string,
	apiKey string,
	side string,
	price uint64,
	quantity uint64,
	runStats *stats,
	sem chan struct{},
	requestWG *sync.WaitGroup,
) {
	sem <- struct{}{}
	requestWG.Add(1)

	go func() {
		defer requestWG.Done()
		defer func() { <-sem }()

		runStats.recordAttempt()
		startedAt := time.Now()

		response, err := submitOrder(client, cfg, apiKey, submitOrderRequest{
			Market:   marketID,
			Side:     side,
			Price:    price,
			Quantity: quantity,
		})
		runStats.recordLatency(time.Since(startedAt))
		if err != nil {
			runStats.recordReject(err.Error())
			return
		}
		runStats.recordSuccess(response)
	}()
}

func submitOrder(client *http.Client, cfg config, apiKey string, request submitOrderRequest) (submitOrderResponse, error) {
	var response submitOrderResponse
	err := requestJSON(
		client,
		cfg.BaseURL,
		http.MethodPost,
		"/api/v1/orders",
		map[string]string{"x-api-key": apiKey},
		request,
		&response,
		cfg.RequestTimeout,
	)
	return response, err
}

func aggregatePositions(client *http.Client, cfg config, users []userProfile) (string, error) {
	aggregate := make(map[string]int64)
	for _, user := range users {
		portfolio, err := getPortfolio(client, cfg, user.APIKey)
		if err != nil {
			return "", err
		}
		for _, position := range portfolio.Positions {
			aggregate[position.Market] += position.NetQuantity
		}
	}

	if len(aggregate) == 0 {
		return "flat", nil
	}

	markets := make([]string, 0, len(aggregate))
	for market := range aggregate {
		markets = append(markets, market)
	}
	sort.Strings(markets)

	parts := make([]string, 0, len(markets))
	for _, market := range markets {
		parts = append(parts, fmt.Sprintf("%s:%d", market, aggregate[market]))
	}
	return strings.Join(parts, ", "), nil
}

func getPortfolio(client *http.Client, cfg config, apiKey string) (portfolioSnapshot, error) {
	var response portfolioSnapshot
	err := requestJSON(
		client,
		cfg.BaseURL,
		http.MethodGet,
		"/api/v1/portfolio",
		map[string]string{"x-api-key": apiKey},
		nil,
		&response,
		cfg.RequestTimeout,
	)
	return response, err
}

func requestJSON(
	client *http.Client,
	baseURL string,
	method string,
	path string,
	headers map[string]string,
	requestBody interface{},
	responseBody interface{},
	timeout time.Duration,
) error {
	endpoint, err := url.Parse(strings.TrimRight(baseURL, "/") + path)
	if err != nil {
		return err
	}

	var bodyReader io.Reader
	if requestBody != nil {
		payload, err := json.Marshal(requestBody)
		if err != nil {
			return err
		}
		bodyReader = bytes.NewReader(payload)
	}

	ctx, cancel := context.WithTimeout(context.Background(), timeout)
	defer cancel()

	request, err := http.NewRequestWithContext(ctx, method, endpoint.String(), bodyReader)
	if err != nil {
		return err
	}
	request.Header.Set("Accept", "application/json")
	if requestBody != nil {
		request.Header.Set("Content-Type", "application/json")
	}
	for key, value := range headers {
		request.Header.Set(key, value)
	}

	response, err := client.Do(request)
	if err != nil {
		return err
	}
	defer response.Body.Close()

	payload, err := io.ReadAll(response.Body)
	if err != nil {
		return err
	}

	if response.StatusCode < 200 || response.StatusCode >= 300 {
		var parsed apiError
		if err := json.Unmarshal(payload, &parsed); err == nil && parsed.Error != "" {
			return &httpError{Status: response.StatusCode, Message: parsed.Error}
		}
		message := strings.TrimSpace(string(payload))
		if message == "" {
			message = response.Status
		}
		return &httpError{Status: response.StatusCode, Message: message}
	}

	if responseBody == nil || len(payload) == 0 {
		return nil
	}
	return json.Unmarshal(payload, responseBody)
}

func findMarket(markets []marketDefinition, marketID string) (marketDefinition, error) {
	for _, market := range markets {
		if market.MarketID == marketID {
			if strings.ToLower(market.Status) != "enabled" {
				return marketDefinition{}, fmt.Errorf("market %s is not enabled", marketID)
			}
			return market, nil
		}
	}
	return marketDefinition{}, fmt.Errorf("market %s not found", marketID)
}

func buildPrices(lower, upper, tick uint64) ([]uint64, error) {
	if tick == 0 {
		return nil, errors.New("market tick size must be positive")
	}

	alignedLower := alignUp(lower, tick)
	alignedUpper := alignDown(upper, tick)
	if alignedLower > alignedUpper {
		return nil, errors.New("price band collapses after tick-size alignment")
	}

	prices := make([]uint64, 0, int((alignedUpper-alignedLower)/tick)+1)
	for price := alignedLower; price <= alignedUpper; price += tick {
		prices = append(prices, price)
		if alignedUpper-price < tick {
			break
		}
	}
	if len(prices) == 0 {
		return nil, errors.New("price band produced no valid prices")
	}
	return prices, nil
}

func quantityBounds(cfg config) (uint64, uint64) {
	if cfg.LowerQuantity > 0 && cfg.UpperQuantity > 0 {
		return cfg.LowerQuantity, cfg.UpperQuantity
	}
	return cfg.Quantity, cfg.Quantity
}

func randomQuantity(cfg config, rng *rand.Rand) uint64 {
	lower, upper := quantityBounds(cfg)
	if lower == upper {
		return lower
	}
	return lower + uint64(rng.Int63n(int64(upper-lower+1)))
}

func quotePrices(prices []uint64, centerIndex int, cfg config, tick int, rng *rand.Rand) (uint64, uint64) {
	if len(prices) == 0 {
		return 0, 0
	}
	centerPrice := prices[clampInt(centerIndex, 0, len(prices)-1)]
	if cfg.CrossIntervalTicks > 0 && (tick+1)%cfg.CrossIntervalTicks == 0 {
		return centerPrice, centerPrice
	}
	if cfg.MaxSpreadTicks == 0 && cfg.MinSpreadTicks == 0 && cfg.MaxDepthTicks == 0 {
		return centerPrice, centerPrice
	}

	spreadTicks := cfg.MinSpreadTicks
	if cfg.MaxSpreadTicks > cfg.MinSpreadTicks {
		spreadTicks += rng.Intn(cfg.MaxSpreadTicks - cfg.MinSpreadTicks + 1)
	}
	if spreadTicks > len(prices)-1 {
		spreadTicks = len(prices) - 1
	}

	bidBaseMax := len(prices) - 1 - spreadTicks
	bidBaseIndex := clampInt(centerIndex-(spreadTicks/2), 0, bidBaseMax)
	askBaseIndex := bidBaseIndex + spreadTicks

	bidDepth := 0
	askDepth := 0
	if cfg.MaxDepthTicks > 0 {
		bidDepth = rng.Intn(cfg.MaxDepthTicks + 1)
		askDepth = rng.Intn(cfg.MaxDepthTicks + 1)
	}

	bidIndex := clampInt(bidBaseIndex-bidDepth, 0, len(prices)-1)
	askIndex := clampInt(askBaseIndex+askDepth, 0, len(prices)-1)
	if askIndex < bidIndex {
		askIndex = bidIndex
	}
	return prices[bidIndex], prices[askIndex]
}

func alignUp(value, tick uint64) uint64 {
	if value%tick == 0 {
		return value
	}
	return value + tick - (value % tick)
}

func alignDown(value, tick uint64) uint64 {
	return value - (value % tick)
}

func sideString(isBuy bool) string {
	if isBuy {
		return "BUY"
	}
	return "SELL"
}

func sidePrice(isBuy bool, buyPrice, sellPrice uint64) uint64 {
	if isBuy {
		return buyPrice
	}
	return sellPrice
}

func nextPriceIndex(pattern string, current, size int, rng *rand.Rand) int {
	if size <= 1 {
		return 0
	}
	if pattern != "random-walk" {
		return rng.Intn(size)
	}

	next := current + rng.Intn(3) - 1
	if next < 0 {
		return 0
	}
	if next >= size {
		return size - 1
	}
	return next
}

func pairOffset(pairIndex, pairCount, opsPerSecond int) time.Duration {
	if pairCount <= 1 {
		return 0
	}
	intervalNS := float64(time.Second) / float64(opsPerSecond)
	return time.Duration((float64(pairIndex) / float64(pairCount)) * intervalNS)
}

func clampInt(value, lower, upper int) int {
	if value < lower {
		return lower
	}
	if value > upper {
		return upper
	}
	return value
}

func recommendedConnPoolSize(maxInFlight int) int {
	poolSize := maxInFlight / 8
	if poolSize < 64 {
		return 64
	}
	if poolSize > 1024 {
		return 1024
	}
	return poolSize
}

func durationForTick(tick, opsPerSecond int) time.Duration {
	return time.Duration(float64(tick) * float64(time.Second) / float64(opsPerSecond))
}

func sleepUntil(target time.Time) {
	delay := time.Until(target)
	if delay > 0 {
		time.Sleep(delay)
	}
}

func logProgress(runStats *stats, expectedOrders int64, startedAt time.Time, interval time.Duration, done <-chan struct{}) {
	ticker := time.NewTicker(interval)
	defer ticker.Stop()

	for {
		select {
		case <-done:
			return
		case <-ticker.C:
			elapsed := time.Since(startedAt).Seconds()
			if elapsed <= 0 {
				elapsed = 0.001
			}
			attempted := atomic.LoadInt64(&runStats.attempted)
			succeeded := atomic.LoadInt64(&runStats.succeeded)
			rejected := atomic.LoadInt64(&runStats.rejected)
			completed := succeeded + rejected
			fmt.Printf(
				"Progress %d/%d attempts | ok=%d reject=%d issued=%.1f req/s completed=%.1f req/s\n",
				attempted,
				expectedOrders,
				succeeded,
				rejected,
				float64(attempted)/elapsed,
				float64(completed)/elapsed,
			)
		}
	}
}

func durationPercentileMS(values []int64, fraction float64) float64 {
	if len(values) == 0 {
		return 0
	}
	sort.Slice(values, func(i, j int) bool { return values[i] < values[j] })
	index := int(float64(len(values))*fraction) - 1
	if index < 0 {
		index = 0
	}
	if index >= len(values) {
		index = len(values) - 1
	}
	return float64(values[index]) / float64(time.Millisecond)
}
