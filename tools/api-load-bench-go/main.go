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

type transportMode string

const (
	modeREST  transportMode = "rest"
	modeWS    transportMode = "ws"
	modeMixed transportMode = "mixed"
	modeAll   transportMode = "all"
)

type config struct {
	BaseURL              string
	AdminToken           string
	Mode                 transportMode
	ProvisionRole        string
	Market               string
	Traders              int
	ObserveL2Clients     int
	ObserveL3Clients     int
	DurationSeconds      int
	StartOpsPerSecond    int
	StepOpsPerSecond     int
	MaxOpsPerSecond      int
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
	MaxRejectRate        float64
	MaxP95Latency        time.Duration
	MinAchievedRatio     float64
	RequireFeedAccuracy  bool
	ObserverSettleDelay  time.Duration
	ResetBeforeSuite     bool
	ResetBetweenTrials   bool
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
	Price    uint64 `json:"price"`
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
	systemErrors   int64

	mu               sync.Mutex
	rejects          map[string]int64
	latencySamplesNS []int64
}

type traderPair struct {
	left       userProfile
	right      userProfile
	rng        *rand.Rand
	priceIndex int
}

type trialResult struct {
	mode            transportMode
	targetOps       int
	elapsed         time.Duration
	stats           *stats
	p50MS           float64
	p95MS           float64
	p99MS           float64
	avgMS           float64
	rejectRate      float64
	achievedRatio   float64
	positionSummary string
	feedSummary     string
	passed          bool
	reasons         []string
}

type feedValidationResult struct {
	summary string
	reasons []string
}

func newStats() *stats {
	return &stats{
		rejects:          make(map[string]int64),
		latencySamplesNS: make([]int64, 0, 200000),
	}
}

func (s *stats) recordAttempt() {
	atomic.AddInt64(&s.attempted, 1)
}

