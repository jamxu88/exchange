"use client";

import type { FormEvent, ReactNode } from "react";
import { useMemo, useState } from "react";
import {
  MAX_BULK_BOT_COUNT,
  resolveBulkBotDefinitions,
} from "@/app/(dashboard)/admin/bulk-bot-config";
import {
  COMPETITION_QUOTE_ASSET,
  deriveCompetitionBaseAsset,
  deriveCompetitionMarketId,
} from "@/app/(dashboard)/admin/market-utils";
import {
  botStatusClass,
  cardClass,
  dangerButtonClass,
  formatCurrency,
  formatTimestamp,
  inputClass,
  neutralButtonClass,
  primaryButtonClass,
  selectClass,
  textareaClass,
  warningButtonClass,
} from "@/app/(dashboard)/admin/ui";
import type {
  ExchangeAdminBot,
  ExchangeAdminState,
  ExchangeMarket,
} from "@/lib/exchange-server";

type AdminMarketBotManagerProps = {
  initialBots: ExchangeAdminBot[];
  initialMarkets: ExchangeMarket[];
};

type AdminMutationResponse = {
  adminState: ExchangeAdminState;
  notice: string;
};

type BotFormMarket = {
  market_id: string;
};

type BotConfigurationFieldsProps = {
  disabled: boolean;
  markets: BotFormMarket[];
  mode: "single" | "batch";
};

type AdminFieldProps = {
  label: string;
  children: ReactNode;
};

function AdminField({ label, children }: AdminFieldProps) {
  return (
    <label className="grid gap-2">
      <p className="ops-kicker text-white">{label}</p>
      {children}
    </label>
  );
}

function BotConfigurationFields({ disabled, markets, mode }: BotConfigurationFieldsProps) {
  const isBatch = mode === "batch";

  return (
    <>
      {isBatch ? (
        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
          <input
            className={inputClass}
            disabled={disabled}
            name="botIdPrefix"
            placeholder="Bot ID prefix, for example depth-maker"
            required
          />
          <input
            className={inputClass}
            disabled={disabled}
            name="displayNamePrefix"
            placeholder="Display name prefix"
          />
          <input
            className={inputClass}
            defaultValue="10"
            disabled={disabled}
            max={MAX_BULK_BOT_COUNT}
            min="1"
            name="botCount"
            placeholder="Bot count"
            required
            type="number"
          />
          <input
            className={inputClass}
            defaultValue="1"
            disabled={disabled}
            min="1"
            name="botStartIndex"
            placeholder="Start index"
            required
            type="number"
          />
        </div>
      ) : (
        <div className="grid gap-3 md:grid-cols-2">
          <input
            className={inputClass}
            disabled={disabled}
            name="botId"
            placeholder="Bot ID, for example depth-maker-1"
            required
          />
          <input
            className={inputClass}
            disabled={disabled}
            name="displayName"
            placeholder="Display name"
          />
        </div>
      )}
      <div className="grid gap-3 md:grid-cols-3">
        <select
          className={selectClass}
          defaultValue=""
          disabled={disabled || markets.length === 0}
          name="marketId"
          required
        >
          <option disabled value="">
            {markets.length === 0 ? "Create a market first" : "Select market"}
          </option>
          {markets.map((market) => (
            <option key={market.market_id} value={market.market_id}>
              {market.market_id}
            </option>
          ))}
        </select>
        <select
          className={selectClass}
          defaultValue=""
          disabled={disabled}
          name="sideMode"
          required
        >
          <option disabled value="">
            Select side mode
          </option>
          <option value="both">Both sides</option>
          <option value="buy">Buy only</option>
          <option value="sell">Sell only</option>
        </select>
        <select
          className={selectClass}
          defaultValue=""
          disabled={disabled}
          name="strategy"
          required
        >
          <option disabled value="">
            Select strategy
          </option>
          <option value="maker">Maker</option>
          <option value="taker">Taker</option>
        </select>
      </div>
      <div className="grid gap-3 md:grid-cols-3">
        <input
          className={inputClass}
          disabled={disabled}
          min="1"
          name="minQuantity"
          placeholder="Min qty"
          required
          type="number"
        />
        <input
          className={inputClass}
          disabled={disabled}
          min="1"
          name="maxQuantity"
          placeholder="Max qty"
          required
          type="number"
        />
        <input
          className={inputClass}
          disabled={disabled}
          min="0"
          name="intervalMs"
          placeholder="Interval ms"
          required
          type="number"
        />
      </div>
      <div className="grid gap-3 md:grid-cols-3">
        <input
          className={inputClass}
          defaultValue="1"
          disabled={disabled}
          min="1"
          name="maxOpenOrders"
          placeholder="Open order cap"
          required
          type="number"
        />
        <input
          className={inputClass}
          disabled={disabled}
          min="1"
          name="minPrice"
          placeholder="Minimum price"
          required
          type="number"
        />
        <input
          className={inputClass}
          disabled={disabled}
          min="1"
          name="maxPrice"
          placeholder="Maximum price"
          required
          type="number"
        />
      </div>
      <label className="flex items-center gap-3 text-sm text-[var(--muted-strong)]">
        <input className="ops-check" disabled={disabled} name="startImmediately" type="checkbox" />
        Start immediately after saving
      </label>
    </>
  );
}

