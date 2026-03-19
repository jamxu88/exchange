import type { SessionUser, UserRole } from "@/lib/auth";
import { createSessionForApiKey } from "@/lib/auth";

type ExchangeRequestOptions = {
  apiKey?: string;
  adminToken?: string;
  method?: string;
  body?: string;
};

type ExchangeErrorPayload = {
  error?: string;
};

type ExchangeUserResponse = {
  trader_id: string;
  username: string;
};

export type ExchangePersistenceStatus = {
  backend: "in_memory" | "postgres";
  mode: "disabled" | "ok" | "backpressured" | "retrying" | "stopped";
  queue_capacity: number;
  backpressure_threshold: number;
  queue_depth: number;
  in_flight_ops: number;
  backlog_depth: number;
  high_water_mark: number;
  total_enqueued: number;
  total_flushes: number;
  total_flushed_ops: number;
  total_blocked_enqueues: number;
  total_enqueue_block_time_ms: number;
  total_flush_failures: number;
  total_retries: number;
  last_batch_size: number;
  last_flush_latency_ms: number;
  max_flush_latency_ms: number;
  last_error: string | null;
};

export type ExchangeMarket = {
  market_id: string;
  display_name: string;
  base_asset: string;
  quote_asset: string;
  tick_size: number;
  min_order_quantity: number;
  reference_price: number | null;
  settlement_price: number | null;
  status: "enabled" | "disabled" | "settled";
  created_at: string;
  updated_at: string;
};

export type ExchangeAdminMessage = {
  message_id: string;
  target_username: string | null;
  target_trader_id: string | null;
  market: string | null;
  level: "info" | "warning" | "critical";
  title: string | null;
  body: string;
  created_at: string;
};

export type ExchangeControls = {
  trading_enabled: boolean;
  updated_at: string;
};

export type ExchangeAdminState = {
  controls: ExchangeControls;
  markets: ExchangeMarket[];
  recent_messages: ExchangeAdminMessage[];
  persistence: ExchangePersistenceStatus;
};

export type ExchangeLeaderboardRow = {
  rank: number;
  trader_id: string;
  username: string;
  equity: number;
  available_cash: number;
  locked_cash: number;
  position_value: number;
};

export class ExchangeServerError extends Error {
  status: number;

  constructor(message: string, status = 500) {
    super(message);
    this.name = "ExchangeServerError";
    this.status = status;
  }
}

function exchangeHttpUrl() {
  return process.env.EXCHANGE_HTTP_URL ?? process.env.NEXT_PUBLIC_EXCHANGE_HTTP_URL ?? "http://localhost:8080";
}

function joinUrl(baseUrl: string, path: string) {
  return new URL(path, baseUrl.endsWith("/") ? baseUrl : `${baseUrl}/`).toString();
}

function parseExchangePayload<T>(text: string): T | ExchangeErrorPayload | string | null {
  if (!text) {
    return null;
  }

  try {
    return JSON.parse(text) as T | ExchangeErrorPayload;
  } catch {
    return text;
  }
}

async function exchangeRequest<T>(
  path: string,
  options: ExchangeRequestOptions = {},
): Promise<T> {
  const url = joinUrl(exchangeHttpUrl(), path);
  let response: Response;
  try {
    response = await fetch(url, {
      method: options.method ?? "GET",
      cache: "no-store",
      headers: {
        accept: "application/json",
        ...(options.body ? { "content-type": "application/json" } : {}),
        ...(options.apiKey ? { "x-api-key": options.apiKey } : {}),
        ...(options.adminToken ? { authorization: `Bearer ${options.adminToken}` } : {}),
      },
      ...(options.body ? { body: options.body } : {}),
    });
  } catch (error) {
    const reason = error instanceof Error ? error.message : "unknown fetch error";
    throw new ExchangeServerError(
      `Failed to reach exchange at ${url}. Check EXCHANGE_HTTP_URL / NEXT_PUBLIC_EXCHANGE_HTTP_URL. ${reason}`,
      503,
    );
  }
  const text = await response.text();
  const payload = parseExchangePayload<T>(text);

  if (!response.ok) {
    const message =
      payload && typeof payload === "object" && "error" in payload && payload.error
        ? payload.error
        : typeof payload === "string" && payload.trim().length > 0
          ? payload
        : `Exchange request failed with ${response.status}`;
    throw new ExchangeServerError(message, response.status);
  }

  if (typeof payload === "string") {
    throw new ExchangeServerError(
      `Exchange returned a non-JSON success response from ${url}`,
      502,
    );
  }

  return payload as T;
}

export async function authenticateExchangeSession(apiKey: string): Promise<SessionUser> {
  const normalized = apiKey.trim();
  if (!normalized) {
    throw new ExchangeServerError("missing api key", 400);
  }

  try {
    await getAdminState(normalized);
    return createSessionForApiKey(normalized, "admin");
  } catch (error) {
    if (!(error instanceof ExchangeServerError) || error.status !== 401) {
      throw error;
    }
  }

  await getTraderProfile(normalized);
  return createSessionForApiKey(normalized, "trader");
}

export async function getTraderProfile(apiKey: string) {
  return exchangeRequest<ExchangeUserResponse>("/api/v1/user", { apiKey });
}

export async function getAdminState(adminToken: string) {
  return exchangeRequest<ExchangeAdminState>("/api/v1/admin/state", {
    adminToken,
  });
}

export async function getAdminLeaderboard(adminToken: string, limit = 10) {
  return exchangeRequest<ExchangeLeaderboardRow[]>(
    `/api/v1/admin/leaderboard?limit=${limit}`,
    {
      adminToken,
    },
  );
}

export async function getPublicMarkets() {
  return exchangeRequest<ExchangeMarket[]>("/api/v1/markets");
}

export async function sendAdminMutation<T>(
  adminToken: string,
  path: string,
  method: "POST" | "PATCH" | "DELETE",
  body?: unknown,
) {
  return exchangeRequest<T>(path, {
    adminToken,
    method,
    body: body === undefined ? undefined : JSON.stringify(body),
  });
}

export async function validateSessionRole(session: SessionUser, role: UserRole) {
  if (role === "admin") {
    await getAdminState(session.apiKey);
    return;
  }
  await getTraderProfile(session.apiKey);
}