func (s *stats) recordRESTSuccess(response submitOrderResponse) {
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

func (s *stats) recordWSSuccess() {
	atomic.AddInt64(&s.succeeded, 1)
}

func (s *stats) recordWSFill(fill fill) {
	atomic.AddInt64(&s.fillCount, 1)
	atomic.AddInt64(&s.filledQuantity, int64(fill.Quantity))
}

func (s *stats) recordWSResting() {
	atomic.AddInt64(&s.resting, 1)
}

func (s *stats) recordReject(message string) {
	atomic.AddInt64(&s.rejected, 1)
	s.mu.Lock()
	s.rejects[normalizeRejectMessage(message)]++
	s.mu.Unlock()
}

func (s *stats) recordSystemError(message string) {
	atomic.AddInt64(&s.systemErrors, 1)
	s.mu.Lock()
	s.rejects["system: "+normalizeRejectMessage(message)]++
	s.mu.Unlock()
}

func (s *stats) recordLatency(latency time.Duration) {
	atomic.AddInt64(&s.latencyCount, 1)
	atomic.AddInt64(&s.latencyTotalNS, latency.Nanoseconds())

	s.mu.Lock()
	if len(s.latencySamplesNS) < cap(s.latencySamplesNS) {
		s.latencySamplesNS = append(s.latencySamplesNS, latency.Nanoseconds())
	}
	s.mu.Unlock()
}

func normalizeRejectMessage(message string) string {
	message = strings.TrimSpace(message)
	if message == "" {
		return "unknown"
	}
	return message
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
	var maxP95MS int
	var observerSettleMS int
	var mode string

	flag.StringVar(&cfg.BaseURL, "base-url", "http://localhost:8080", "Exchange HTTP base URL")
	flag.StringVar(&cfg.AdminToken, "admin-token", "admin", "Admin bearer token used for provisioning and admin endpoints")
	flag.StringVar(&mode, "mode", string(modeAll), "Benchmark mode: rest, ws, mixed, or all")
	flag.StringVar(&cfg.ProvisionRole, "provision-role", "trader", "Role to provision for benchmark users: trader or admin")
	flag.StringVar(&cfg.Market, "market", "BTC-USD", "Market symbol to trade")
	flag.IntVar(&cfg.Traders, "traders", 50, "Number of simulated traders to provision")
	flag.IntVar(&cfg.ObserveL2Clients, "observe-l2-clients", 1, "Number of observer sockets that validate the aggregated l2 feed")
	flag.IntVar(&cfg.ObserveL3Clients, "observe-l3-clients", 1, "Number of observer sockets that validate the authenticated l3 feed")
	flag.IntVar(&cfg.DurationSeconds, "duration-seconds", 20, "Per-trial duration in seconds")
	flag.IntVar(&cfg.StartOpsPerSecond, "start-ops-per-second", 100, "Initial global target order rate in ops/sec")
	flag.IntVar(&cfg.StepOpsPerSecond, "step-ops-per-second", 100, "Increment for each global target ops/sec trial")
	flag.IntVar(&cfg.MaxOpsPerSecond, "max-ops-per-second", 2000, "Maximum global target order rate in ops/sec")
	flag.Uint64Var(&cfg.Quantity, "quantity", 1, "Fixed order quantity when lower/upper quantity are omitted")
	flag.Uint64Var(&cfg.LowerQuantity, "lower-quantity", 0, "Lower bound for randomized order quantities")
	flag.Uint64Var(&cfg.UpperQuantity, "upper-quantity", 0, "Upper bound for randomized order quantities")
	flag.Uint64Var(&cfg.LowerPrice, "lower-price", 90, "Lower bound for price band")
	flag.Uint64Var(&cfg.UpperPrice, "upper-price", 110, "Upper bound for price band")
	flag.StringVar(&cfg.PricePattern, "price-pattern", "random-walk", "Price selection pattern: uniform or random-walk")
	flag.IntVar(&cfg.MinSpreadTicks, "min-spread-ticks", 0, "Minimum spread in ticks")
	flag.IntVar(&cfg.MaxSpreadTicks, "max-spread-ticks", 2, "Maximum spread in ticks")
	flag.IntVar(&cfg.MaxDepthTicks, "max-depth-ticks", 4, "Maximum additional depth from the inside price in ticks")
	flag.IntVar(&cfg.CrossIntervalTicks, "cross-interval-ticks", 8, "Force same-price crosses every N pair events; 0 disables")
	flag.IntVar(&requestTimeoutMS, "request-timeout-ms", 10000, "Per-request timeout in milliseconds")
	flag.IntVar(&progressIntervalMS, "progress-interval-ms", 5000, "Progress log interval in milliseconds")
	flag.IntVar(&cfg.ProvisionConcurrency, "provision-concurrency", 25, "Parallelism for user provisioning")
	flag.IntVar(&cfg.MaxInFlight, "max-in-flight", 25000, "Global cap on in-flight operations")
	flag.Float64Var(&cfg.MaxRejectRate, "max-reject-rate", 0.01, "Maximum reject ratio allowed for a passing trial")
	flag.IntVar(&maxP95MS, "max-p95-ms", 250, "Maximum p95 latency in milliseconds for a passing trial")
	flag.Float64Var(&cfg.MinAchievedRatio, "min-achieved-ratio", 0.95, "Minimum completion ratio required for a passing trial")
	flag.BoolVar(&cfg.RequireFeedAccuracy, "require-feed-accuracy", true, "Fail a trial when observer feeds gap, resync, or diverge from a fresh snapshot")
	flag.IntVar(&observerSettleMS, "observer-settle-ms", 300, "How long to wait after load completes before validating observer feed state")
	flag.BoolVar(&cfg.ResetBeforeSuite, "reset-before-suite", false, "Call the admin reset endpoint before running benchmarks")
	flag.BoolVar(&cfg.ResetBetweenTrials, "reset-between-trials", false, "Call the admin reset endpoint between trials")
	flag.StringVar(&cfg.Prefix, "prefix", fmt.Sprintf("api-bench-%d", time.Now().Unix()), "Username prefix for provisioned accounts")
	flag.Parse()

	cfg.RequestTimeout = time.Duration(requestTimeoutMS) * time.Millisecond
	cfg.ProgressInterval = time.Duration(progressIntervalMS) * time.Millisecond
	cfg.MaxP95Latency = time.Duration(maxP95MS) * time.Millisecond
	cfg.ObserverSettleDelay = time.Duration(observerSettleMS) * time.Millisecond
	cfg.Mode = transportMode(strings.ToLower(strings.TrimSpace(mode)))

	switch cfg.Mode {
	case modeREST, modeWS, modeMixed, modeAll:
	default:
		return cfg, errors.New("--mode must be one of: rest, ws, mixed, all")
	}
	if cfg.ProvisionRole != "trader" && cfg.ProvisionRole != "admin" {
		return cfg, errors.New("--provision-role must be one of: trader, admin")
	}
	if cfg.AdminToken == "" {
		return cfg, errors.New("--admin-token is required")
	}
	if cfg.Traders <= 0 {
		return cfg, errors.New("--traders must be positive")
	}
	if cfg.ObserveL2Clients < 0 || cfg.ObserveL3Clients < 0 {
		return cfg, errors.New("--observe-l2-clients and --observe-l3-clients must be non-negative")
	}
	if cfg.Traders%2 != 0 {
		return cfg, errors.New("--traders must be even so traders can be paired")
	}
	if cfg.DurationSeconds <= 0 {
		return cfg, errors.New("--duration-seconds must be positive")
	}
	if cfg.StartOpsPerSecond <= 0 || cfg.StepOpsPerSecond <= 0 || cfg.MaxOpsPerSecond <= 0 {
		return cfg, errors.New("--start-ops-per-second, --step-ops-per-second, and --max-ops-per-second must be positive")
	}
	if cfg.StartOpsPerSecond > cfg.MaxOpsPerSecond {
		return cfg, errors.New("--start-ops-per-second must be less than or equal to --max-ops-per-second")
	}
	if cfg.StartOpsPerSecond%2 != 0 || cfg.StepOpsPerSecond%2 != 0 || cfg.MaxOpsPerSecond%2 != 0 {
		return cfg, errors.New("ops-per-second values must be even because each scheduled event submits a buy and a sell")
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
	if cfg.MinSpreadTicks < 0 || cfg.MaxSpreadTicks < 0 || cfg.MaxDepthTicks < 0 || cfg.CrossIntervalTicks < 0 {
		return cfg, errors.New("spread/depth/cross settings must be non-negative")
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
	if cfg.MaxRejectRate < 0 || cfg.MaxRejectRate > 1 {
		return cfg, errors.New("--max-reject-rate must be between 0 and 1")
	}
	if cfg.MinAchievedRatio <= 0 || cfg.MinAchievedRatio > 1 {
		return cfg, errors.New("--min-achieved-ratio must be between 0 and 1")
	}
	if cfg.RequestTimeout <= 0 || cfg.ProgressInterval <= 0 || cfg.MaxP95Latency <= 0 {
		return cfg, errors.New("timing parameters must be positive")
	}
	if cfg.ObserverSettleDelay <= 0 {
		return cfg, errors.New("--observer-settle-ms must be positive")
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
	client := &http.Client{
		Timeout: cfg.RequestTimeout,
		Transport: &http.Transport{
			Proxy:                 http.ProxyFromEnvironment,
			ForceAttemptHTTP2:     false,
			MaxIdleConns:          recommendedConnPoolSize(cfg.MaxInFlight),
			MaxIdleConnsPerHost:   recommendedConnPoolSize(cfg.MaxInFlight),
			IdleConnTimeout:       90 * time.Second,
			TLSHandshakeTimeout:   10 * time.Second,
			ExpectContinueTimeout: time.Second,
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

	quantityLower, _ := quantityBounds(cfg)
	if quantityLower < market.MinOrderQuantity {
		return fmt.Errorf("configured lower quantity %d is below market minimum %d", quantityLower, market.MinOrderQuantity)
	}
	prices, err := buildPrices(cfg.LowerPrice, cfg.UpperPrice, market.TickSize)
	if err != nil {
		return err
	}

	if cfg.ResetBeforeSuite {
		fmt.Println("Resetting user trading state before benchmark suite.")
		if err := resetUsers(client, cfg); err != nil {
			return err
		}
	}

	modes := selectedModes(cfg.Mode)
	results := make([]trialResult, 0, len(modes))
	for _, mode := range modes {
		fmt.Println("")
		fmt.Printf("== %s benchmark ==\n", strings.ToUpper(string(mode)))
		result, err := runModeBenchmark(client, cfg, market, prices, mode)
		if err != nil {
			return err
		}
		results = append(results, result)
	}

	fmt.Println("")
	fmt.Println("Benchmark summary")
	for _, result := range results {
		fmt.Printf(
			"  %s: best target=%d ops/s | pass=%t | p95=%.2fms | reject-rate=%.2f%% | completed=%.1f%%\n",
			strings.ToUpper(string(result.mode)),
			result.targetOps,
			result.passed,
			result.p95MS,
			result.rejectRate*100,
			result.achievedRatio*100,
		)
	}

	return nil
}

func selectedModes(mode transportMode) []transportMode {
	if mode == modeAll {
		return []transportMode{modeREST, modeWS, modeMixed}
	}
	return []transportMode{mode}
}

func runModeBenchmark(
	client *http.Client,
	cfg config,
	market marketDefinition,
	prices []uint64,
	mode transportMode,
) (trialResult, error) {
	var best *trialResult

	for targetOps := cfg.StartOpsPerSecond; targetOps <= cfg.MaxOpsPerSecond; targetOps += cfg.StepOpsPerSecond {
		if best != nil && cfg.ResetBetweenTrials {
			fmt.Println("Resetting user trading state between trials.")
			if err := resetUsers(client, cfg); err != nil {
				return trialResult{}, err
			}
		}

		quantityLower, quantityUpper := quantityBounds(cfg)
		fmt.Printf(
			"Trial target=%d ops/s duration=%ds traders=%d quantity=%d-%d pattern=%s spread=%d-%d depth=0-%d crossEvery=%d\n",
			targetOps,
			cfg.DurationSeconds,
			cfg.Traders,
			quantityLower,
			quantityUpper,
			cfg.PricePattern,
			cfg.MinSpreadTicks,
			cfg.MaxSpreadTicks,
			cfg.MaxDepthTicks,
			cfg.CrossIntervalTicks,
		)

		result, err := runTrial(client, cfg, market, prices, mode, targetOps)
		if err != nil {
			return trialResult{}, err
		}
		printTrial(result)

		if result.passed {
			snapshot := result
			best = &snapshot
			continue
		}

		if best == nil {
			return result, nil
		}
		return *best, nil
	}

	if best == nil {
		return trialResult{mode: mode}, nil
	}
	return *best, nil
}

func runTrial(
	client *http.Client,
	cfg config,
	market marketDefinition,
	prices []uint64,
	mode transportMode,
	targetOps int,
) (trialResult, error) {
	trialPrefix := fmt.Sprintf("%s-%s-%d", cfg.Prefix, mode, targetOps)
	users, err := provisionUsers(client, cfg, trialPrefix)
	if err != nil {
		return trialResult{}, err
	}
	fmt.Printf("Provisioned %d %s accounts.\n", len(users), cfg.ProvisionRole)

	var observerManager *feedObserverManager
	if cfg.ObserveL2Clients > 0 || cfg.ObserveL3Clients > 0 {
		l3Observers, err := provisionUsersWithCount(
			client,
			cfg,
			fmt.Sprintf("%s-observer", trialPrefix),
			cfg.ObserveL3Clients,
		)
		if err != nil {
			return trialResult{}, err
		}
		observerManager, err = newFeedObserverManager(cfg, market.MarketID, l3Observers)
		if err != nil {
			return trialResult{}, err
		}
	}

	pairs := buildPairs(users, prices)
	stats := newStats()
	expectedOrders := int64(cfg.DurationSeconds * targetOps)
	done := make(chan struct{})
	sem := make(chan struct{}, cfg.MaxInFlight)
	var requestWG sync.WaitGroup
	var wsManager *wsManager

	if mode == modeWS || mode == modeMixed {
		wsUsers := users
		if mode == modeMixed {
			wsUsers = make([]userProfile, 0, len(pairs))
			for _, pair := range pairs {
				wsUsers = append(wsUsers, pair.right)
			}
		}
		wsManager, err = newWSManager(cfg, stats, wsUsers, sem, &requestWG)
		if err != nil {
			close(done)
			return trialResult{}, err
		}
	}

	pairEventsPerSecond := targetOps / 2
	totalPairEvents := cfg.DurationSeconds * pairEventsPerSecond
	loadStart := time.Now().Add(500 * time.Millisecond)
	go logProgress(stats, expectedOrders, loadStart, cfg.ProgressInterval, done)

	for tick := 0; tick < totalPairEvents; tick++ {
		target := loadStart.Add(durationForPairEvent(tick, pairEventsPerSecond))
		sleepUntil(target)

		pair := &pairs[tick%len(pairs)]
		pair.priceIndex = nextPriceIndex(cfg.PricePattern, pair.priceIndex, len(prices), pair.rng)
		quantity := randomQuantity(cfg, pair.rng)
		leftBuys := pair.rng.Intn(2) == 0
		buyPrice, sellPrice := quotePrices(prices, pair.priceIndex, cfg, tick, pair.rng)

		switch mode {
		case modeREST:
			dispatchREST(client, cfg, market.MarketID, pair.left.APIKey, sideString(leftBuys), sidePrice(leftBuys, buyPrice, sellPrice), quantity, stats, sem, &requestWG)
			dispatchREST(client, cfg, market.MarketID, pair.right.APIKey, sideString(!leftBuys), sidePrice(!leftBuys, buyPrice, sellPrice), quantity, stats, sem, &requestWG)
		case modeWS:
			wsManager.Dispatch(pair.left, market.MarketID, sideString(leftBuys), sidePrice(leftBuys, buyPrice, sellPrice), quantity)
			wsManager.Dispatch(pair.right, market.MarketID, sideString(!leftBuys), sidePrice(!leftBuys, buyPrice, sellPrice), quantity)
		case modeMixed:
			dispatchREST(client, cfg, market.MarketID, pair.left.APIKey, sideString(leftBuys), sidePrice(leftBuys, buyPrice, sellPrice), quantity, stats, sem, &requestWG)
			wsManager.Dispatch(pair.right, market.MarketID, sideString(!leftBuys), sidePrice(!leftBuys, buyPrice, sellPrice), quantity)
		}
	}

	requestWG.Wait()
	close(done)

	if wsManager != nil {
		_ = wsManager.Close()
	}

	feedValidation := feedValidationResult{summary: "observers disabled"}
	if observerManager != nil {
		time.Sleep(cfg.ObserverSettleDelay)
		feedValidation, err = observerManager.Validate()
		observerManager.Close()
		if err != nil {
			return trialResult{}, err
		}
	}

	elapsed := time.Since(loadStart)
	positionSummary, err := aggregatePositions(client, cfg, users)
	if err != nil {
		positionSummary = fmt.Sprintf("position summary unavailable: %v", err)
	}

	result := buildTrialResult(
		mode,
		targetOps,
		elapsed,
		stats,
		expectedOrders,
		positionSummary,
		feedValidation,
		cfg,
	)
	return result, nil
}

func buildPairs(users []userProfile, prices []uint64) []traderPair {
	pairs := make([]traderPair, 0, len(users)/2)
	for index := 0; index < len(users); index += 2 {
		seed := time.Now().UnixNano() + int64(index+1)*7919
		pairs = append(pairs, traderPair{
			left:       users[index],
			right:      users[index+1],
			rng:        rand.New(rand.NewSource(seed)),
			priceIndex: int(seed % int64(len(prices))),
		})
	}
	return pairs
}

func buildTrialResult(
	mode transportMode,
	targetOps int,
	elapsed time.Duration,
	stats *stats,
	expectedOrders int64,
	positionSummary string,
	feedValidation feedValidationResult,
	cfg config,
) trialResult {
	latenciesNS := stats.latencySnapshot()
	latencyCount := atomic.LoadInt64(&stats.latencyCount)
	var avgMS float64
	if latencyCount > 0 {
		avgMS = float64(atomic.LoadInt64(&stats.latencyTotalNS)) / float64(latencyCount) / float64(time.Millisecond)
	}

	attempted := atomic.LoadInt64(&stats.attempted)
	completed := atomic.LoadInt64(&stats.succeeded) + atomic.LoadInt64(&stats.rejected)
	rejectRate := 0.0
	if attempted > 0 {
		rejectRate = float64(atomic.LoadInt64(&stats.rejected)) / float64(attempted)
	}
	achievedRatio := 0.0
	if expectedOrders > 0 {
		achievedRatio = float64(completed) / float64(expectedOrders)
	}

	result := trialResult{
		mode:            mode,
		targetOps:       targetOps,
		elapsed:         elapsed,
		stats:           stats,
		avgMS:           avgMS,
		p50MS:           durationPercentileMS(latenciesNS, 0.50),
		p95MS:           durationPercentileMS(latenciesNS, 0.95),
		p99MS:           durationPercentileMS(latenciesNS, 0.99),
		rejectRate:      rejectRate,
		achievedRatio:   achievedRatio,
		positionSummary: positionSummary,
		feedSummary:     feedValidation.summary,
	}

	if rejectRate > cfg.MaxRejectRate {
		result.reasons = append(result.reasons, fmt.Sprintf("reject rate %.2f%% > %.2f%%", rejectRate*100, cfg.MaxRejectRate*100))
	}
	if result.p95MS > float64(cfg.MaxP95Latency)/float64(time.Millisecond) {
		result.reasons = append(result.reasons, fmt.Sprintf("p95 %.2fms > %.2fms", result.p95MS, float64(cfg.MaxP95Latency)/float64(time.Millisecond)))
	}
	if achievedRatio < cfg.MinAchievedRatio {
		result.reasons = append(result.reasons, fmt.Sprintf("completion ratio %.1f%% < %.1f%%", achievedRatio*100, cfg.MinAchievedRatio*100))
	}
	result.reasons = append(result.reasons, feedValidation.reasons...)
	result.passed = len(result.reasons) == 0
	return result
}

func printTrial(result trialResult) {
	fmt.Printf("  Attempts: %d\n", atomic.LoadInt64(&result.stats.attempted))
	fmt.Printf("  Accepted: %d\n", atomic.LoadInt64(&result.stats.succeeded))
	fmt.Printf("  Rejected: %d\n", atomic.LoadInt64(&result.stats.rejected))
	fmt.Printf("  Resting orders: %d\n", atomic.LoadInt64(&result.stats.resting))
	fmt.Printf("  Fill events: %d\n", atomic.LoadInt64(&result.stats.fillCount))
	fmt.Printf("  Filled quantity: %d\n", atomic.LoadInt64(&result.stats.filledQuantity))
	fmt.Printf("  System errors: %d\n", atomic.LoadInt64(&result.stats.systemErrors))
	fmt.Printf("  Elapsed: %.2fs\n", result.elapsed.Seconds())
	fmt.Printf("  Issued throughput: %.1f ops/s\n", float64(atomic.LoadInt64(&result.stats.attempted))/result.elapsed.Seconds())
	fmt.Printf("  Completed throughput: %.1f ops/s\n", float64(atomic.LoadInt64(&result.stats.succeeded)+atomic.LoadInt64(&result.stats.rejected))/result.elapsed.Seconds())
	fmt.Printf("  Avg latency: %.2fms\n", result.avgMS)
	fmt.Printf("  P50 latency: %.2fms\n", result.p50MS)
	fmt.Printf("  P95 latency: %.2fms\n", result.p95MS)
	fmt.Printf("  P99 latency: %.2fms\n", result.p99MS)
	fmt.Printf("  Reject rate: %.2f%%\n", result.rejectRate*100)
	fmt.Printf("  Completion ratio: %.1f%%\n", result.achievedRatio*100)
	fmt.Printf("  Reject summary: %s\n", result.stats.rejectSummary())
	fmt.Printf("  Aggregate end positions: %s\n", result.positionSummary)
	fmt.Printf("  Feed validation: %s\n", result.feedSummary)
	if result.passed {
		fmt.Println("  Verdict: PASS")
	} else {
		fmt.Printf("  Verdict: FAIL (%s)\n", strings.Join(result.reasons, ", "))
	}
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

func resetUsers(client *http.Client, cfg config) error {
	return requestJSON(
		client,
		cfg.BaseURL,
		http.MethodPost,
		"/api/v1/admin/users/reset",
		map[string]string{"Authorization": "Bearer " + cfg.AdminToken},
		nil,
		nil,
		cfg.RequestTimeout,
	)
}

func provisionUsers(client *http.Client, cfg config, prefix string) ([]userProfile, error) {
	return provisionUsersWithCount(client, cfg, prefix, cfg.Traders)
}

func provisionUsersWithCount(client *http.Client, cfg config, prefix string, count int) ([]userProfile, error) {
	type result struct {
		Index int
		User  userProfile
		Err   error
	}

	if count == 0 {
		return nil, nil
	}

	results := make(chan result, count)
	work := make(chan int)
	var wg sync.WaitGroup

	workerCount := cfg.ProvisionConcurrency
	if workerCount > count {
		workerCount = count
	}

	for worker := 0; worker < workerCount; worker++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for index := range work {
				username := fmt.Sprintf("%s-trader-%03d", prefix, index+1)
				response, err := provisionUser(client, cfg, provisionUserRequest{
					Username: username,
					Role:     cfg.ProvisionRole,
				})
				results <- result{Index: index, User: response.Profile, Err: err}
			}
		}()
	}

	go func() {
		for index := 0; index < count; index++ {
			work <- index
		}
		close(work)
		wg.Wait()
		close(results)
	}()

	users := make([]userProfile, count)
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

func dispatchREST(
	client *http.Client,
	cfg config,
	marketID string,
	apiKey string,
	side string,
	price uint64,
	quantity uint64,
	stats *stats,
	sem chan struct{},
	requestWG *sync.WaitGroup,
) {
	sem <- struct{}{}
	requestWG.Add(1)

	go func() {
		defer requestWG.Done()
		defer func() { <-sem }()

		stats.recordAttempt()
		startedAt := time.Now()

		response, err := submitOrder(client, cfg, apiKey, submitOrderRequest{
			Market:   marketID,
			Side:     side,
			Price:    price,
			Quantity: quantity,
		})
		stats.recordLatency(time.Since(startedAt))
		if err != nil {
			stats.recordReject(err.Error())
			return
		}
		stats.recordRESTSuccess(response)
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

func durationForPairEvent(tick, pairEventsPerSecond int) time.Duration {
	return time.Duration(float64(tick) * float64(time.Second) / float64(pairEventsPerSecond))
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
				"Progress %d/%d attempts | ok=%d reject=%d issued=%.1f ops/s completed=%.1f ops/s\n",
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
