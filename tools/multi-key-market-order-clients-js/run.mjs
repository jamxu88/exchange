#!/usr/bin/env node

import { readFile } from "node:fs/promises";
import { performance } from "node:perf_hooks";
import { setTimeout as sleep } from "node:timers/promises";

const DEFAULTS = {
  baseUrl: process.env.EXCHANGE_HTTP_URL ?? "http://localhost:8080",
  keysFile: "../../allocated-api-keys-batch-1.txt",
  market: "test-market",
  quantity: 1,
  durationSeconds: 10,
  timeoutMs: 4_000,
  progressEveryMs: 1_000,
  maxInFlightPerClient: 25,
  perKeyOpsPerWindow: 500,
  windowSeconds: 10,
  indefinite: false,
};

function parseArgs(argv) {
  const options = { ...DEFAULTS };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--help") {
      options.help = true;
      continue;
    }
    if (arg === "--indefinite") {
      options.indefinite = true;
      continue;
    }
    if (!arg.startsWith("--")) {
      throw new Error(`Unexpected argument: ${arg}`);
    }
    const [rawKey, inlineValue] = arg.slice(2).split("=", 2);
    const value = inlineValue ?? argv[++index];
    if (value === undefined) {
      throw new Error(`Missing value for --${rawKey}`);
    }
    const key = rawKey.replace(/-([a-z])/g, (_, letter) => letter.toUpperCase());
    options[key] = value;
  }

  options.quantity = parsePositiveInt(options.quantity, "quantity");
  if (!options.indefinite) {
    options.durationSeconds = parsePositiveInt(options.durationSeconds, "duration-seconds");
  }
  options.timeoutMs = parsePositiveInt(options.timeoutMs, "timeout-ms");
  options.progressEveryMs = parsePositiveInt(options.progressEveryMs, "progress-every-ms");
  options.maxInFlightPerClient = parsePositiveInt(
    options.maxInFlightPerClient,
    "max-in-flight-per-client",
  );
  options.perKeyOpsPerWindow = parsePositiveInt(options.perKeyOpsPerWindow, "per-key-ops-per-window");
  options.windowSeconds = parsePositiveInt(options.windowSeconds, "window-seconds");
  options.opsPerSecondPerKey = options.perKeyOpsPerWindow / options.windowSeconds;
  if (!Number.isFinite(options.opsPerSecondPerKey) || options.opsPerSecondPerKey <= 0) {
    throw new Error("Invalid per-key rate configuration");
  }
  return options;
}

function parsePositiveInt(value, name) {
  const parsed = Number.parseInt(String(value), 10);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    throw new Error(`--${name} must be a positive integer`);
  }
  return parsed;
}

function printUsage() {
  console.log(`Usage:
  node tools/multi-key-market-order-clients-js/run.mjs [options]

Options:
  --base-url URL                  Exchange HTTP base URL
  --keys-file PATH                TSV file with "identifier<TAB>api_key"
  --market SYMBOL                 Target market (default BTC-USD)
  --quantity SIZE                 Market order quantity
  --duration-seconds SECONDS      How long to run
  --indefinite                    Run until stopped (Ctrl+C)
  --timeout-ms MS                 Per-request timeout
  --max-in-flight-per-client N    Backpressure cap per key
  --per-key-ops-per-window N      Rate limit ops in window (default 500)
  --window-seconds N              Rate limit window seconds (default 10)
  --help                          Show this message
`);
}

async function loadApiKeys(path) {
  const content = await readFile(new URL(path, import.meta.url), "utf8");
  const lines = content.split(/\r?\n/).filter(Boolean);
  if (lines.length < 2) {
    throw new Error(`No API keys found in ${path}`);
  }

  const parsed = [];
  for (const line of lines.slice(1)) {
    const [identifier, apiKey] = line.split("\t");
    if (!identifier || !apiKey) {
      continue;
    }
    parsed.push({ identifier: identifier.trim(), apiKey: apiKey.trim() });
  }
  if (parsed.length === 0) {
    throw new Error(`No valid key rows found in ${path}`);
  }
  return parsed;
}

function createStats() {
  return {
    attempted: 0,
    succeeded: 0,
    rejected: 0,
    timedOut: 0,
    inFlight: 0,
    errors: new Map(),
  };
}

function recordError(stats, key) {
  stats.errors.set(key, (stats.errors.get(key) ?? 0) + 1);
}

