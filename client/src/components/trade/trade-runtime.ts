import type { MarketDefinition } from "@/components/trade/trade-types";

export type TradeRuntimeConfig = {
  httpUrl: string;
  wsUrl: string;
  apiKey?: string;
  markets: MarketDefinition[];
  reconnectDelayMs: number;
};

const DEFAULT_HTTP_URL = "http://localhost:8080";
const DEFAULT_WS_URL = "ws://localhost:8080/ws";
const DEFAULT_RECONNECT_DELAY_MS = 1_500;

function splitMarketId(id: string) {
  const separatorIndex = id.lastIndexOf("-");
  if (separatorIndex <= 0 || separatorIndex >= id.length - 1) {
    return { baseAsset: id, quoteAsset: "USD" };
  }

  return {
    baseAsset: id.slice(0, separatorIndex),
    quoteAsset: id.slice(separatorIndex + 1),
  };
}

function toMarketDefinition(entry: string): MarketDefinition {
  const [rawId, rawLabel] = entry.split("|");
  const id = rawId.trim();
  const { baseAsset, quoteAsset } = splitMarketId(id);

  return {
    id,
    name: rawLabel?.trim() || id,
    baseAsset,
    quoteAsset,
    status: "enabled",
  };
}

export function createTradeRuntimeConfig(
  env: NodeJS.ProcessEnv = process.env,
): TradeRuntimeConfig {
  const markets = (env.NEXT_PUBLIC_EXCHANGE_MARKETS ?? "")
    .split(",")
    .map((entry) => entry.trim())
    .filter(Boolean)
    .map(toMarketDefinition);

  return {
    httpUrl: env.NEXT_PUBLIC_EXCHANGE_HTTP_URL || DEFAULT_HTTP_URL,
    wsUrl: env.NEXT_PUBLIC_EXCHANGE_WS_URL || DEFAULT_WS_URL,
    apiKey: env.NEXT_PUBLIC_EXCHANGE_API_KEY || undefined,
    markets,
    reconnectDelayMs: DEFAULT_RECONNECT_DELAY_MS,
  };
}
