#!/usr/bin/env node
/*
  WS connection stress test for the live exchange.

  Opens N concurrent WebSocket clients against wss://<host>/ws, each one:
    1. authenticates with a distinct API key
    2. subscribes to `data` for the configured markets
    3. passively consumes the feed and records metrics

  Reports:
    - connect/auth/first-snapshot latencies
    - messages/sec per connection and aggregate
    - sequence-gap count per (connection, market)
    - disconnects / errors
    - a sample of the last few events seen on one connection, so we can
      eyeball that real bot-driven trading activity is flowing in
*/

import WebSocket from "../../client/node_modules/ws/wrapper.mjs";
import fs from "node:fs";
import path from "node:path";

function parseArgs(argv) {
  const args = { connections: 100, durationSeconds: 30 };
  for (let i = 2; i < argv.length; i += 1) {
    const key = argv[i];
    const value = argv[i + 1];
    if (key === "--connections") {
      args.connections = Number(value);
      i += 1;
    } else if (key === "--duration-seconds") {
      args.durationSeconds = Number(value);
      i += 1;
    } else if (key === "--base-url") {
      args.baseUrl = value;
      i += 1;
    } else if (key === "--keys-file") {
      args.keysFile = value;
      i += 1;
    } else if (key === "--keys-files") {
      args.keysFiles = value.split(",").map((v) => v.trim()).filter(Boolean);
      i += 1;
    } else if (key === "--keys-to-use") {
      args.keysToUse = Number(value);
      i += 1;
    } else if (key === "--markets") {
      args.markets = value;
      i += 1;
    }
  }
  return args;
}

const cli = parseArgs(process.argv);
const baseUrl = cli.baseUrl ?? "wss://exchange.jamesxu.dev/ws";
const keysFiles =
  cli.keysFiles ??
  (cli.keysFile ? [cli.keysFile] : [
    path.resolve("../../allocated-api-keys-batch-1.txt"),
    path.resolve("../../allocated-api-keys-batch-2.txt"),
  ]);
const markets = (cli.markets ?? "PLAYGROUND-MARKET")
  .split(",")
  .map((entry) => entry.trim())
  .filter(Boolean);

const allKeys = keysFiles.flatMap((file) =>
  fs
    .readFileSync(file, "utf8")
    .split("\n")
    .slice(1)
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const [identifier, apiKey] = line.split(",");
      return { identifier, apiKey };
    }),
);

const keysToUse =
  cli.keysToUse ?? Math.max(1, Math.ceil(cli.connections / 2));
if (allKeys.length < keysToUse) {
  console.error(
    `need ${keysToUse} distinct keys but only found ${allKeys.length} across ${keysFiles.join(", ")}`,
  );
  process.exit(1);
}
const keyPool = allKeys.slice(0, keysToUse);

function keyForConnection(index) {
  return keyPool[index % keyPool.length];
}

const now = () => Number(process.hrtime.bigint() / 1_000_000n);

const clients = [];

class ClientState {
  constructor(index, keyEntry) {
    this.index = index;
    this.identifier = keyEntry.identifier;
    this.apiKey = keyEntry.apiKey;
    this.openedAt = null;
    this.authenticatedAt = null;
    this.firstSnapshotAt = null;
    this.messageCount = 0;
    this.typeCounts = Object.create(null);
    this.lastSequence = Object.create(null);
    this.sequenceGaps = 0;
    this.errorCount = 0;
    this.closedAt = null;
    this.closeCode = null;
    this.resyncCount = 0;
    this.ringBuffer = [];
    this.connectStartedAt = now();
    this.socket = null;
  }

  noteMessage(raw) {
    this.messageCount += 1;
    let message;
    try {
      message = JSON.parse(raw);
    } catch {
      this.errorCount += 1;
      return;
    }
    const type = message.type ?? "unknown";
    this.typeCounts[type] = (this.typeCounts[type] ?? 0) + 1;

    if (type === "snapshot" && this.firstSnapshotAt == null) {
      this.firstSnapshotAt = now();
    }
    if (type === "snapshot" || type === "delta") {
      const market = message.market;
      const endSeq = message.sequence;
      const startSeq =
        typeof message.start_sequence === "number"
          ? message.start_sequence
          : endSeq;
      if (typeof endSeq === "number" && market) {
        const prior = this.lastSequence[market];
        if (prior != null && startSeq !== prior + 1) {
          this.sequenceGaps += 1;
        }
        this.lastSequence[market] = endSeq;
      }
    }
    if (type === "resync_required") {
      this.resyncCount += 1;
      this.resyncDetails = this.resyncDetails ?? [];
      if (this.resyncDetails.length < 3) {
        this.resyncDetails.push({
          atMs: this.firstSnapshotAt
            ? now() - this.firstSnapshotAt
            : now() - this.connectStartedAt,
          phase: this.firstSnapshotAt ? "post-snapshot" : "pre-snapshot",
          reason: message.reason,
          expected: message.expected_sequence,
          current: message.current_sequence,
        });
      }
    }
    if (this.index === 0) {
      this.ringBuffer.push(message);
      if (this.ringBuffer.length > 20) this.ringBuffer.shift();
    }
  }
}

function spawnClient(index) {
  const keyEntry = keyForConnection(index);
  const state = new ClientState(index, keyEntry);
  const socket = new WebSocket(baseUrl);
  state.socket = socket;

  socket.on("open", () => {
    state.openedAt = now();
    socket.send(JSON.stringify({ op: "authenticate", api_key: state.apiKey }));
  });

  socket.on("message", (raw) => {
    const text = raw.toString();
    state.noteMessage(text);
    if (state.authenticatedAt == null && text.includes('"authenticated"')) {
      state.authenticatedAt = now();
      for (const market of markets) {
        socket.send(
          JSON.stringify({ op: "subscribe", channel: "data", market }),
        );
      }
    }
  });

  socket.on("error", () => {
    state.errorCount += 1;
  });

  socket.on("close", (code) => {
    state.closedAt = now();
    state.closeCode = code;
  });

  clients.push(state);
}

