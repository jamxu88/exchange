#!/usr/bin/env node

import { performance } from "node:perf_hooks";
import { setTimeout as sleep } from "node:timers/promises";

const DEFAULTS = {
  baseUrl: process.env.EXCHANGE_HTTP_URL ?? "http://localhost:8080",
  market: "BTC-USD",
  traders: 25,
  makerCount: 0,
  iterations: 100,
  durationSeconds: null,
  opsPerSecond: null,
  quantity: 1,
  centerPrice: 100,
  spread: 2,
  lowerPrice: null,
  upperPrice: null,
  requestTimeoutMs: 5_000,
  jitterMs: 25,
  progressIntervalMs: 1_000,
  provisionConcurrency: 10,
  prefix: `stress-${Date.now()}`,
  quoteSize: null,
  cleanupQuotes: true,
  resetBeforeStart: false,
  resetAfterRun: false,
};

class HttpError extends Error {
  constructor(message, status, payload = null) {
    super(message);
    this.name = "HttpError";
    this.status = status;
    this.payload = payload;
  }
}

function printUsage() {
  console.log(`Usage:
  node tools/trader-stress-bot/index.mjs --admin-token TOKEN [options]

Options:
  --base-url URL                 Exchange HTTP base URL
  --admin-token TOKEN            Admin bearer token used for provisioning
  --market SYMBOL                Market symbol to trade
  --traders COUNT                Number of simulated traders to provision
  --maker-count COUNT            Number of provisioned traders reserved for quote making
  --iterations COUNT             Orders per simulated trader
  --duration-seconds SECONDS     Wall-clock duration for rate-based trading
  --ops-per-second COUNT         Per-user order submission rate in duration mode
  --quantity SIZE                Quantity per trader order
  --center-price PRICE           Mid price used to seed the quote account
  --spread PRICE                 Total bid/ask spread in price units
  --lower-price PRICE            Lower bound for random trading prices
  --upper-price PRICE            Upper bound for random trading prices
  --quote-size SIZE              Resting size for each admin quote
  --jitter-ms MS                 Max random delay between trader orders
  --request-timeout-ms MS        Per-request timeout
  --prefix TEXT                  Username prefix for provisioned accounts
  --provision-concurrency COUNT  Parallelism for user provisioning
  --reset-before-start           Call POST /api/v1/admin/users/reset before the run
  --reset-after-run              Call POST /api/v1/admin/users/reset after the run
  --no-cleanup-quotes            Leave seeded admin quote orders open
  --help                         Show this message
`);
}

