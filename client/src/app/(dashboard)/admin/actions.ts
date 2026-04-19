"use server";

import { cookies } from "next/headers";
import { redirect } from "next/navigation";
import {
  resolveBulkBotDefinitions,
} from "@/app/(dashboard)/admin/bulk-bot-config";
import {
  deriveCompetitionBaseAsset,
  COMPETITION_QUOTE_ASSET,
  deriveCompetitionMarketId,
} from "@/app/(dashboard)/admin/market-utils";
import { resolveAdminMessageTargets } from "@/app/(dashboard)/admin/message-targets";
import { readSessionFromCookieValue, SESSION_COOKIE } from "@/lib/auth";
import {
  ExchangeAdminDeskOrderResponse,
  ExchangeServerError,
  listProvisionedUsers,
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

function readSharedBotMutationFields(formData: FormData) {
  return {
    market_id: String(formData.get("marketId") ?? "").trim(),
    strategy: String(formData.get("strategy") ?? "maker"),
    side_mode: String(formData.get("sideMode") ?? "both"),
    min_quantity: Number(formData.get("minQuantity") ?? 0),
    max_quantity: Number(formData.get("maxQuantity") ?? 0),
    interval_ms: Number(formData.get("intervalMs") ?? 0),
    max_open_orders: Number(formData.get("maxOpenOrders") ?? 0),
    min_price: Number(formData.get("minPrice") ?? 0),
    max_price: Number(formData.get("maxPrice") ?? 0),
    start_immediately: String(formData.get("startImmediately") ?? "") === "on",
  };
}

function buildBotMutationRequest(
  formData: FormData,
  overrides: { botId: string; displayName: string | null },
) {
  return {
    bot_id: overrides.botId,
    display_name: overrides.displayName,
    ...readSharedBotMutationFields(formData),
  };
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
      min: parseNumberField(formData, "minPrice"),
      max: parseNumberField(formData, "maxPrice"),
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
  const audience = String(formData.get("audience") ?? "single").trim();
  const targetUsername = asOptionalString(formData, "targetUsername");
  const targetUsernames = String(formData.get("targetUsernames") ?? "");
  const payload = {
    market: asOptionalString(formData, "market"),
    level: String(formData.get("level") ?? "info"),
    title: asOptionalString(formData, "title"),
    body: String(formData.get("body") ?? "").trim(),
  };

  const resolved = (() => {
    try {
      return resolveAdminMessageTargets(audience, targetUsername, targetUsernames);
    } catch (error) {
      adminRedirect({
        error: error instanceof Error ? error.message : "Admin action failed.",
      });
      throw new Error("unreachable");
    }
  })();

  const apiKey = await requireAdminApiKey();
  let sentCount = 0;
  let lastTarget: string | null = null;
  let successNotice = "Admin message sent.";

  try {
    if (resolved.audience === "all") {
      await sendAdminMutation(apiKey, "/api/v1/admin/messages", "POST", {
        ...payload,
        target_username: null,
      });
      successNotice = "Broadcast sent to all users.";
    } else {
      for (const username of resolved.targets) {
        lastTarget = username;
        await sendAdminMutation(apiKey, "/api/v1/admin/messages", "POST", {
          ...payload,
          target_username: username,
        });
        sentCount += 1;
      }

      successNotice =
        sentCount === 1
          ? `Admin message sent to ${resolved.targets[0]}.`
          : `Admin message sent to ${sentCount} users.`;
    }
  } catch (error) {
    if (error instanceof ExchangeServerError && error.status === 401) {
      redirect("/login?error=session-expired");
    }

    const message =
      error instanceof Error ? error.message : "Admin action failed.";

    if (sentCount > 0 && lastTarget) {
      adminRedirect({
        error: `Sent to ${sentCount} users before failing on ${lastTarget}. ${message}`,
      });
    }

    adminRedirect({ error: message });
  }

  adminRedirect({ notice: successNotice });
}

export async function saveBotAction(formData: FormData) {
  await runMutation(
    "/api/v1/admin/bots",
    "POST",
    buildBotMutationRequest(formData, {
      botId: String(formData.get("botId") ?? "").trim(),
      displayName: asOptionalString(formData, "displayName"),
    }),
    "Bot configuration saved.",
  );
}

export async function saveBotBatchAction(formData: FormData) {
  const definitions = (() => {
    try {
      return resolveBulkBotDefinitions({
        botIdPrefix: String(formData.get("botIdPrefix") ?? ""),
        displayNamePrefix: asOptionalString(formData, "displayNamePrefix"),
        count: Number(formData.get("botCount") ?? 0),
        startIndex: Number(formData.get("botStartIndex") ?? 1),
      });
    } catch (error) {
      adminRedirect({
        error: error instanceof Error ? error.message : "Admin action failed.",
      });
      throw new Error("unreachable");
    }
  })();

  const apiKey = await requireAdminApiKey();
  let savedCount = 0;
  let lastBotId: string | null = null;

  try {
    for (const definition of definitions) {
      lastBotId = definition.botId;
      await sendAdminMutation(apiKey, "/api/v1/admin/bots", "POST", buildBotMutationRequest(
        formData,
        {
          botId: definition.botId,
          displayName: definition.displayName,
        },
      ));
      savedCount += 1;
    }
  } catch (error) {
    if (error instanceof ExchangeServerError && error.status === 401) {
      redirect("/login?error=session-expired");
    }

    const message =
      error instanceof Error ? error.message : "Admin action failed.";

    if (savedCount > 0 && lastBotId) {
      adminRedirect({
        error: `Saved ${savedCount} bots before failing on ${lastBotId}. ${message}`,
      });
    }

    adminRedirect({ error: message });
  }

  const firstBotId = definitions[0]?.botId;
  const lastCreatedBotId = definitions[definitions.length - 1]?.botId;
  adminRedirect({
    notice:
      definitions.length === 1
        ? `Saved bot ${firstBotId}.`
        : `Saved ${definitions.length} bots from ${firstBotId} through ${lastCreatedBotId}.`,
  });
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

const CARD_VALUE_SET = new Set([
  "A",
  "2",
  "3",
  "4",
  "5",
  "6",
  "7",
  "8",
  "9",
  "10",
  "J",
  "Q",
  "K",
]);
const CARD_SUIT_SET = new Set(["S", "H", "D", "C"]);
const SUIT_LABELS: Record<string, string> = {
  S: "spades",
  H: "hearts",
  D: "diamonds",
  C: "clubs",
};
const DEALABLE_POSITIONS = [1, 2, 4, 5, 7, 8, 10] as const;

function pickThreePositions(): [number, number, number] {
  const pool = [...DEALABLE_POSITIONS];
  for (let i = pool.length - 1; i > 0; i -= 1) {
    const j = Math.floor(Math.random() * (i + 1));
    [pool[i], pool[j]] = [pool[j], pool[i]];
  }
  const picked = pool.slice(0, 3).sort((a, b) => a - b);
  return [picked[0], picked[1], picked[2]];
}

export async function sendCardDealsAction(formData: FormData) {
  const roundLabel = String(formData.get("roundLabel") ?? "").trim();
  const cards: Array<{ value: string; suit: string }> = [];
  for (let pos = 1; pos <= 10; pos += 1) {
    const value = String(formData.get(`value_${pos}`) ?? "").trim();
    const suit = String(formData.get(`suit_${pos}`) ?? "").trim().toUpperCase();
    if (!CARD_VALUE_SET.has(value)) {
      adminRedirect({ error: `Position ${pos}: pick a card value.` });
      throw new Error("unreachable");
    }
    if (!CARD_SUIT_SET.has(suit)) {
      adminRedirect({ error: `Position ${pos}: pick a suit.` });
      throw new Error("unreachable");
    }
    cards.push({ value, suit });
  }

  const apiKey = await requireAdminApiKey();

  let roster: Awaited<ReturnType<typeof listProvisionedUsers>>;
  try {
    roster = await listProvisionedUsers(apiKey, { role: "trader" });
  } catch (error) {
    if (error instanceof ExchangeServerError && error.status === 401) {
      redirect("/login?error=session-expired");
    }
    adminRedirect({
      error: error instanceof Error ? error.message : "Failed to load team roster.",
    });
    throw new Error("unreachable");
  }

  const teams = roster.users.filter((user) => user.role === "trader");
  if (teams.length === 0) {
    adminRedirect({ error: "No trader users to deal cards to." });
    throw new Error("unreachable");
  }

  // trade-store renders `${title}: ${body}` — keep title as round context only so we do not
  // duplicate "Your card reveal" (which lives in the body).
  const messageTitle = roundLabel.length > 0 ? roundLabel : null;
  let sentCount = 0;
  let lastTarget: string | null = null;

  try {
    for (const team of teams) {
      lastTarget = team.username;
      const positions = pickThreePositions();
      const cardList = positions
        .map((pos) => {
          const card = cards[pos - 1];
          return `${card.value} of ${SUIT_LABELS[card.suit]} (pos ${pos})`;
        })
        .join(", ");
      const body = `Your card reveal: ${cardList}`;

      await sendAdminMutation(apiKey, "/api/v1/admin/messages", "POST", {
        title: messageTitle,
        body,
        level: "info",
        target_username: team.username,
        market: null,
      });
      sentCount += 1;
    }
  } catch (error) {
    if (error instanceof ExchangeServerError && error.status === 401) {
      redirect("/login?error=session-expired");
    }
    const message =
      error instanceof Error ? error.message : "Admin action failed.";
    adminRedirect({
      error:
        sentCount > 0 && lastTarget
          ? `Dealt to ${sentCount} teams before failing on ${lastTarget}. ${message}`
          : message,
    });
    throw new Error("unreachable");
  }

  const summary = roundLabel.length > 0
    ? `Dealt private 3-card subsets to ${sentCount} teams for ${roundLabel}.`
    : `Dealt private 3-card subsets to ${sentCount} teams.`;
  adminRedirect({ notice: summary });
}
