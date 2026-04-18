#!/usr/bin/env node

import { writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const CONFIG = {
  baseUrl: process.env.EXCHANGE_URL ?? "http://localhost:8080",
  adminToken: process.env.ADMIN_API_TOKEN ?? "",
  teamCount: Number.parseInt(process.env.TEAM_COUNT ?? "200", 10),
  batchSize: Number.parseInt(process.env.BATCH_SIZE ?? "100", 10),
  outputDir:
    process.env.OUTPUT_DIR ??
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", ".."),
  teamPrefix: process.env.TEAM_PREFIX ?? "Team ",
  identifierPrefix: process.env.IDENTIFIER_PREFIX ?? "Team #",
};

function requireConfig() {
  if (typeof fetch !== "function") {
    throw new Error("Node.js with global fetch support is required (Node 18+).");
  }
  if (!CONFIG.adminToken) {
    throw new Error("ADMIN_API_TOKEN env var is required.");
  }
  if (!Number.isInteger(CONFIG.teamCount) || CONFIG.teamCount <= 0) {
    throw new Error("TEAM_COUNT must be a positive integer.");
  }
  if (!Number.isInteger(CONFIG.batchSize) || CONFIG.batchSize <= 0) {
    throw new Error("BATCH_SIZE must be a positive integer.");
  }
  if (CONFIG.teamCount % CONFIG.batchSize !== 0) {
    throw new Error(
      `TEAM_COUNT (${CONFIG.teamCount}) must be evenly divisible by BATCH_SIZE (${CONFIG.batchSize}).`,
    );
  }
}

async function provisionTeam(teamIndex) {
  const username = `${CONFIG.teamPrefix}${teamIndex}`;
  const response = await fetch(`${CONFIG.baseUrl}/api/v1/admin/users`, {
    method: "POST",
    headers: {
      accept: "application/json",
      "content-type": "application/json",
      authorization: `Bearer ${CONFIG.adminToken}`,
    },
    body: JSON.stringify({
      username,
      team_number: username,
      role: "trader",
    }),
  });

  const text = await response.text();
  if (!response.ok) {
    throw new Error(
      `Provision failed for ${username} (HTTP ${response.status}): ${text || response.statusText}`,
    );
  }

  const payload = text ? JSON.parse(text) : null;
  const apiKey = payload?.profile?.api_key;
  if (typeof apiKey !== "string" || apiKey.length === 0) {
    throw new Error(
      `Provision response for ${username} missing api_key. Raw body: ${text}`,
    );
  }

  return { teamIndex, username, apiKey };
}

function batchFilename(batchIndex) {
  return `allocated-api-keys-batch-${batchIndex}.txt`;
}

async function writeBatch(batchIndex, rows) {
  const header = "identifier,api_key";
  const lines = rows.map(
    ({ teamIndex, apiKey }) => `${CONFIG.identifierPrefix}${teamIndex},${apiKey}`,
  );
  const body = [header, ...lines, ""].join("\n");
  const target = path.join(CONFIG.outputDir, batchFilename(batchIndex));
  await writeFile(target, body, "utf8");
  console.log(`Wrote ${rows.length} rows to ${target}`);
}

async function main() {
  requireConfig();

  console.log(
    `Provisioning ${CONFIG.teamCount} teams against ${CONFIG.baseUrl} (batch size ${CONFIG.batchSize}).`,
  );

  const rows = [];
  for (let teamIndex = 1; teamIndex <= CONFIG.teamCount; teamIndex += 1) {
    const row = await provisionTeam(teamIndex);
    rows.push(row);
    if (teamIndex % 25 === 0 || teamIndex === CONFIG.teamCount) {
      console.log(`  ... provisioned ${teamIndex}/${CONFIG.teamCount}`);
    }
  }

  const batchCount = CONFIG.teamCount / CONFIG.batchSize;
  for (let batchIndex = 1; batchIndex <= batchCount; batchIndex += 1) {
    const start = (batchIndex - 1) * CONFIG.batchSize;
    const end = start + CONFIG.batchSize;
    await writeBatch(batchIndex, rows.slice(start, end));
  }

  const unique = new Set(rows.map((row) => row.apiKey));
  if (unique.size !== rows.length) {
    throw new Error(
      `Duplicate api_keys returned by the exchange (unique=${unique.size}, total=${rows.length}).`,
    );
  }
  console.log(`Done. Generated ${rows.length} unique 7-char api keys.`);
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
});