function parseArgs(argv) {
  const options = { ...DEFAULTS };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--help") {
      options.help = true;
      continue;
    }
    if (arg === "--reset-before-start") {
      options.resetBeforeStart = true;
      continue;
    }
    if (arg === "--reset-after-run") {
      options.resetAfterRun = true;
      continue;
    }
    if (arg === "--no-cleanup-quotes") {
      options.cleanupQuotes = false;
      continue;
    }
    if (!arg.startsWith("--")) {
      throw new Error(`Unexpected argument: ${arg}`);
    }

    const [rawKey, inlineValue] = arg.slice(2).split("=", 2);
    const key = camelCase(rawKey);
    const value = inlineValue ?? argv[++index];
    if (value === undefined) {
      throw new Error(`Missing value for --${rawKey}`);
    }
    options[key] = value;
  }

  options.traders = parsePositiveInt(options.traders, "traders");
  options.makerCount = parseNonNegativeInt(options.makerCount, "maker-count");
  options.iterations = parsePositiveInt(options.iterations, "iterations");
  if (options.durationSeconds !== null) {
    options.durationSeconds = parsePositiveInt(options.durationSeconds, "duration-seconds");
  }
  if (options.opsPerSecond !== null) {
    options.opsPerSecond = parsePositiveInt(options.opsPerSecond, "ops-per-second");
  }
  options.quantity = parsePositiveInt(options.quantity, "quantity");
  options.centerPrice = parsePositiveInt(options.centerPrice, "center-price");
  options.spread = parsePositiveInt(options.spread, "spread");
  if (options.lowerPrice !== null || options.upperPrice !== null) {
    if (options.lowerPrice === null || options.upperPrice === null) {
      throw new Error("--lower-price and --upper-price must be supplied together");
    }
    options.lowerPrice = parsePositiveInt(options.lowerPrice, "lower-price");
    options.upperPrice = parsePositiveInt(options.upperPrice, "upper-price");
    if (options.lowerPrice >= options.upperPrice) {
      throw new Error("--lower-price must be below --upper-price");
    }
  }
  options.requestTimeoutMs = parsePositiveInt(
    options.requestTimeoutMs,
    "request-timeout-ms",
  );
  options.jitterMs = parseNonNegativeInt(options.jitterMs, "jitter-ms");
  options.progressIntervalMs = parsePositiveInt(
    options.progressIntervalMs,
    "progress-interval-ms",
  );
  options.provisionConcurrency = parsePositiveInt(
    options.provisionConcurrency,
    "provision-concurrency",
  );
  if (options.quoteSize !== null) {
    options.quoteSize = parsePositiveInt(options.quoteSize, "quote-size");
  }
  if (!options.adminToken && !options.help) {
    throw new Error("--admin-token is required");
  }
  if (options.makerCount >= options.traders) {
    throw new Error("--maker-count must be lower than --traders");
  }
  if ((options.durationSeconds === null) !== (options.opsPerSecond === null)) {
    throw new Error("--duration-seconds and --ops-per-second must be supplied together");
  }
  if (options.durationSeconds !== null) {
    if (options.makerCount !== 0) {
      throw new Error("duration mode does not support --maker-count");
    }
    if (options.traders % 2 !== 0) {
      throw new Error("duration mode requires an even --traders count");
    }
    if (options.lowerPrice === null || options.upperPrice === null) {
      throw new Error("duration mode requires --lower-price and --upper-price");
    }
  }

  return options;
}

function camelCase(flag) {
  return flag.replace(/-([a-z])/g, (_, letter) => letter.toUpperCase());
}

function parsePositiveInt(value, name) {
  const parsed = Number.parseInt(String(value), 10);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    throw new Error(`--${name} must be a positive integer`);
  }
  return parsed;
}

function parseNonNegativeInt(value, name) {
  const parsed = Number.parseInt(String(value), 10);
  if (!Number.isFinite(parsed) || parsed < 0) {
    throw new Error(`--${name} must be a non-negative integer`);
  }
  return parsed;
}

function percentile(values, fraction) {
  if (values.length === 0) {
    return 0;
  }
  const sorted = [...values].sort((left, right) => left - right);
  const position = Math.min(
    sorted.length - 1,
    Math.max(0, Math.ceil(sorted.length * fraction) - 1),
  );
  return sorted[position];
}

function average(values) {
  if (values.length === 0) {
    return 0;
  }
  return values.reduce((sum, value) => sum + value, 0) / values.length;
}

function alignDown(value, tickSize) {
  return Math.floor(value / tickSize) * tickSize;
}

function alignUp(value, tickSize) {
  return Math.ceil(value / tickSize) * tickSize;
}

function buildQuotePrices(centerPrice, spread, tickSize) {
  const minimumSpread = Math.max(spread, tickSize * 2);
  const halfSpread = minimumSpread / 2;
  const bid = alignDown(centerPrice - halfSpread, tickSize);
  let ask = alignUp(centerPrice + halfSpread, tickSize);
  if (ask <= bid) {
    ask = bid + tickSize;
  }
  return { bid, ask };
}

