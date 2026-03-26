"use server";

import { cookies } from "next/headers";
import { redirect } from "next/navigation";
import {
  deriveCompetitionBaseAsset,
  COMPETITION_QUOTE_ASSET,
  deriveCompetitionMarketId,
} from "@/app/(dashboard)/admin/market-utils";
import { readSessionFromCookieValue, SESSION_COOKIE } from "@/lib/auth";
import {
  ExchangeAdminDeskOrderResponse,
  ExchangeServerError,
  sendAdminMutation,
} from "@/lib/exchange-server";

function adminRedirect(params: Record<string, string>) {
  const search = new URLSearchParams(params);
  redirect(`/admin?${search.toString()}`);
}

async function requireAdminApiKey() {
  const cookieStore = await cookies();
  const session = readSessionFromCookieValue(cookieStore.get(SESSION_COOKIE)?.value);
  if (!session) {
    redirect("/login");
  }
  return session.apiKey;
}

function asOptionalString(formData: FormData, key: string) {
  const value = String(formData.get(key) ?? "").trim();
  return value.length > 0 ? value : null;
}

function parseNumberField(formData: FormData, key: string) {
  const raw = String(formData.get(key) ?? "").trim();
  return raw.length > 0 ? Number(raw) : null;
}

async function runMutation<T>(
  path: string,
  method: "POST" | "PATCH" | "DELETE",
  body: T | undefined,
  successNotice: string,
) {
  await performMutation(path, method, body);
  adminRedirect({ notice: successNotice });
}

async function performMutation<TRequest, TResponse = void>(
  path: string,
  method: "POST" | "PATCH" | "DELETE",
  body: TRequest | undefined,
): Promise<TResponse> {
  const apiKey = await requireAdminApiKey();
  try {
    return await sendAdminMutation<TResponse>(apiKey, path, method, body);
  } catch (error) {
    if (error instanceof ExchangeServerError && error.status === 401) {
      redirect("/login?error=session-expired");
    }
    adminRedirect({
      error: error instanceof Error ? error.message : "Admin action failed.",
    });
    throw new Error("unreachable");
  }
}

export async function startTradingAction() {
  await runMutation("/api/v1/admin/trading/start", "POST", undefined, "Trading started.");
}

export async function stopTradingAction() {
  await runMutation("/api/v1/admin/trading/stop", "POST", undefined, "Trading stopped.");
}

export async function resetAllUsersAction() {
  await runMutation(
    "/api/v1/admin/users/reset",
    "POST",
    undefined,
    "All user positions, orders, and fills were reset.",
  );
}

export async function createMarketAction(formData: FormData) {
  const displayName = String(formData.get("displayName") ?? "").trim();
  await runMutation(
    "/api/v1/admin/markets",
    "POST",
    {
      market_id: deriveCompetitionMarketId(displayName),
      display_name: displayName,
      base_asset: deriveCompetitionBaseAsset(displayName),
      quote_asset: COMPETITION_QUOTE_ASSET,
      tick_size: Number(formData.get("tickSize") ?? 0),
      min_order_quantity: Number(formData.get("minOrderQuantity") ?? 0),
      reference_price: parseNumberField(formData, "referencePrice"),
      enabled: String(formData.get("enabled") ?? "on") === "on",
    },
    "Market saved.",
  );
}

export async function toggleMarketAction(formData: FormData) {
  const marketId = String(formData.get("marketId") ?? "").trim();
  const enable = String(formData.get("enable") ?? "") === "true";
  await runMutation(
    `/api/v1/admin/markets/${encodeURIComponent(marketId)}`,
    "PATCH",
    {
      enabled: enable,
    },
    enable ? `${marketId} enabled.` : `${marketId} disabled.`,
  );
}

export async function deleteMarketAction(formData: FormData) {
  const marketId = String(formData.get("marketId") ?? "").trim();
  await runMutation(
    `/api/v1/admin/markets/${encodeURIComponent(marketId)}`,
    "DELETE",
    undefined,
    `${marketId} deleted.`,
  );
}

export async function settleMarketAction(formData: FormData) {
  const marketId = String(formData.get("marketId") ?? "").trim();
  await runMutation(
    `/api/v1/admin/markets/${encodeURIComponent(marketId)}/settle`,
    "POST",
    {
      settlement_price: Number(formData.get("settlementPrice") ?? 0),
      announcement: asOptionalString(formData, "announcement"),
    },
    `${marketId} settled.`,
  );
}

export async function loadConfigAction(formData: FormData) {
  const raw = String(formData.get("config") ?? "").trim();
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    adminRedirect({ error: "Config payload must be valid JSON." });
  }

  await runMutation(
    "/api/v1/admin/config/load",
    "POST",
    parsed,
    "Exchange config loaded.",
  );
}