async function main() {
  const overallStart = now();
  console.log(
    `spawning ${cli.connections} WS clients → ${baseUrl} (markets: ${markets.join(
      ",",
    )}, keys=${keyPool.length} → ${(cli.connections / keyPool.length).toFixed(2)} conn/key)`,
  );

  for (let i = 0; i < cli.connections; i += 1) {
    spawnClient(i);
    if (i % 10 === 9) await new Promise((r) => setTimeout(r, 50));
  }

  const progressEveryMs = 5000;
  const endAt = overallStart + cli.durationSeconds * 1000;
  let lastProgressAt = overallStart;
  let lastTotal = 0;
  while (now() < endAt) {
    await new Promise((r) => setTimeout(r, 250));
    if (now() - lastProgressAt >= progressEveryMs) {
      const total = clients.reduce((sum, c) => sum + c.messageCount, 0);
      const delta = total - lastTotal;
      const windowSec = (now() - lastProgressAt) / 1000;
      console.log(
        `t=${Math.round((now() - overallStart) / 1000)}s  msgs=${total}  +${(
          delta / windowSec
        ).toFixed(0)}/s  open=${clients.filter((c) => c.socket.readyState === 1).length}`,
      );
      lastProgressAt = now();
      lastTotal = total;
    }
  }

  for (const client of clients) {
    try {
      client.socket.close(1000, "stress-test-complete");
    } catch {
      // ignore
    }
  }
  await new Promise((r) => setTimeout(r, 500));

  const overallEnd = now();
  const totalMessages = clients.reduce((sum, c) => sum + c.messageCount, 0);
  const openDurations = clients
    .filter((c) => c.openedAt != null)
    .map((c) => c.openedAt - c.connectStartedAt);
  const authDurations = clients
    .filter((c) => c.authenticatedAt != null)
    .map((c) => c.authenticatedAt - c.connectStartedAt);
  const snapshotDurations = clients
    .filter((c) => c.firstSnapshotAt != null)
    .map((c) => c.firstSnapshotAt - c.connectStartedAt);
  const closedEarly = clients.filter(
    (c) =>
      c.closedAt != null && c.closedAt < endAt - 200 && c.closeCode !== 1000,
  );

  function stats(label, arr) {
    if (arr.length === 0) return `${label}: n=0`;
    const sorted = [...arr].sort((a, b) => a - b);
    const p50 = sorted[Math.floor(sorted.length * 0.5)];
    const p95 = sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * 0.95))];
    const p99 = sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * 0.99))];
    return `${label}: n=${arr.length} p50=${p50}ms p95=${p95}ms p99=${p99}ms max=${sorted[sorted.length - 1]}ms`;
  }

  const typeTotals = Object.create(null);
  let totalGaps = 0;
  let totalResyncs = 0;
  const resyncExamples = [];
  for (const c of clients) {
    for (const [type, count] of Object.entries(c.typeCounts)) {
      typeTotals[type] = (typeTotals[type] ?? 0) + count;
    }
    totalGaps += c.sequenceGaps;
    totalResyncs += c.resyncCount;
    if (c.resyncDetails) {
      for (const detail of c.resyncDetails) {
        if (resyncExamples.length < 5) resyncExamples.push({ client: c.identifier, ...detail });
      }
    }
  }

  const durationSec = (overallEnd - overallStart) / 1000;
  console.log("\n=== WS stress test results ===");
  console.log(`duration:            ${durationSec.toFixed(1)}s`);
  console.log(`connections:         ${clients.length}`);
  console.log(`opened:              ${openDurations.length}`);
  console.log(`authenticated:       ${authDurations.length}`);
  console.log(`snapshots received:  ${snapshotDurations.length}`);
  console.log(`closed early:        ${closedEarly.length}`);
  console.log(stats("connect latency", openDurations));
  console.log(stats("auth latency   ", authDurations));
  console.log(stats("snapshot latency", snapshotDurations));
  console.log(`total messages:      ${totalMessages}`);
  console.log(
    `aggregate msg/sec:   ${(totalMessages / durationSec).toFixed(0)}`,
  );
  console.log(
    `per-client msg/sec:  ${(totalMessages / durationSec / Math.max(clients.length, 1)).toFixed(1)}`,
  );
  console.log(`sequence gaps:       ${totalGaps}`);
  console.log(`resync_required:     ${totalResyncs}`);
  if (resyncExamples.length > 0) {
    console.log("resync examples:");
    for (const example of resyncExamples) {
      console.log(
        `  ${example.client} phase=${example.phase} atMs=${example.atMs} expected=${example.expected} current=${example.current} reason=${example.reason}`,
      );
    }
  }
  console.log("message type totals:");
  for (const [type, count] of Object.entries(typeTotals).sort(
    (a, b) => b[1] - a[1],
  )) {
    console.log(`  ${type.padEnd(20)} ${count}`);
  }

  const sample = clients[0];
  if (sample) {
    console.log(
      `\nlast ${sample.ringBuffer.length} messages on connection #0 (${sample.identifier}):`,
    );
    for (const message of sample.ringBuffer) {
      const preview = JSON.stringify(message);
      console.log(
        `  ${message.type?.padEnd(14) ?? "unknown"} ${preview.slice(0, 220)}${preview.length > 220 ? "…" : ""}`,
      );
    }
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
