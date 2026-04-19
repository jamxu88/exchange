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
  team_number: string;
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

export type ExchangeDispatchQueueStatus = {
  mode: "disabled" | "ok" | "backpressured" | "stopped";
  queue_capacity: number;
  backpressure_threshold: number;
  queue_depth: number;
  high_water_mark: number;
  total_enqueued: number;
  total_dequeued: number;
  total_blocked_enqueues: number;
  total_enqueue_block_time_ms: number;
};

export type ExchangeBarrierWaitStatus = {
  total_waits: number;
  total_wait_time_ms: number;
  max_wait_time_ms: number;
  last_wait_time_ms: number;
  waits_over_1ms: number;
  waits_over_5ms: number;
  waits_over_25ms: number;
  waits_over_100ms: number;
};

export type ExchangeAccountBarrierStatus = {
  submit: ExchangeBarrierWaitStatus;
  cancel: ExchangeBarrierWaitStatus;
  amend: ExchangeBarrierWaitStatus;
};

export type ExchangeHealth = {
  status: "ok" | "degraded";
  service: string;
  now: string;
  persistence: ExchangePersistenceStatus;
  runtime_dispatch: ExchangeDispatchQueueStatus;
  account_dispatch: ExchangeDispatchQueueStatus;
  persistence_dispatch: ExchangeDispatchQueueStatus;
  account_barrier: ExchangeAccountBarrierStatus;
};

export type ExchangeActionTelemetry = {
  total: number;
  accepted: number;
  rejected: number;
  total_per_second_10s: number;
  accepted_per_second_10s: number;
  rejected_per_second_10s: number;
};

export type ExchangeFillTelemetry = {
  total: number;
  shares: number;
  fills_per_second_10s: number;
  shares_per_second_10s: number;
};

export type ExchangeCounterTelemetry = {
  total: number;
  per_second_10s: number;
};

export type ExchangeWebSocketTelemetry = {
  connections_current: number;
  connections_total: number;
  authenticated_current: number;
  authenticated_total: number;
  data_stream_subscribers_current: number;
};

export type ExchangeResyncTelemetry = {
  user: ExchangeCounterTelemetry;
  system: ExchangeCounterTelemetry;
  data_stream: ExchangeCounterTelemetry;
};

export type ExchangeOperatorTelemetry = {
  submits: ExchangeActionTelemetry;
  cancels: ExchangeActionTelemetry;
  amends: ExchangeActionTelemetry;
  fills: ExchangeFillTelemetry;
  rate_limit_rejections: ExchangeCounterTelemetry;
  websocket: ExchangeWebSocketTelemetry;
  resyncs: ExchangeResyncTelemetry;
};

export type ExchangeAdminTelemetry = ExchangeHealth & {
  traffic: ExchangeOperatorTelemetry;
};

export type ExchangeMarket = {
  market_id: string;
  display_name: string;
  base_asset: string;
  quote_asset: string;
  tick_size: number;
  min_order_quantity: number;
  min_price: number | null;
  max_price: number | null;
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

export type ExchangeAdminBot = {
  bot_id: string;
  display_name: string;
  trader_id: string;
  trader_username: string;
  market_id: string;
  strategy: "maker" | "taker";
  side_mode: "buy" | "sell" | "both";
  status: "paused" | "running";
  min_quantity: number;
  max_quantity: number;
  interval_ms: number;
  max_open_orders: number;
  min_price: number;
  max_price: number;
  last_error: string | null;
  last_submitted_at: string | null;
  created_at: string;
  updated_at: string;
};

export type ExchangeAdminDesk = {
  trader_id: string;
  username: string;
  position_limit: number | null;
  created_at: string;
};

export type ExchangeControls = {
  trading_enabled: boolean;
  updated_at: string;
};

export type ExchangeAdminState = {
  controls: ExchangeControls;
  markets: ExchangeMarket[];
  bots: ExchangeAdminBot[];
  admin_desk: ExchangeAdminDesk | null;
  recent_messages: ExchangeAdminMessage[];
  persistence: ExchangePersistenceStatus;
};

type ExchangeAdminStatePayload = Omit<
  ExchangeAdminState,
  "bots" | "admin_desk" | "recent_messages"
> & {
  bots?: ExchangeAdminBot[];
  admin_desk?: ExchangeAdminDesk | null;
  recent_messages?: ExchangeAdminMessage[];
};

export type ExchangeFill = {
  fill_id: string;
  market: string;
  maker_order_id: string;
  taker_order_id: string;
  price: number;
  quantity: number;
  occurred_at: string;
};

export type ExchangeSubmittedOrder = {
  id: string;
  trader_id: string;
  market: string;
  side: "BUY" | "SELL";
  price: number;
  quantity: number;
  remaining: number;
  created_at: string;
};

export type ExchangeAdminDeskOrderResponse = {
  desk: ExchangeAdminDesk;
  submission: {
    order: ExchangeSubmittedOrder;
    fills: ExchangeFill[];
    resting: boolean;
  };
};

export type ExchangeLeaderboardRow = {
  rank: number;
  trader_id: string;
  team_number: string;
  net_pnl: number;
  realized_pnl: number;
  unrealized_pnl: number;
  gross_exposure: number;
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
  const adminState = await exchangeRequest<ExchangeAdminStatePayload>("/api/v1/admin/state", {
    adminToken,
  });

  return {
    ...adminState,
    bots: Array.isArray(adminState.bots) ? adminState.bots : [],
    admin_desk: adminState.admin_desk ?? null,
    recent_messages: Array.isArray(adminState.recent_messages)
      ? adminState.recent_messages
      : [],
  };
}

export type ProvisionedUserCredential = {
  trader_id: string;
  username: string;
  api_key: string;
  role: "trader" | "admin";
};

export type ProvisionedUsersResponse = {
  users: ProvisionedUserCredential[];
};

export async function listProvisionedUsers(
  adminToken: string,
  filters?: { role?: "trader" | "admin"; usernamePrefix?: string; limit?: number },
) {
  const params = new URLSearchParams();
  if (filters?.role) params.set("role", filters.role);
  if (filters?.usernamePrefix) params.set("username_prefix", filters.usernamePrefix);
  if (typeof filters?.limit === "number") params.set("limit", String(filters.limit));
  const query = params.toString();
  const path = query ? `/api/v1/admin/users?${query}` : "/api/v1/admin/users";
  return exchangeRequest<ProvisionedUsersResponse>(path, { adminToken });
}

export async function getAdminLeaderboard(adminToken: string, limit?: number) {
  const path = typeof limit === "number"
    ? `/api/v1/admin/leaderboard?limit=${limit}`
    : "/api/v1/admin/leaderboard";
  return exchangeRequest<ExchangeLeaderboardRow[]>(path, {
    adminToken,
  });
}

export async function getPublicMarkets() {
  return exchangeRequest<ExchangeMarket[]>("/api/v1/markets");
}

export async function getExchangeHealth() {
  return exchangeRequest<ExchangeHealth>("/health");
}

export async function getAdminTelemetry(adminToken: string) {
  return exchangeRequest<ExchangeAdminTelemetry>("/api/v1/admin/telemetry", {
    adminToken,
  });
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
