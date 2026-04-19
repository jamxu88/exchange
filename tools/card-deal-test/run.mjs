#!/usr/bin/env node
/*
  End-to-end test for the admin "Deal cards to teams" broadcaster.

  1. Opens one WS connection per team API key (default: all 100 in batch-1).
  2. Authenticates each, captures the server-assigned trader_id + team_number.
  3. Acts as the broadcaster:
       - GET /api/v1/admin/users?role=trader         (roster)
       - For every connected team, picks 3 positions from {1,2,4,5,7,8,10}
         and POSTs /api/v1/admin/messages with target_username + body.
  4. Each WS client captures its inbound `admin_message` events.
  5. Verifies:
       - every team received exactly one card-deal message
       - message body lists the 3 expected positions + card labels
       - no team received a message targeted at a different team
       - no unconnected team "leaked" messages to connected teams

  Run:
    ADMIN_API_TOKEN=... node run.mjs \
      --base-url wss://exchange.jamesxu.dev/ws \
      --http-url https://exchange.jamesxu.dev \
      --keys-file ../../allocated-api-keys-batch-1.csv
*/

import WebSocket from "../../client/node_modules/ws/wrapper.mjs";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));

function parseArgs(argv) {
  const args = {};
  for (let i = 2; i < argv.length; i += 1) {
    const key = argv[i];
    const value = argv[i + 1];
    if (key === "--base-url") {
      args.baseUrl = value;
      i += 1;
    } else if (key === "--http-url") {
      args.httpUrl = value;
      i += 1;
    } else if (key === "--keys-file") {
      args.keysFile = value;
      i += 1;
    } else if (key === "--max-teams") {
      args.maxTeams = Number(value);
      i += 1;
    } else if (key === "--round-label") {
      args.roundLabel = value;
      i += 1;
    } else if (key === "--post-wait-ms") {
      args.postWaitMs = Number(value);
      i += 1;
    }
  }
  return args;
}

const cli = parseArgs(process.argv);
const wsBaseUrl = cli.baseUrl ?? "wss://exchange.jamesxu.dev/ws";
const httpBaseUrl = cli.httpUrl ?? "https://exchange.jamesxu.dev";
const keysFile =
  cli.keysFile ?? path.resolve(SCRIPT_DIR, "..", "..", "allocated-api-keys-batch-1.csv");
const roundLabel = cli.roundLabel ?? "Smoke Test Round";
const postWaitMs = Number.isFinite(cli.postWaitMs) ? cli.postWaitMs : 4000;
const adminToken = process.env.ADMIN_API_TOKEN;
if (!adminToken) {
  console.error("ADMIN_API_TOKEN env var is required.");
  process.exit(1);
}

const CARD_VALUES = ["A", "2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K"];
const SUITS = [
  { id: "S", label: "spades" },
  { id: "H", label: "hearts" },
  { id: "D", label: "diamonds" },
  { id: "C", label: "clubs" },
];
const DEALABLE_POSITIONS = [1, 2, 4, 5, 7, 8, 10];
const SUIT_LABEL_BY_ID = Object.fromEntries(SUITS.map((s) => [s.id, s.label]));

function now() {
  return Number(process.hrtime.bigint() / 1_000_000n);
}

function randInt(n) {
  return Math.floor(Math.random() * n);
}

function pickThreePositions() {
  const pool = [...DEALABLE_POSITIONS];
  for (let i = pool.length - 1; i > 0; i -= 1) {
    const j = randInt(i + 1);
    [pool[i], pool[j]] = [pool[j], pool[i]];
  }
  return pool.slice(0, 3).sort((a, b) => a - b);
}

function randomCardSet() {
  const cards = [];
  for (let i = 0; i < 10; i += 1) {
    cards.push({
      value: CARD_VALUES[randInt(CARD_VALUES.length)],
      suit: SUITS[randInt(SUITS.length)].id,
    });
  }
  return cards;
}

function loadKeys() {
  const raw = fs.readFileSync(keysFile, "utf8");
  const lines = raw.split("\n").slice(1); // skip header
  const rows = [];
  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    const [identifier, apiKey] = trimmed.split(",");
    if (!identifier || !apiKey) continue;
    rows.push({ identifier: identifier.trim(), apiKey: apiKey.trim() });
  }
  return rows;
}