function parseResponsePayload(text: string) {
  if (!text) {
    return null;
  }

  try {
    return JSON.parse(text) as { error?: string } | AdminMutationResponse | ExchangeAdminState;
  } catch {
    return text;
  }
}

async function readMutationResponse(response: Response) {
  const text = await response.text();
  const payload = parseResponsePayload(text);

  if (!response.ok) {
    const message =
      payload && typeof payload === "object" && "error" in payload && typeof payload.error === "string"
        ? payload.error
        : typeof payload === "string" && payload.trim().length > 0
          ? payload
          : `Admin request failed with ${response.status}`;
    throw new Error(message);
  }

  if (!payload || typeof payload !== "object" || !("adminState" in payload)) {
    throw new Error("Admin request returned an invalid response.");
  }

  return payload as AdminMutationResponse;
}

async function readAdminState(response: Response) {
  const text = await response.text();
  const payload = parseResponsePayload(text);

  if (!response.ok) {
    const message =
      payload && typeof payload === "object" && "error" in payload && typeof payload.error === "string"
        ? payload.error
        : `Admin request failed with ${response.status}`;
    throw new Error(message);
  }

  if (!payload || typeof payload !== "object" || !("markets" in payload) || !("bots" in payload)) {
    throw new Error("Admin state response was invalid.");
  }

  return payload as ExchangeAdminState;
}

function optionalNumber(formData: FormData, key: string) {
  const value = String(formData.get(key) ?? "").trim();
  return value.length > 0 ? Number(value) : null;
}

function describeMarketPriceBounds(market: ExchangeMarket) {
  if (market.min_price !== null && market.max_price !== null) {
    return `price ${market.min_price} to ${market.max_price}`;
  }
  if (market.min_price !== null) {
    return `price floor ${market.min_price}`;
  }
  if (market.max_price !== null) {
    return `price cap ${market.max_price}`;
  }
  return "price unbounded";
}