function buildRandomBand(lowerPrice, upperPrice, tickSize) {
  const lower = alignUp(lowerPrice, tickSize);
  const upper = alignDown(upperPrice, tickSize);
  if (lower >= upper) {
    throw new Error("Price band collapses after tick-size alignment");
  }

  const stepCount = Math.floor((upper - lower) / tickSize);
  const highestBid = lower + Math.floor(stepCount / 2) * tickSize;
  const lowestAsk = highestBid + tickSize;
  const bids = [];
  const asks = [];
  const prices = [];

  for (let price = lower; price <= upper; price += tickSize) {
    prices.push(price);
  }

  for (let price = lower; price <= highestBid; price += tickSize) {
    bids.push(price);
  }
  for (let price = lowestAsk; price <= upper; price += tickSize) {
    asks.push(price);
  }
  if (bids.length === 0 || asks.length === 0) {
    throw new Error("Price band must leave room for both bids and asks");
  }

  return { lower, upper, bids, asks, prices };
}

function chooseRandom(items) {
  return items[Math.floor(Math.random() * items.length)];
}

function createStats() {
  return {
    attempted: 0,
    succeeded: 0,
    rejected: 0,
    resting: 0,
    fillCount: 0,
    filledQuantity: 0,
    latencyCount: 0,
    latencyTotalMs: 0,
    latencySamplesMs: [],
    rejects: new Map(),
  };
}

function recordLatency(stats, latencyMs) {
  const sampleLimit = 50_000;
  stats.latencyCount += 1;
  stats.latencyTotalMs += latencyMs;
  if (stats.latencySamplesMs.length < sampleLimit) {
    stats.latencySamplesMs.push(latencyMs);
    return;
  }

  const slot = Math.floor(Math.random() * stats.latencyCount);
  if (slot < sampleLimit) {
    stats.latencySamplesMs[slot] = latencyMs;
  }
}

function recordOrderResult(stats, response) {
  stats.succeeded += 1;
  stats.resting += response.resting ? 1 : 0;
  stats.fillCount += response.fills.length;
  stats.filledQuantity += response.fills.reduce((sum, fill) => sum + fill.quantity, 0);
}

function recordOrderError(stats, error) {
  stats.rejected += 1;
  const key =
    error instanceof HttpError
      ? `${error.status} ${error.message}`
      : error instanceof Error
        ? error.message
        : String(error);
  stats.rejects.set(key, (stats.rejects.get(key) ?? 0) + 1);
}

function launchTrackedOrder(config, apiKey, payload, stats, inFlight) {
  const latencyStartedAt = performance.now();
  stats.attempted += 1;
  const task = (async () => {
    try {
      const response = await submitOrder(config, apiKey, payload);
      recordOrderResult(stats, response);
    } catch (error) {
      recordOrderError(stats, error);
    } finally {
      recordLatency(stats, performance.now() - latencyStartedAt);
    }
  })();
  inFlight.add(task);
  task.finally(() => inFlight.delete(task));
  return task;
}

async function waitUntil(targetMs) {
  const delayMs = targetMs - performance.now();
  if (delayMs > 0) {
    await sleep(delayMs);
  }
}

async function drainInFlight(inFlight) {
  while (inFlight.size > 0) {
    await Promise.allSettled([...inFlight]);
  }
}

async function requestJson(baseUrl, path, init = {}, timeoutMs = 5_000) {
  const url = new URL(path, baseUrl.endsWith("/") ? baseUrl : `${baseUrl}/`).toString();
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), timeoutMs);

  try {
    const response = await fetch(url, {
      ...init,
      signal: controller.signal,
      headers: {
        accept: "application/json",
        ...(init.body ? { "content-type": "application/json" } : {}),
        ...(init.headers ?? {}),
      },
    });
    const text = await response.text();
    const payload = text ? safeJsonParse(text) : null;
    if (!response.ok) {
      const message =
        payload && typeof payload === "object" && "error" in payload && payload.error
          ? payload.error
          : text || `Request failed with status ${response.status}`;
      throw new HttpError(message, response.status, payload);
    }
    return payload;
  } catch (error) {
    if (error instanceof HttpError) {
      throw error;
    }
    if (error?.name === "AbortError") {
      throw new Error(`Request to ${path} timed out after ${timeoutMs}ms`);
    }
    throw error;
  } finally {
    clearTimeout(timeout);
  }
}