async function main() {
  const allKeys = loadKeys();
  const keys =
    Number.isFinite(cli.maxTeams) && cli.maxTeams > 0
      ? allKeys.slice(0, cli.maxTeams)
      : allKeys;
  console.log(
    `Loaded ${keys.length} team keys from ${path.relative(process.cwd(), keysFile)}`,
  );
  console.log(`WS → ${wsBaseUrl}`);
  console.log(`HTTP → ${httpBaseUrl}`);

  const states = keys.map((entry, index) => ({
    index,
    identifier: entry.identifier,
    apiKey: entry.apiKey,
    socket: null,
    traderId: null,
    username: null,
    authenticated: false,
    authError: null,
    receivedDeal: [],
    otherAdminMessages: [],
  }));

  // Open all WS sockets and authenticate.
  await new Promise((resolve) => {
    let doneAuth = 0;
    let resolved = false;
    const finish = () => {
      if (resolved) return;
      resolved = true;
      resolve();
    };
    const maybeDone = () => {
      if (doneAuth >= states.length) finish();
    };
    for (const state of states) {
      const socket = new WebSocket(wsBaseUrl);
      state.socket = socket;
      socket.on("open", () => {
        socket.send(JSON.stringify({ op: "authenticate", api_key: state.apiKey }));
      });
      socket.on("message", (raw) => {
        let msg;
        try {
          msg = JSON.parse(raw.toString());
        } catch {
          return;
        }
        if (msg.type === "authenticated" && !state.authenticated) {
          state.authenticated = true;
          state.traderId = msg.trader_id;
          state.username = msg.team_number;
          doneAuth += 1;
          maybeDone();
        } else if (msg.type === "admin_message") {
          const body = msg.message?.body ?? "";
          if (typeof body === "string" && body.startsWith("Your card reveal:")) {
            state.receivedDeal.push(msg.message);
          } else {
            state.otherAdminMessages.push(msg.message);
          }
        } else if (msg.type === "error" && !state.authenticated) {
          state.authError = msg.message ?? "error";
          doneAuth += 1;
          maybeDone();
        }
      });
      socket.on("error", (err) => {
        if (!state.authenticated) {
          state.authError = err?.message ?? "ws error";
          doneAuth += 1;
          maybeDone();
        }
      });
      socket.on("close", () => {
        if (!state.authenticated && state.authError == null) {
          state.authError = "closed before auth";
          doneAuth += 1;
          maybeDone();
        }
      });
    }

    // Hard timeout.
    setTimeout(() => {
      console.warn(
        `Auth wait timed out with ${doneAuth}/${states.length} resolved. Proceeding.`,
      );
      finish();
    }, 15000);
  });

  const authed = states.filter((s) => s.authenticated);
  const failed = states.filter((s) => !s.authenticated);
  console.log(`Authenticated: ${authed.length}/${states.length}`);
  if (failed.length > 0) {
    console.log(`  failures: ${failed.slice(0, 5).map((s) => `${s.identifier}(${s.authError})`).join(", ")}${failed.length > 5 ? " …" : ""}`);
  }
  if (authed.length === 0) {
    console.error("No WS clients authenticated; aborting.");
    process.exit(1);
  }

  const usernameToState = new Map(authed.map((s) => [s.username, s]));

  // 1. Fetch trader roster (what the broadcaster does first).
  console.log("\nGET /api/v1/admin/users?role=trader");
  const rosterRes = await fetch(`${httpBaseUrl}/api/v1/admin/users?role=trader`, {
    headers: { authorization: `Bearer ${adminToken}`, accept: "application/json" },
  });
  if (!rosterRes.ok) {
    console.error(`roster fetch failed: HTTP ${rosterRes.status} ${await rosterRes.text()}`);
    process.exit(1);
  }
  const roster = await rosterRes.json();
  const traders = (roster.users ?? []).filter((u) => u.role === "trader");
  console.log(`  roster size: ${traders.length}`);

  // 2. Choose a deterministic card set; deal per team.
  const cards = randomCardSet();
  console.log(`\nCards this round:`);
  for (let i = 0; i < 10; i += 1) {
    const card = cards[i];
    const marker = [3, 6, 9].includes(i + 1) ? " (public)" : "";
    console.log(`  Position ${(i + 1).toString().padStart(2)}: ${card.value} of ${SUIT_LABEL_BY_ID[card.suit]}${marker}`);
  }

  const expectedByUsername = new Map();
  for (const trader of traders) {
    const positions = pickThreePositions();
    expectedByUsername.set(trader.username, positions);
  }

  // 3. POST admin messages in parallel with limited concurrency.
  console.log(`\nPOSTing ${traders.length} private messages …`);
  const messageTitle = roundLabel.length > 0 ? roundLabel : null;
  const concurrency = 16;
  let cursor = 0;
  let sentOk = 0;
  let sendErrors = 0;
  const sendStart = now();
  async function worker() {
    while (cursor < traders.length) {
      const idx = cursor++;
      const trader = traders[idx];
      const positions = expectedByUsername.get(trader.username);
      const cardList = positions
        .map((pos) => {
          const card = cards[pos - 1];
          return `${card.value} of ${SUIT_LABEL_BY_ID[card.suit]} (pos ${pos})`;
        })
        .join(", ");
      const body = `Your card reveal: ${cardList}`;
      const payload = {
        title: messageTitle,
        body,
        level: "info",
        target_username: trader.username,
        market: null,
      };
      try {
        const res = await fetch(`${httpBaseUrl}/api/v1/admin/messages`, {
          method: "POST",
          headers: {
            accept: "application/json",
            "content-type": "application/json",
            authorization: `Bearer ${adminToken}`,
          },
          body: JSON.stringify(payload),
        });
        if (!res.ok) {
          sendErrors += 1;
          if (sendErrors <= 3) {
            console.warn(`  send failed for ${trader.username}: HTTP ${res.status} ${await res.text()}`);
          }
        } else {
          sentOk += 1;
        }
      } catch (err) {
        sendErrors += 1;
        if (sendErrors <= 3) console.warn(`  send threw for ${trader.username}: ${err.message}`);
      }
    }
  }
  await Promise.all(Array.from({ length: concurrency }, worker));
  console.log(`Sent ok=${sentOk} errors=${sendErrors} in ${(now() - sendStart).toFixed(0)}ms`);

  // 4. Wait for the fan-out to land on every client.
  console.log(`\nWaiting ${postWaitMs}ms for admin_message events to propagate…`);
  await new Promise((r) => setTimeout(r, postWaitMs));

  // 5. Verify each authenticated team got exactly one message with the expected positions+cards.
  let perfect = 0;
  let missing = 0;
  let duplicate = 0;
  let mismatched = 0;
  const mismatches = [];
  const missingSamples = [];

  for (const state of authed) {
    const expectedPositions = expectedByUsername.get(state.username);
    if (!expectedPositions) {
      // connected WS but not in roster (shouldn't happen)
      continue;
    }
    const deals = state.receivedDeal;
    if (deals.length === 0) {
      missing += 1;
      if (missingSamples.length < 5) missingSamples.push(state.username);
      continue;
    }
    if (deals.length > 1) duplicate += 1;
    const latest = deals[deals.length - 1];
    if (latest.target_username && latest.target_username !== state.username) {
      mismatched += 1;
      mismatches.push({ team: state.username, wrong_target: latest.target_username });
      continue;
    }
    // Parse "Your card reveal: <value> of <suit> (pos N), …"
    const body = String(latest.body ?? "");
    const prefix = "Your card reveal: ";
    const parsed = [];
    if (body.startsWith(prefix)) {
      const segments = body.slice(prefix.length).split(",").map((s) => s.trim());
      for (const segment of segments) {
        const m = segment.match(/^(\S+) of (\w+) \(pos (\d+)\)$/);
        if (m) parsed.push({ value: m[1], suit: m[2], position: Number(m[3]) });
      }
    }
    const parsedPositions = parsed.map((p) => p.position).sort((a, b) => a - b);
    const positionsMatch =
      parsed.length === 3 &&
      expectedPositions.length === 3 &&
      parsedPositions.every((pos, i) => pos === expectedPositions[i]);
    const cardsMatch = parsed.every((p) => {
      const card = cards[p.position - 1];
      return p.value === card.value && p.suit === SUIT_LABEL_BY_ID[card.suit];
    });
    if (!positionsMatch || !cardsMatch) {
      mismatched += 1;
      mismatches.push({
        team: state.username,
        expected: expectedPositions,
        got: parsedPositions,
        cardsMatch,
      });
    } else {
      perfect += 1;
    }
  }

  console.log("\n=== Verification ===");
  console.log(`  exact match:  ${perfect} / ${authed.length}`);
  console.log(`  missing:      ${missing}${missingSamples.length > 0 ? `  sample: ${missingSamples.join(", ")}` : ""}`);
  console.log(`  duplicate:    ${duplicate}`);
  console.log(`  mismatched:   ${mismatched}`);
  if (mismatches.length > 0) {
    for (const m of mismatches.slice(0, 5)) {
      console.log(`    ${JSON.stringify(m)}`);
    }
  }

  // Spot-check: print a sample message for team #1.
  const sample = authed.find((s) => s.identifier === "Team #1") ?? authed[0];
  if (sample && sample.receivedDeal.length > 0) {
    const last = sample.receivedDeal[sample.receivedDeal.length - 1];
    console.log(`\nSample for ${sample.identifier} (${sample.username}) →`);
    console.log(`  title: ${last.title}`);
    console.log(`  target_username: ${last.target_username}`);
    console.log(`  created_at: ${last.created_at}`);
    console.log(`  body:`);
    for (const line of String(last.body ?? "").split("\n")) {
      console.log(`    ${line}`);
    }
  }

  // Close WS clients.
  for (const state of states) {
    try {
      state.socket?.close(1000, "test-complete");
    } catch {
      // ignore
    }
  }
  await new Promise((r) => setTimeout(r, 500));

  if (perfect === authed.length && missing === 0 && mismatched === 0 && duplicate === 0) {
    console.log("\nPASS: every connected team received exactly the right card reveal.");
    process.exit(0);
  } else {
    console.log("\nFAIL: see counters above.");
    process.exit(1);
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