async function requestJson(baseUrl, path, init = {}, timeoutMs = 4_000) {
  const url = new URL(path, baseUrl.endsWith("/") ? baseUrl : `${baseUrl}/`).toString();
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetch(url, {
      ...init,
      signal: controller.signal,
      headers: {
        accept: "application/json",
        "content-type": "application/json",
        ...(init.headers ?? {}),
      },
    });
    const text = await response.text();
    const payload = text ? safeJsonParse(text) : null;
    if (!response.ok) {
      const message =
        payload && typeof payload === "object" && "error" in payload && payload.error
          ? String(payload.error)
          : text || `HTTP ${response.status}`;
      return { ok: false, status: response.status, message };
    }
    return { ok: true, payload };
  } catch (error) {
    if (error?.name === "AbortError") {
      return { ok: false, status: 0, message: "request timeout", timedOut: true };
    }
    return { ok: false, status: 0, message: error instanceof Error ? error.message : String(error) };
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

function randomSide() {
  return Math.random() < 0.5 ? "BUY" : "SELL";
}

async function runClient(config, profile, globalStats, startedAtMs, stopAtMs) {
  const intervalMs = 1_000 / config.opsPerSecondPerKey;
  const inFlight = new Set();
  let tick = 0;

  while (performance.now() < stopAtMs) {
    const targetMs = startedAtMs + tick * intervalMs;
    tick += 1;
    const delayMs = targetMs - performance.now();
    if (delayMs > 0) {
      await sleep(delayMs);
    }

    if (inFlight.size >= config.maxInFlightPerClient) {
      await Promise.race(inFlight);
      continue;
    }

    const task = (async () => {
      globalStats.attempted += 1;
      globalStats.inFlight += 1;
      const result = await requestJson(
        config.baseUrl,
        "/api/v1/orders",
        {
          method: "POST",
          headers: { "x-api-key": profile.apiKey },
          body: JSON.stringify({
            market: config.market,
            side: randomSide(),
            order_type: "market",
            price: 0,
            quantity: config.quantity,
          }),
        },
        config.timeoutMs,
      );
      if (result.ok) {
        globalStats.succeeded += 1;
      } else {
        globalStats.rejected += 1;
        if (result.timedOut) {
          globalStats.timedOut += 1;
        }
        recordError(globalStats, `${result.status} ${result.message}`);
      }
      globalStats.inFlight -= 1;
    })();

    inFlight.add(task);
    task.finally(() => inFlight.delete(task));
  }

  if (inFlight.size > 0) {
    await Promise.allSettled([...inFlight]);
  }
}

function summarizeTopErrors(stats, top = 5) {
  if (stats.errors.size === 0) {
    return "none";
  }
  return [...stats.errors.entries()]
    .sort((left, right) => right[1] - left[1])
    .slice(0, top)
    .map(([message, count]) => `${count}x ${message}`)
    .join("; ");
}

async function main() {
  const config = parseArgs(process.argv.slice(2));
  if (config.help) {
    printUsage();
    return;
  }
  if (typeof fetch !== "function") {
    throw new Error("Node.js with global fetch support is required.");
  }

  const profiles = await loadApiKeys(config.keysFile);
  const stats = createStats();
  const startedAtMs = performance.now();
  const stopAtMs = config.indefinite ? Number.POSITIVE_INFINITY : startedAtMs + config.durationSeconds * 1_000;

  console.log(
    [
      `Starting ${profiles.length} clients`,
      `market=${config.market}`,
      `perKeyRate=${config.perKeyOpsPerWindow}/${config.windowSeconds}s`,
      `aggregateTargetRps=${(profiles.length * config.opsPerSecondPerKey).toFixed(1)}`,
      `duration=${config.indefinite ? "indefinite" : `${config.durationSeconds}s`}`,
    ].join(" | "),
  );

  const progressTimer = setInterval(() => {
    const elapsed = Math.max(0.001, (performance.now() - startedAtMs) / 1_000);
    const throughput = (stats.attempted / elapsed).toFixed(1);
    const avgPerClientPer10s = ((stats.attempted / Math.max(1, profiles.length) / elapsed) * 10).toFixed(1);
    console.log(
      `Progress attempted=${stats.attempted} ok=${stats.succeeded} rejected=${stats.rejected} inFlight=${stats.inFlight} throughput=${throughput}/s avgPerClient=${avgPerClientPer10s}/10s`,
    );
  }, config.progressEveryMs);

  try {
    await Promise.all(
      profiles.map((profile) => runClient(config, profile, stats, startedAtMs, stopAtMs)),
    );
  } finally {
    clearInterval(progressTimer);
  }

  const elapsed = Math.max(0.001, (performance.now() - startedAtMs) / 1_000);
  console.log("");
  console.log("Run complete");
  console.log(`  Clients: ${profiles.length}`);
  console.log(`  Attempted: ${stats.attempted}`);
  console.log(`  Accepted: ${stats.succeeded}`);
  console.log(`  Rejected: ${stats.rejected}`);
  console.log(`  Timed out: ${stats.timedOut}`);
  console.log(`  Throughput: ${(stats.attempted / elapsed).toFixed(1)} req/s`);
  console.log(`  Top rejects: ${summarizeTopErrors(stats)}`);
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
});