export async function sendMessageAction(formData: FormData) {
  await runMutation(
    "/api/v1/admin/messages",
    "POST",
    {
      target_username: asOptionalString(formData, "targetUsername"),
      market: asOptionalString(formData, "market"),
      level: String(formData.get("level") ?? "info"),
      title: asOptionalString(formData, "title"),
      body: String(formData.get("body") ?? "").trim(),
    },
    "Admin message sent.",
  );
}

export async function saveBotAction(formData: FormData) {
  await runMutation(
    "/api/v1/admin/bots",
    "POST",
    {
      bot_id: String(formData.get("botId") ?? "").trim(),
      display_name: asOptionalString(formData, "displayName"),
      market_id: String(formData.get("marketId") ?? "").trim(),
      order_type: String(formData.get("orderType") ?? "limit"),
      side_mode: String(formData.get("sideMode") ?? "both"),
      min_quantity: Number(formData.get("minQuantity") ?? 0),
      max_quantity: Number(formData.get("maxQuantity") ?? 0),
      interval_ms: Number(formData.get("intervalMs") ?? 0),
      max_open_orders: Number(formData.get("maxOpenOrders") ?? 0),
      price_offset_ticks: Number(formData.get("priceOffsetTicks") ?? 0),
      walk_step_ticks: Number(formData.get("walkStepTicks") ?? 0),
      fallback_price: parseNumberField(formData, "fallbackPrice"),
      start_immediately: String(formData.get("startImmediately") ?? "") === "on",
    },
    "Bot configuration saved.",
  );
}

export async function startBotAction(formData: FormData) {
  const botId = String(formData.get("botId") ?? "").trim();
  await runMutation(
    `/api/v1/admin/bots/${encodeURIComponent(botId)}/start`,
    "POST",
    undefined,
    `${botId} started.`,
  );
}

export async function pauseBotAction(formData: FormData) {
  const botId = String(formData.get("botId") ?? "").trim();
  await runMutation(
    `/api/v1/admin/bots/${encodeURIComponent(botId)}/pause`,
    "POST",
    undefined,
    `${botId} paused.`,
  );
}

export async function deleteBotAction(formData: FormData) {
  const botId = String(formData.get("botId") ?? "").trim();
  await runMutation(
    `/api/v1/admin/bots/${encodeURIComponent(botId)}`,
    "DELETE",
    undefined,
    `${botId} deleted.`,
  );
}

export async function ensureAdminDeskAction() {
  const desk = await performMutation<void, { username: string }>(
    "/api/v1/admin/desk/ensure",
    "POST",
    undefined,
  );
  adminRedirect({
    notice: `Admin desk ${desk.username} is ready for unlimited-position trading.`,
  });
}

function formatPrice(value: number) {
  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(value);
}

function weightedFillPrice(fills: ExchangeAdminDeskOrderResponse["submission"]["fills"]) {
  const totalQuantity = fills.reduce((sum, fill) => sum + fill.quantity, 0);
  if (totalQuantity <= 0) {
    return null;
  }

  const weightedSum = fills.reduce((sum, fill) => sum + fill.quantity * fill.price, 0);
  return weightedSum / totalQuantity;
}

export async function submitAdminDeskOrderAction(formData: FormData) {
  const response = await performMutation<
    {
      market: string;
      side: "BUY" | "SELL";
      order_type: "limit" | "market";
      price: number;
      quantity: number;
    },
    ExchangeAdminDeskOrderResponse
  >("/api/v1/admin/desk/orders", "POST", {
    market: String(formData.get("marketId") ?? "").trim(),
    side: String(formData.get("side") ?? "BUY").trim().toUpperCase() as "BUY" | "SELL",
    order_type: String(formData.get("orderType") ?? "limit").trim() as "limit" | "market",
    price: Number(formData.get("price") ?? 0),
    quantity: Number(formData.get("quantity") ?? 0),
  });

  const fills = response.submission.fills;
  const filledQuantity = fills.reduce((sum, fill) => sum + fill.quantity, 0);
  const executionPrice = weightedFillPrice(fills) ?? response.submission.order.price;
  const notice = response.submission.resting && response.submission.order.remaining > 0
    ? filledQuantity > 0
      ? `Admin desk ${response.desk.username} filled ${filledQuantity} at ${formatPrice(executionPrice)} and left ${response.submission.order.remaining} resting at ${formatPrice(response.submission.order.price)}.`
      : `Admin desk ${response.desk.username} placed ${response.submission.order.side} ${response.submission.order.market} for ${response.submission.order.quantity} shares at ${formatPrice(response.submission.order.price)}.`
    : `Admin desk ${response.desk.username} filled ${filledQuantity || response.submission.order.quantity} ${response.submission.order.market} shares at ${formatPrice(executionPrice)}.`;

  adminRedirect({ notice });
}
