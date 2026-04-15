import { readFile } from "node:fs/promises";
import path from "node:path";

const DEFAULT_ALLOCATED_API_KEYS_FILE = "allocated-api-keys-2026-04-05.txt";

export class ApiKeyLookupError extends Error {
  status: number;

  constructor(message: string, status: number) {
    super(message);
    this.name = "ApiKeyLookupError";
    this.status = status;
  }
}

let cachedLookupTable: Promise<Map<string, string>> | null = null;

export function normalizeLookupIdentifier(identifier: string) {
  return identifier.trim().toUpperCase();
}

export function parseAllocatedApiKeysFile(contents: string) {
  const table = new Map<string, string>();
  const lines = contents.split(/\r?\n/);

  for (const line of lines.slice(1)) {
    const trimmed = line.trim();
    if (!trimmed) {
      continue;
    }

    const [rawIdentifier, rawApiKey] = trimmed.split("\t");
    const identifier = normalizeLookupIdentifier(rawIdentifier ?? "");
    const apiKey = rawApiKey?.trim();

    if (!identifier || !apiKey) {
      continue;
    }

    table.set(identifier, apiKey);
  }

  return table;
}

function allocatedApiKeysFileCandidates() {
  const configured = process.env.ALLOCATED_API_KEYS_FILE ?? DEFAULT_ALLOCATED_API_KEYS_FILE;

  return [
    path.join(process.cwd(), configured),
    path.join(process.cwd(), "..", configured),
  ];
}

async function loadAllocatedApiKeysFile() {
  let lastError: unknown;

  for (const candidate of allocatedApiKeysFileCandidates()) {
    try {
      return await readFile(candidate, "utf8");
    } catch (error) {
      lastError = error;
    }
  }

  throw lastError;
}

async function getLookupTable() {
  if (!cachedLookupTable) {
    cachedLookupTable = loadAllocatedApiKeysFile()
      .then((contents) => {
        const table = parseAllocatedApiKeysFile(contents);
        if (table.size === 0) {
          throw new ApiKeyLookupError("Allocated API key data is empty.", 503);
        }
        return table;
      })
      .catch((error) => {
        cachedLookupTable = null;

        if (error instanceof ApiKeyLookupError) {
          throw error;
        }

        throw new ApiKeyLookupError("Allocated API key data is unavailable.", 503);
      });
  }

  return cachedLookupTable;
}

export async function lookupAllocatedApiKey(identifier: string) {
  const normalizedIdentifier = normalizeLookupIdentifier(identifier);
  if (!normalizedIdentifier) {
    throw new ApiKeyLookupError("Identifier is required.", 400);
  }

  const table = await getLookupTable();
  const apiKey = table.get(normalizedIdentifier);
  if (!apiKey) {
    throw new ApiKeyLookupError("No API key was found for that identifier.", 404);
  }

  return {
    apiKey,
  };
}

export function resetAllocatedApiKeysCacheForTests() {
  cachedLookupTable = null;
}