function buildBotPayload(
  formData: FormData,
  overrides: { botId: string; displayName: string | null },
) {
  return {
    bot_id: overrides.botId,
    display_name: overrides.displayName,
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

function sortMarkets(markets: ExchangeMarket[]) {
  return [...markets];
}

function sortBots(bots: ExchangeAdminBot[]) {
  return [...bots].sort((left, right) => left.bot_id.localeCompare(right.bot_id));
}

export function AdminMarketBotManager({
  initialBots,
  initialMarkets,
}: AdminMarketBotManagerProps) {
  const [markets, setMarkets] = useState(() => sortMarkets(initialMarkets));
  const [bots, setBots] = useState(() => sortBots(initialBots));
  const [isBotRosterExpanded, setIsBotRosterExpanded] = useState(true);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pendingAction, setPendingAction] = useState<string | null>(null);
  const [marketDisplayName, setMarketDisplayName] = useState("");

  const sortedMarkets = useMemo(() => sortMarkets(markets), [markets]);
  const sortedBots = useMemo(() => sortBots(bots), [bots]);
  const isBusy = pendingAction !== null;
  const hasMarkets = sortedMarkets.length > 0;
  const marketIdPreview = deriveCompetitionMarketId(marketDisplayName);

  const syncAdminState = (adminState: ExchangeAdminState) => {
    setMarkets(sortMarkets(adminState.markets));
    setBots(sortBots(adminState.bots));
  };

  const applyMutationResult = (result: AdminMutationResponse) => {
    syncAdminState(result.adminState);
    setNotice(result.notice);
    setError(null);
  };

  const refreshAdminState = async () => {
    const response = await fetch("/api/admin/state", {
      cache: "no-store",
    });
    const adminState = await readAdminState(response);
    syncAdminState(adminState);
  };

  const runMutation = async (
    actionKey: string,
    path: string,
    method: "POST" | "PATCH" | "DELETE",
    body?: unknown,
  ) => {
    setPendingAction(actionKey);
    try {
      const response = await fetch(path, {
        method,
        headers: body === undefined ? undefined : { "content-type": "application/json" },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
      const result = await readMutationResponse(response);
      applyMutationResult(result);
      return result;
    } finally {
      setPendingAction(null);
    }
  };

  const handleCreateMarket = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const form = event.currentTarget;
    const formData = new FormData(form);

    try {
      await runMutation("create-market", "/api/admin/markets", "POST", {
        market_id: deriveCompetitionMarketId(String(formData.get("displayName") ?? "")),
        display_name: String(formData.get("displayName") ?? "").trim(),
        base_asset: deriveCompetitionBaseAsset(String(formData.get("displayName") ?? "")),
        quote_asset: COMPETITION_QUOTE_ASSET,
        tick_size: Number(formData.get("tickSize") ?? 0),
        min_order_quantity: Number(formData.get("minOrderQuantity") ?? 0),
        min: optionalNumber(formData, "minPrice"),
        max: optionalNumber(formData, "maxPrice"),
        reference_price: optionalNumber(formData, "referencePrice"),
        enabled: String(formData.get("enabled") ?? "on") === "on",
      });
      form.reset();
      setMarketDisplayName("");
    } catch (submitError) {
      setError(submitError instanceof Error ? submitError.message : "Failed to save market.");
      setNotice(null);
    }
  };

  const handleSaveBot = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const formData = new FormData(event.currentTarget);

    try {
      await runMutation(
        "save-bot",
        "/api/admin/bots",
        "POST",
        buildBotPayload(formData, {
          botId: String(formData.get("botId") ?? "").trim(),
          displayName: (() => {
            const value = String(formData.get("displayName") ?? "").trim();
            return value.length > 0 ? value : null;
          })(),
        }),
      );
    } catch (submitError) {
      setError(submitError instanceof Error ? submitError.message : "Failed to save bot.");
      setNotice(null);
    }
  };

  const handleSaveBotBatch = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const formData = new FormData(event.currentTarget);

    let definitions;
    try {
      definitions = resolveBulkBotDefinitions({
        botIdPrefix: String(formData.get("botIdPrefix") ?? ""),
        displayNamePrefix: (() => {
          const value = String(formData.get("displayNamePrefix") ?? "").trim();
          return value.length > 0 ? value : null;
        })(),
        count: Number(formData.get("botCount") ?? 0),
        startIndex: Number(formData.get("botStartIndex") ?? 1),
      });
    } catch (definitionError) {
      setError(definitionError instanceof Error ? definitionError.message : "Invalid bot batch.");
      setNotice(null);
      return;
    }

    setPendingAction("save-bot-batch");
    let savedCount = 0;
    let lastBotId: string | null = null;
    let latestState: ExchangeAdminState | null = null;

    try {
      for (const definition of definitions) {
        lastBotId = definition.botId;
        const response = await fetch("/api/admin/bots", {
          method: "POST",
          headers: {
            "content-type": "application/json",
          },
          body: JSON.stringify(buildBotPayload(formData, {
            botId: definition.botId,
            displayName: definition.displayName,
          })),
        });
        const result = await readMutationResponse(response);
        latestState = result.adminState;
        savedCount += 1;
      }

      if (latestState) {
        syncAdminState(latestState);
      }
      const firstBotId = definitions[0]?.botId;
      const lastCreatedBotId = definitions[definitions.length - 1]?.botId;
      setNotice(
        definitions.length === 1
          ? `Saved bot ${firstBotId}.`
          : `Saved ${definitions.length} bots from ${firstBotId} through ${lastCreatedBotId}.`,
      );
      setError(null);
    } catch (submitError) {
      if (savedCount > 0) {
        try {
          await refreshAdminState();
        } catch {
          // Keep the partial-save error even if the refresh fails.
        }
      }
      const message = submitError instanceof Error ? submitError.message : "Failed to save bot batch.";
      setError(
        savedCount > 0 && lastBotId
          ? `Saved ${savedCount} bots before failing on ${lastBotId}. ${message}`
          : message,
      );
      setNotice(null);
    } finally {
      setPendingAction(null);
    }
  };

  const handleStartBot = async (botId?: string) => {
    try {
      await runMutation(
        botId ? `start-${botId}` : "start-all-bots",
        "/api/admin/bots/start",
        "POST",
        botId ? { bot_id: botId } : {},
      );
    } catch (mutationError) {
      setError(mutationError instanceof Error ? mutationError.message : "Failed to start bots.");
      setNotice(null);
    }
  };

  const handlePauseBot = async (botId?: string) => {
    try {
      await runMutation(
        botId ? `pause-${botId}` : "pause-all-bots",
        "/api/admin/bots/pause",
        "POST",
        botId ? { bot_id: botId } : {},
      );
    } catch (mutationError) {
      setError(mutationError instanceof Error ? mutationError.message : "Failed to pause bots.");
      setNotice(null);
    }
  };

  const handleDeleteBot = async (botId?: string) => {
    try {
      await runMutation(
        botId ? `delete-${botId}` : "delete-all-bots",
        "/api/admin/bots",
        "DELETE",
        botId ? { bot_id: botId } : {},
      );
    } catch (mutationError) {
      setError(mutationError instanceof Error ? mutationError.message : "Failed to delete bots.");
      setNotice(null);
    }
  };

  const handleToggleMarket = async (marketId: string, enabled: boolean) => {
    try {
      await runMutation(`toggle-${marketId}`, "/api/admin/markets", "PATCH", {
        market_id: marketId,
        enabled,
      });
    } catch (mutationError) {
      setError(mutationError instanceof Error ? mutationError.message : "Failed to update market.");
      setNotice(null);
    }
  };

  const handleDeleteMarket = async (marketId: string) => {
    try {
      await runMutation(`delete-market-${marketId}`, "/api/admin/markets", "DELETE", {
        market_id: marketId,
      });
    } catch (mutationError) {
      setError(mutationError instanceof Error ? mutationError.message : "Failed to delete market.");
      setNotice(null);
    }
  };

    const handleSettleMarket = async (
    marketId: string,
    event: FormEvent<HTMLFormElement>,
  ) => {
    event.preventDefault();
    const form = event.currentTarget;
    const formData = new FormData(form);

    try {
      await runMutation(`settle-${marketId}`, "/api/admin/markets/settle", "POST", {
        market_id: marketId,
        settlement_price: Number(formData.get("settlementPrice") ?? 0),
        announcement: (() => {
          const value = String(formData.get("announcement") ?? "").trim();
          return value.length > 0 ? value : null;
        })(),
      });
      form.reset();
    } catch (mutationError) {
      setError(mutationError instanceof Error ? mutationError.message : "Failed to settle market.");
      setNotice(null);
    }
  };

  return (
    <div className="grid gap-4">
      {notice ? (
        <p className="ops-note border-[rgba(66,204,78,0.35)] bg-[rgba(66,204,78,0.08)] px-4 py-3 text-base text-[var(--green-strong)]">
          {notice}
        </p>
      ) : null}
      {error ? (
        <p className="ops-note border-[rgba(216,91,91,0.42)] bg-[rgba(216,91,91,0.08)] px-4 py-3 text-base text-[color:var(--red-strong)]">
          {error}
        </p>
      ) : null}

      <div className="grid gap-4 xl:grid-cols-2">
        <section className="ops-panel px-5 py-5">
          <div className="flex flex-wrap items-start justify-between gap-4">
            <div>
              <h2 className="ops-section-title">Trading bots</h2>
              <p className="mt-2 text-base text-[var(--muted-strong)]">
                Save bot configs in place, keep the roster tucked away when you do not need it, and expand individual rows only when you need detail.
              </p>
            </div>
            <div className="flex flex-wrap gap-2">
              <button
                className={primaryButtonClass}
                disabled={isBusy || sortedBots.length === 0}
                onClick={() => void handleStartBot()}
                type="button"
              >
                Start all bots
              </button>
              <button
                className={neutralButtonClass}
                disabled={isBusy || sortedBots.length === 0}
                onClick={() => void handlePauseBot()}
                type="button"
              >
                Pause all bots
              </button>
              <button
                className={dangerButtonClass}
                disabled={isBusy || sortedBots.length === 0}
                onClick={() => void handleDeleteBot()}
                type="button"
              >
                Delete all bots
              </button>
            </div>
          </div>
          <p className="mt-3 text-sm text-[var(--muted)]">
            Maker bots rest limit orders inside the configured price band. Taker bots submit market orders only when the best bid (sells) or best ask (buys) is inside that band; otherwise they wait. They never post resting limits.
          </p>
          <div className="mt-4 grid gap-4">
            <form className="ops-panel-soft grid gap-3 px-4 py-4" onSubmit={(event) => void handleSaveBot(event)}>
              <div>
                <p className="ops-kicker">Single bot</p>
                <p className="mt-2 text-sm text-[var(--muted)]">
                  Save one bot config with an explicit id.
                </p>
              </div>
              <BotConfigurationFields disabled={isBusy} markets={sortedMarkets} mode="single" />
              <button
                className={primaryButtonClass}
                disabled={isBusy || !hasMarkets}
                type="submit"
              >
                Save bot
              </button>
            </form>

            <form className="ops-panel-soft grid gap-3 px-4 py-4" onSubmit={(event) => void handleSaveBotBatch(event)}>
              <div>
                <p className="ops-kicker">Bot batch</p>
                <p className="mt-2 text-sm text-[var(--muted)]">
                  Stamp out a numbered range like <code>depth-maker-1</code> through <code>depth-maker-10</code>. Batch creation stays capped at {MAX_BULK_BOT_COUNT} bots per submit.
                </p>
              </div>
              <BotConfigurationFields disabled={isBusy} markets={sortedMarkets} mode="batch" />
              <button
                className={primaryButtonClass}
                disabled={isBusy || !hasMarkets}
                type="submit"
              >
                Create bot batch
              </button>
            </form>
          </div>

          <div className="mt-5 grid gap-3">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <p className="ops-kicker">
                Bot roster ({sortedBots.length})
              </p>
              <button
                className={neutralButtonClass}
                disabled={sortedBots.length === 0}
                onClick={() => setIsBotRosterExpanded((current) => !current)}
                type="button"
              >
                {isBotRosterExpanded ? "Hide bot roster" : "Show bot roster"}
              </button>
            </div>
            {sortedBots.length === 0 ? (
              <div className={cardClass}>
                No bots configured yet.
              </div>
            ) : isBotRosterExpanded ? (
              sortedBots.map((bot) => (
                <details
                  className="ops-panel-soft overflow-hidden"
                  key={bot.bot_id}
                >
                  <summary className="flex cursor-pointer list-none flex-wrap items-center justify-between gap-3 px-4 py-3">
                    <div className="min-w-0">
                      <div className="flex flex-wrap items-center gap-2">
                        <p className="text-base font-semibold text-white">{bot.display_name}</p>
                        <span className="text-xs text-[var(--muted)]">{bot.bot_id}</span>
                      </div>
                      <p className="mt-1 text-sm text-[var(--muted-strong)]">
                        {bot.market_id} · {bot.strategy} · {bot.side_mode} · qty {bot.min_quantity}-{bot.max_quantity} · {bot.interval_ms} ms
                      </p>
                    </div>
                    <div className="flex items-center gap-2">
                      {bot.last_error ? (
                        <span className="ops-badge border-[rgba(216,91,91,0.42)] bg-[rgba(216,91,91,0.08)] text-[#ffb2b2]">
                          error
                        </span>
                      ) : null}
                      <span className={`ops-badge ${botStatusClass(bot.status)}`}>
                        {bot.status}
                      </span>
                    </div>
                  </summary>
                  <div className="border-t border-[var(--surface-stroke)] px-4 py-4 text-[14px] text-[var(--muted-strong)]">
                    <div className="grid gap-3 md:grid-cols-2">
                      <p>Trader {bot.trader_username}</p>
                      <p>Open order cap {bot.max_open_orders}</p>
                      <p>
                        Strategy {bot.strategy}
                      </p>
                      <p>
                        Price bounds {formatCurrency(bot.min_price)} to {formatCurrency(bot.max_price)}
                      </p>
                      <p>
                        Last submit {bot.last_submitted_at ? formatTimestamp(bot.last_submitted_at) : "Never"}
                      </p>
                      <p>Updated {formatTimestamp(bot.updated_at)}</p>
                    </div>
                    {bot.last_error ? (
                      <p className="mt-3 text-sm text-[#ffb2b2]">
                        {bot.last_error}
                      </p>
                    ) : null}
                    <div className="mt-4 flex flex-wrap gap-3">
                      <button
                        className={primaryButtonClass}
                        disabled={isBusy}
                        onClick={() => void handleStartBot(bot.bot_id)}
                        type="button"
                      >
                        Start
                      </button>
                      <button
                        className={neutralButtonClass}
                        disabled={isBusy}
                        onClick={() => void handlePauseBot(bot.bot_id)}
                        type="button"
                      >
                        Pause
                      </button>
                      <button
                        className={dangerButtonClass}
                        disabled={isBusy}
                        onClick={() => void handleDeleteBot(bot.bot_id)}
                        type="button"
                      >
                        Delete
                      </button>
                    </div>
                  </div>
                </details>
              ))
            ) : (
              <div className={cardClass}>
                {sortedBots.length} {sortedBots.length === 1 ? "bot" : "bots"} hidden.
              </div>
            )}
          </div>
        </section>

        <div className="grid gap-4">
          <section className="ops-panel px-5 py-5">
            <h2 className="ops-section-title">Create market</h2>
            <form className="mt-4 grid gap-3" onSubmit={(event) => void handleCreateMarket(event)}>
              <div className="grid gap-3">
                <AdminField label="Display Name">
                  <input
                    className={inputClass}
                    disabled={isBusy}
                    name="displayName"
                    onChange={(event) => setMarketDisplayName(event.target.value)}
                    placeholder="Bitcoin"
                    required
                    value={marketDisplayName}
                  />
                </AdminField>
              </div>
              <div className="grid gap-3">
                <AdminField label="Market ID">
                  <input
                    className={`${inputClass} bg-black/30`}
                    placeholder="BITCOIN-MARKET"
                    readOnly
                    tabIndex={-1}
                    value={marketIdPreview}
                  />
                </AdminField>
              </div>
              <div className="grid gap-3 md:grid-cols-3">
                <AdminField label="Tick Size">
                  <input
                    className={inputClass}
                    defaultValue="1"
                    disabled={isBusy}
                    min="1"
                    name="tickSize"
                    required
                    type="number"
                  />
                </AdminField>
                <AdminField label="Minimum Order Quantity">
                  <input
                    className={inputClass}
                    defaultValue="1"
                    disabled={isBusy}
                    min="1"
                    name="minOrderQuantity"
                    required
                    type="number"
                  />
                </AdminField>
                <AdminField label="Reference Price">
                  <input
                    className={inputClass}
                    disabled={isBusy}
                    min="0"
                    name="referencePrice"
                    placeholder="100"
                    type="number"
                  />
                </AdminField>
              </div>
              <div className="grid gap-3 md:grid-cols-2">
                <AdminField label="Minimum Allowed Price">
                  <input
                    className={inputClass}
                    disabled={isBusy}
                    min="1"
                    name="minPrice"
                    placeholder="Optional floor"
                    type="number"
                  />
                </AdminField>
                <AdminField label="Maximum Allowed Price">
                  <input
                    className={inputClass}
                    disabled={isBusy}
                    min="1"
                    name="maxPrice"
                    placeholder="Optional cap"
                    type="number"
                  />
                </AdminField>
              </div>
              <div className="grid gap-2">
                <label className="flex items-center gap-3 text-sm text-[var(--muted-strong)]">
                  <input className="ops-check" defaultChecked disabled={isBusy} name="enabled" type="checkbox" />
                  Enable immediately
                </label>
                <p className="text-sm text-[var(--muted-strong)]">
                  Enabled markets can accept orders right away.
                </p>
              </div>
              <button
                className={primaryButtonClass}
                disabled={isBusy}
                type="submit"
              >
                Save market
              </button>
            </form>
          </section>

          <section className="ops-panel px-5 py-5">
            <h2 className="ops-section-title">Markets</h2>
            <div className="mt-4 grid gap-3">
              {sortedMarkets.length === 0 ? (
                <div className={cardClass}>
                  No markets configured yet.
                </div>
              ) : (
                sortedMarkets.map((market) => (
                  <div
                    className="ops-panel-soft grid gap-4 px-4 py-4 text-[15px] text-[var(--muted-strong)]"
                    key={market.market_id}
                  >
                    <div className="flex flex-wrap items-center justify-between gap-3">
                      <div>
                        <p className="text-xl font-bold text-white">{market.display_name}</p>
                        <p>
                          {market.market_id} · tick {market.tick_size} · min qty {market.min_order_quantity} ·{" "}
                          {describeMarketPriceBounds(market)}
                        </p>
                      </div>
                      <span className="ops-badge text-white">
                        {market.status}
                      </span>
                    </div>
                    <div className="flex flex-wrap gap-3">
                      {market.status !== "settled" ? (
                        <button
                          className={neutralButtonClass}
                          disabled={isBusy}
                          onClick={() => void handleToggleMarket(market.market_id, market.status !== "enabled")}
                          type="button"
                        >
                          {market.status === "enabled" ? "Disable" : "Enable"}
                        </button>
                      ) : null}
                      {market.status !== "settled" ? (
                        <form className="flex flex-wrap gap-3" onSubmit={(event) => void handleSettleMarket(market.market_id, event)}>
                          <input
                            className={inputClass}
                            disabled={isBusy}
                            min="0"
                            name="settlementPrice"
                            placeholder="True value per share"
                            required
                            type="number"
                          />
                          <input
                            className={inputClass}
                            disabled={isBusy}
                            name="announcement"
                            placeholder="Optional announcement"
                          />
                          <button
                            className={warningButtonClass}
                            disabled={isBusy}
                            type="submit"
                          >
                            Settle
                          </button>
                        </form>
                      ) : null}
                      <button
                        className={dangerButtonClass}
                        disabled={isBusy}
                        onClick={() => void handleDeleteMarket(market.market_id)}
                        type="button"
                      >
                        Delete
                      </button>
                    </div>
                  </div>
                ))
              )}
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