function safeJsonParse(text) {
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

function adminHeaders(adminToken) {
  return { authorization: `Bearer ${adminToken}` };
}

function traderHeaders(apiKey) {
  return { "x-api-key": apiKey };
}

async function getAdminState(config) {
  return requestJson(
    config.baseUrl,
    "/api/v1/admin/state",
    {
      method: "GET",
      headers: adminHeaders(config.adminToken),
    },
    config.requestTimeoutMs,
  );
}

async function resetAllUsers(config) {
  return requestJson(
    config.baseUrl,
    "/api/v1/admin/users/reset",
    {
      method: "POST",
      headers: adminHeaders(config.adminToken),
    },
    config.requestTimeoutMs,
  );
}

async function getMarkets(config) {
  return requestJson(
    config.baseUrl,
    "/api/v1/markets",
    {
      method: "GET",
    },
    config.requestTimeoutMs,
  );
}

async function provisionUser(config, payload) {
  return requestJson(
    config.baseUrl,
    "/api/v1/admin/users",
    {
      method: "POST",
      headers: adminHeaders(config.adminToken),
      body: JSON.stringify(payload),
    },
    config.requestTimeoutMs,
  );
}

async function submitOrder(config, apiKey, payload) {
  return requestJson(
    config.baseUrl,
    "/api/v1/orders",
    {
      method: "POST",
      headers: traderHeaders(apiKey),
      body: JSON.stringify(payload),
    },
    config.requestTimeoutMs,
  );
}

async function getOpenOrders(config, apiKey) {
  return requestJson(
    config.baseUrl,
    "/api/v1/open-orders",
    {
      method: "GET",
      headers: traderHeaders(apiKey),
    },
    config.requestTimeoutMs,
  );
}

async function cancelOrder(config, apiKey, orderId) {
  return requestJson(
    config.baseUrl,
    `/api/v1/orders/${orderId}`,
    {
      method: "DELETE",
      headers: traderHeaders(apiKey),
    },
    config.requestTimeoutMs,
  );
}

async function getPortfolio(config, apiKey) {
  return requestJson(
    config.baseUrl,
    "/api/v1/portfolio",
    {
      method: "GET",
      headers: traderHeaders(apiKey),
    },
    config.requestTimeoutMs,
  );
}

async function summarizePortfolios(config, users) {
  const portfolios = await Promise.all(
    users.map((user) =>
      getPortfolio(config, user.api_key).catch(() => ({
        trader_id: user.trader_id,
        position_limit: null,
        positions: [],
      })),
    ),
  );

  const aggregate = new Map();
  for (const portfolio of portfolios) {
    for (const position of portfolio.positions) {
      aggregate.set(
        position.market,
        (aggregate.get(position.market) ?? 0) + position.net_quantity,
      );
    }
  }

  return aggregate.size === 0
    ? "flat"
    : [...aggregate.entries()]
        .map(([market, netQuantity]) => `${market}:${netQuantity}`)
        .join(", ");
}

async function mapLimit(items, limit, mapper) {
  const results = new Array(items.length);
  let cursor = 0;

  async function worker() {
    while (true) {
      const current = cursor;
      cursor += 1;
      if (current >= items.length) {
        return;
      }
      results[current] = await mapper(items[current], current);
    }
  }

  const workerCount = Math.max(1, Math.min(limit, items.length));
  await Promise.all(Array.from({ length: workerCount }, () => worker()));
  return results;
}

function makeTraderUsername(prefix, index) {
  return `${prefix}-trader-${String(index + 1).padStart(3, "0")}`;
}

function summarizeRejects(rejects) {
  if (rejects.size === 0) {
    return "none";
  }
  return [...rejects.entries()]
    .sort((left, right) => right[1] - left[1])
    .map(([message, count]) => `${count}x ${message}`)
    .join("; ");
}

async function runStressTest(config) {
  if (typeof fetch !== "function") {
    throw new Error("This script requires Node.js with global fetch support.");
  }

  if (config.resetBeforeStart) {
    console.log("Resetting trading state before the run.");
    await resetAllUsers(config);
  }

  const adminState = await getAdminState(config);
  if (!adminState?.controls?.trading_enabled) {
    throw new Error("Trading is disabled. Enable it before starting the stress run.");
  }

  const markets = await getMarkets(config);
  const market = markets.find((entry) => entry.market_id === config.market);
  if (!market) {
    throw new Error(`Market ${config.market} not found`);
  }

  const tickSize = market.tick_size;
  const minOrderQuantity = market.min_order_quantity;
  if (config.quantity < minOrderQuantity) {
    throw new Error(
      `Configured quantity ${config.quantity} is below market minimum ${minOrderQuantity}`,
    );
  }

  const randomBand =
    config.lowerPrice !== null
      ? buildRandomBand(config.lowerPrice, config.upperPrice, tickSize)
      : null;
  const quotes = randomBand
    ? null
    : buildQuotePrices(config.centerPrice, config.spread, tickSize);
  if (quotes && (quotes.bid <= 0 || quotes.ask <= 0)) {
    throw new Error(
      `Invalid quote prices generated from center=${config.centerPrice} spread=${config.spread}`,
    );
  }
  const quoteSize =
    config.quoteSize ??
    Math.max(config.quantity * config.traders * config.iterations + config.quantity * 10, 1_000);
  const totalProvisionCount = config.traders;

  console.log(
    config.durationSeconds !== null
      ? `Provisioning ${totalProvisionCount} traders on ${market.market_id}. Duration=${config.durationSeconds}s Rate=${config.opsPerSecond}/s Band=${randomBand.lower}-${randomBand.upper}`
      : randomBand
      ? `Provisioning ${totalProvisionCount} traders on ${market.market_id}. Random band=${randomBand.lower}-${randomBand.upper} QuoteLevels=${randomBand.bids.length + randomBand.asks.length} QuoteSize=${quoteSize}`
      : `Provisioning ${config.traders} traders on ${market.market_id}. Bid=${quotes.bid} Ask=${quotes.ask} QuoteSize=${quoteSize}`,
  );

  const quoteUser =
    config.durationSeconds === null && config.makerCount === 0
      ? (
          await provisionUser(config, {
            username: `${config.prefix}-admin-maker`,
            role: "admin",
          })
        ).profile
      : null;

  const traderUsers = (
    await mapLimit(
      Array.from({ length: totalProvisionCount }, (_, index) => index),
      config.provisionConcurrency,
      async (index) => {
        const provisioned = await provisionUser(config, {
          username: makeTraderUsername(config.prefix, index),
          role: "trader",
        });
        return provisioned.profile;
      },
    )
  ).filter(Boolean);
  const makerUsers = traderUsers.slice(0, config.makerCount);
  const takerUsers =
    config.makerCount > 0 ? traderUsers.slice(config.makerCount) : traderUsers;

  console.log(
    config.durationSeconds !== null
      ? `Provisioned ${traderUsers.length} trader accounts for duration mode.`
      : config.makerCount > 0
      ? `Provisioned ${traderUsers.length} trader accounts. Makers=${makerUsers.length} Takers=${takerUsers.length}.`
      : `Provisioned quote user ${quoteUser.username} and ${traderUsers.length} simulated traders.`,
  );

  const stats = createStats();
  const startedAt = performance.now();
  const totalAttempts =
    config.durationSeconds !== null
      ? takerUsers.length * config.opsPerSecond * config.durationSeconds
      : takerUsers.length * config.iterations;
  const progressTimer = setInterval(() => {
    const elapsedSeconds = Math.max(0.001, (performance.now() - startedAt) / 1_000);
    const issuedRate = (stats.attempted / elapsedSeconds).toFixed(1);
    const completed = stats.succeeded + stats.rejected;
    const completedRate = (completed / elapsedSeconds).toFixed(1);
    console.log(
      `Progress ${stats.attempted}/${totalAttempts} attempts | ok=${stats.succeeded} reject=${stats.rejected} issued=${issuedRate} req/s completed=${completedRate} req/s`,
    );
  }, config.progressIntervalMs);

  try {
    if (config.durationSeconds !== null) {
      if (!randomBand) {
        throw new Error("duration mode requires a random trading band");
      }

      console.log(
        `Running duration mode for ${config.durationSeconds}s at ${config.opsPerSecond} ops/s per user.`,
      );
      const inFlight = new Set();
      const totalTicks = config.durationSeconds * config.opsPerSecond;
      const intervalMs = 1_000 / config.opsPerSecond;
      const traderPairs = [];
      for (let index = 0; index < takerUsers.length; index += 2) {
        traderPairs.push([takerUsers[index], takerUsers[index + 1]]);
      }

      for (let tick = 0; tick < totalTicks; tick += 1) {
        await waitUntil(startedAt + tick * intervalMs);
        for (const [leftTrader, rightTrader] of traderPairs) {
          const price = chooseRandom(randomBand.prices);
          const leftSide = Math.random() < 0.5 ? "BUY" : "SELL";
          const rightSide = leftSide === "BUY" ? "SELL" : "BUY";
          launchTrackedOrder(
            config,
            leftTrader.api_key,
            {
              market: market.market_id,
              side: leftSide,
              price,
              quantity: config.quantity,
            },
            stats,
            inFlight,
          );
          launchTrackedOrder(
            config,
            rightTrader.api_key,
            {
              market: market.market_id,
              side: rightSide,
              price,
              quantity: config.quantity,
            },
            stats,
            inFlight,
          );
        }
      }

      console.log(`Issued all ${stats.attempted} duration-mode orders. Waiting for completions.`);
      await drainInFlight(inFlight);
    } else {
      const quoteOrders =
        config.makerCount > 0
          ? (
              await Promise.all(
                makerUsers.map(async (maker) => {
                  const bidLevels = randomBand ? randomBand.bids : [quotes.bid];
                  const askLevels = randomBand ? randomBand.asks : [quotes.ask];
                  const perBidLevel = Math.max(1, Math.floor(1_000 / bidLevels.length));
                  const perAskLevel = Math.max(1, Math.floor(1_000 / askLevels.length));
                  return Promise.all([
                    ...bidLevels.map((price) =>
                      submitOrder(config, maker.api_key, {
                        market: market.market_id,
                        side: "BUY",
                        price,
                        quantity: perBidLevel,
                      }),
                    ),
                    ...askLevels.map((price) =>
                      submitOrder(config, maker.api_key, {
                        market: market.market_id,
                        side: "SELL",
                        price,
                        quantity: perAskLevel,
                      }),
                    ),
                  ]);
                }),
              )
            ).flat()
          : await Promise.all([
              submitOrder(config, quoteUser.api_key, {
                market: market.market_id,
                side: "BUY",
                price: quotes.bid,
                quantity: quoteSize,
              }),
              submitOrder(config, quoteUser.api_key, {
                market: market.market_id,
                side: "SELL",
                price: quotes.ask,
                quantity: quoteSize,
              }),
            ]);

      console.log(`Seeded admin quote ladder. Orders=${quoteOrders.length}`);

      await Promise.all(
        takerUsers.map(async (trader, traderIndex) => {
          for (let iteration = 0; iteration < config.iterations; iteration += 1) {
            const side = randomBand
              ? Math.random() < 0.5
                ? "BUY"
                : "SELL"
              : (traderIndex + iteration) % 2 === 0
                ? "BUY"
                : "SELL";
            const price = randomBand
              ? side === "BUY"
                ? chooseRandom(randomBand.asks)
                : chooseRandom(randomBand.bids)
              : side === "BUY"
                ? quotes.ask
                : quotes.bid;
            const latencyStartedAt = performance.now();
            stats.attempted += 1;

            try {
              const response = await submitOrder(config, trader.api_key, {
                market: market.market_id,
                side,
                price,
                quantity: config.quantity,
              });
              recordOrderResult(stats, response);
            } catch (error) {
              recordOrderError(stats, error);
            } finally {
              recordLatency(stats, performance.now() - latencyStartedAt);
            }

            if (config.jitterMs > 0) {
              await sleep(Math.floor(Math.random() * (config.jitterMs + 1)));
            }
          }
        }),
      );
    }
  } finally {
    clearInterval(progressTimer);
  }

  const elapsedMs = performance.now() - startedAt;
  const elapsedSeconds = elapsedMs / 1_000;
  const quoteSummary =
    config.durationSeconds !== null
      ? await summarizePortfolios(config, traderUsers)
      : config.makerCount > 0
        ? await summarizePortfolios(config, makerUsers)
        : await getPortfolio(config, quoteUser.api_key);

  if (config.cleanupQuotes && config.durationSeconds === null) {
    const quoteCleanupUsers = config.makerCount > 0 ? makerUsers : [quoteUser];
    await Promise.all(
      quoteCleanupUsers.map(async (user) => {
        const openOrders = await getOpenOrders(config, user.api_key).catch(() => []);
        await Promise.all(
          openOrders.map((order) =>
            cancelOrder(config, user.api_key, order.id).catch((error) => {
              const message = error instanceof Error ? error.message : String(error);
              console.warn(`Quote cleanup failed for ${order.id}: ${message}`);
            }),
          ),
        );
      }),
    );
  }

  if (config.resetAfterRun) {
    console.log("Resetting trading state after the run.");
    await resetAllUsers(config);
  }

  console.log("");
  console.log("Run complete");
  console.log(`  Market: ${market.market_id}`);
  console.log(`  Provisioned users: ${traderUsers.length}`);
  console.log(`  Makers: ${makerUsers.length}`);
  console.log(`  Takers: ${takerUsers.length}`);
  console.log(`  Orders attempted: ${stats.attempted}`);
  console.log(`  Orders accepted: ${stats.succeeded}`);
  console.log(`  Orders rejected: ${stats.rejected}`);
  console.log(`  Resting trader orders: ${stats.resting}`);
  console.log(`  Taker fill events: ${stats.fillCount}`);
  console.log(`  Taker filled quantity: ${stats.filledQuantity}`);
  console.log(`  Elapsed: ${elapsedSeconds.toFixed(2)}s`);
  console.log(`  Throughput: ${(stats.attempted / elapsedSeconds).toFixed(1)} req/s`);
  console.log(`  Avg latency: ${(stats.latencyTotalMs / Math.max(1, stats.latencyCount)).toFixed(2)}ms`);
  console.log(`  P50 latency: ${percentile(stats.latencySamplesMs, 0.5).toFixed(2)}ms`);
  console.log(`  P95 latency: ${percentile(stats.latencySamplesMs, 0.95).toFixed(2)}ms`);
  console.log(`  P99 latency: ${percentile(stats.latencySamplesMs, 0.99).toFixed(2)}ms`);
  console.log(`  Reject summary: ${summarizeRejects(stats.rejects)}`);
  if (config.durationSeconds !== null) {
    console.log(`  Aggregate end positions: ${quoteSummary}`);
  } else if (config.makerCount > 0) {
    console.log(`  Maker aggregate positions: ${quoteSummary}`);
  } else {
    console.log(
      `  Quote account role=${quoteUser.role} fixed_limit=${quoteSummary.position_limit ?? "unlimited"}`,
    );
    console.log(
      `  Quote account positions: ${
        quoteSummary.positions.length === 0
          ? "flat"
          : quoteSummary.positions
              .map((position) => `${position.market}:${position.net_quantity}`)
              .join(", ")
      }`,
    );
  }
}

async function main() {
  try {
    const config = parseArgs(process.argv.slice(2));
    if (config.help) {
      printUsage();
      return;
    }
    await runStressTest(config);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    console.error(message);
    process.exitCode = 1;
  }
}

await main();
