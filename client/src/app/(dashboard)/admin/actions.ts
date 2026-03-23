"use server";

import { cookies } from "next/headers";
import { redirect } from "next/navigation";
import {
  COMPETITION_QUOTE_ASSET,
  deriveCompetitionMarketId,
  normalizeBaseAsset,
} from "@/app/(dashboard)/admin/market-utils";
import { readSessionFromCookieValue, SESSION_COOKIE } from "@/lib/auth";
import { ExchangeServerError, sendAdminMutation } from "@/lib/exchange-server";

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
  const apiKey = await requireAdminApiKey();
  try {
    await sendAdminMutation(apiKey, path, method, body);
  } catch (error) {
    if (error instanceof ExchangeServerError && error.status === 401) {
      redirect("/login?error=session-expired");
    }
    adminRedirect({
      error: error instanceof Error ? error.message : "Admin action failed.",
    });
  }
  adminRedirect({ notice: successNotice });
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
  const baseAsset = normalizeBaseAsset(String(formData.get("baseAsset") ?? ""));
  await runMutation(
    "/api/v1/admin/markets",
    "POST",
    {
      market_id: deriveCompetitionMarketId(baseAsset),
      display_name: asOptionalString(formData, "displayName"),
      base_asset: baseAsset,
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
