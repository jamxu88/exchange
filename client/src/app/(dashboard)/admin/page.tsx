import { cookies } from "next/headers";
import { redirect } from "next/navigation";
import {
  MAX_BULK_BOT_COUNT,
} from "@/app/(dashboard)/admin/bulk-bot-config";
import {
  deleteBotAction,
  deleteMarketAction,
  ensureAdminDeskAction,
  loadConfigAction,
  pauseBotAction,
  resetAllUsersAction,
  saveBotBatchAction,
  saveBotAction,
  sendMessageAction,
  settleMarketAction,
  startBotAction,
  startTradingAction,
  stopTradingAction,
  submitAdminDeskOrderAction,
  toggleMarketAction,
} from "@/app/(dashboard)/admin/actions";
import { CreateMarketForm } from "@/app/(dashboard)/admin/create-market-form";
import { LiveTelemetryPanel } from "@/app/(dashboard)/admin/live-telemetry-panel";
import {
  COMPETITION_QUOTE_ASSET,
  deriveCompetitionMarketId,
} from "@/app/(dashboard)/admin/market-utils";
import {
  getAdminTelemetry,
  ExchangeServerError,
  getAdminLeaderboard,
  getAdminState,
} from "@/lib/exchange-server";
import { readSessionFromCookieValue, SESSION_COOKIE } from "@/lib/auth";

type AdminPageProps = {
  searchParams?: Promise<{
    notice?: string;
    error?: string;
  }>;
};

function formatCurrency(value: number) {
  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    minimumFractionDigits: 0,
    maximumFractionDigits: 0,
  }).format(value);
}

function formatSignedCurrency(value: number) {
  return value > 0 ? `+${formatCurrency(value)}` : value < 0 ? `-${formatCurrency(Math.abs(value))}` : formatCurrency(0);
}

function formatTimestamp(value: string) {
  return new Date(value).toLocaleString("en-US", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function toneClass(level: "info" | "warning" | "critical") {
  if (level === "critical") {
    return "text-[color:var(--red-strong)]";
  }
  if (level === "warning") {
    return "text-[color:var(--amber)]";
  }
  return "text-[var(--green)]";
}

function botStatusClass(status: "paused" | "running") {
  return status === "running"
    ? "border-[#2f6b37] bg-[#102015] text-[#b8ffbd]"
    : "border-[var(--surface-stroke)] bg-[var(--surface-soft)] text-[var(--text-primary)]";
}

function formatPositionLimit(value: number | null) {
  if (value === null) {
    return "Unlimited";
  }

  return `${value.toLocaleString("en-US")} shares`;
}

const primaryButtonClass = "ops-button ops-button-primary";
const neutralButtonClass = "ops-button ops-button-neutral";
const warningButtonClass = "ops-button ops-button-warning";
const dangerButtonClass = "ops-button ops-button-danger";
const inputClass = "ops-input";
const selectClass = "ops-select";
const textareaClass = "ops-textarea";
const cardClass = "ops-panel-soft px-4 py-4 text-[15px] text-[var(--muted-strong)]";

type BotFormMarket = {
  market_id: string;
};

type BotConfigurationFieldsProps = {
  markets: BotFormMarket[];
  mode: "single" | "batch";
};

function BotConfigurationFields({ markets, mode }: BotConfigurationFieldsProps) {
  const isBatch = mode === "batch";

  return (
    <>
      {isBatch ? (
        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
          <input
            className={inputClass}
            name="botIdPrefix"
            placeholder="Bot ID prefix, for example depth-maker"
            required
          />
          <input
            className={inputClass}
            name="displayNamePrefix"
            placeholder="Display name prefix"
          />
          <input
            className={inputClass}
            defaultValue="10"
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
            name="botId"
            placeholder="Bot ID, for example depth-maker-1"
            required
          />
          <input
            className={inputClass}
            name="displayName"
            placeholder="Display name"
          />
        </div>
      )}
      <div className="grid gap-3 md:grid-cols-3">
        <select
          className={selectClass}
          defaultValue=""
          name="marketId"
          required
        >
          <option disabled value="">
            Select market
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
          name="orderType"
          required
        >
          <option disabled value="">
            Select order type
          </option>
          <option value="limit">Limit</option>
          <option value="market">Market</option>
        </select>
      </div>
      <div className="grid gap-3 md:grid-cols-3">
        <input
          className={inputClass}
          min="1"
          name="minQuantity"
          placeholder="Min qty"
          required
          type="number"
        />
        <input
          className={inputClass}
          min="1"
          name="maxQuantity"
          placeholder="Max qty"
          required
          type="number"
        />
        <input
          className={inputClass}
          min="100"
          name="intervalMs"
          placeholder="Interval ms"
          required
          type="number"
        />
      </div>
      <div className="grid gap-3 md:grid-cols-4">
        <input
          className={inputClass}
          min="1"
          name="maxOpenOrders"
          placeholder="Open order cap"
          required
          type="number"
        />
        <input
          className={inputClass}
          min="0"
          name="priceOffsetTicks"
          placeholder="Offset ticks"
          required
          type="number"
        />
        <input
          className={inputClass}
          min="0"
          name="walkStepTicks"
          placeholder="Walk ticks"
          required
          type="number"
        />
        <input
          className={inputClass}
          min="0"
          name="fallbackPrice"
          placeholder="Fallback price"
          type="number"
        />
      </div>
      <label className="flex items-center gap-3 text-sm text-[var(--muted-strong)]">
        <input className="ops-check" name="startImmediately" type="checkbox" />
        Start immediately after saving
      </label>
    </>
  );
}

export default async function AdminPage({ searchParams }: AdminPageProps) {
  const cookieStore = await cookies();
  const session = readSessionFromCookieValue(cookieStore.get(SESSION_COOKIE)?.value);

  if (!session) {
    redirect("/login");
  }

  let adminState;
  let leaderboard;
  let initialTelemetry = null;
  try {
    [adminState, leaderboard, initialTelemetry] = await Promise.all([
      getAdminState(session.apiKey),
      getAdminLeaderboard(session.apiKey, 10),
      getAdminTelemetry(session.apiKey).catch(() => null),
    ]);
  } catch (error) {
    if (error instanceof ExchangeServerError && error.status === 401) {
      redirect("/trade");
    }
    throw error;
  }

  const resolvedSearchParams = searchParams ? await searchParams : undefined;
  const notice = resolvedSearchParams?.notice;
  const error = resolvedSearchParams?.error;
  const adminDesk = adminState.admin_desk ?? null;
  const bots = adminState.bots ?? [];
  const recentMessages = adminState.recent_messages ?? [];

  return (
    <main className="mx-auto flex min-h-screen w-full max-w-[1600px] flex-col gap-4 px-5 py-5 lg:px-8">
      <section className="ops-panel flex items-start justify-between gap-4 px-4 py-4 lg:px-5">
        <div>
          <p className="ops-kicker">
            Admin Panel
          </p>
        </div>
        <form action="/api/auth/logout" className="shrink-0" method="post">
          <button
            className={neutralButtonClass}
            type="submit"
          >
            Log out
          </button>
        </form>
      </section>

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

      <div className="grid gap-4 xl:grid-cols-[1.2fr_0.8fr]">
        <section className="ops-panel px-5 py-5">
          <div className="flex flex-wrap items-center justify-between gap-4">
            <div>
              <h2 className="ops-section-title">Exchange controls</h2>
              <p className="mt-2 text-base text-[var(--muted-strong)]">
                Trading is currently{" "}
                <span className="font-semibold text-[var(--text-primary)]">
                  {adminState.controls.trading_enabled ? "enabled" : "stopped"}
                </span>
                .
              </p>
            </div>
            <div className="flex flex-wrap gap-2">
              <form action={startTradingAction}>
                <button
                  className={primaryButtonClass}
                  type="submit"
                >
                  Start trading
                </button>
              </form>
              <form action={stopTradingAction}>
                <button
                  className={dangerButtonClass}
                  type="submit"
                >
                  Stop trading
                </button>
              </form>
              <form action={resetAllUsersAction}>
                <button
                  className={warningButtonClass}
                  type="submit"
                >
                  Reset all users
                </button>
              </form>
            </div>
          </div>
          <div className="mt-4 grid gap-3 md:grid-cols-3 text-base text-[var(--muted-strong)]">
            <div className="ops-panel-soft flex items-center justify-between px-4 py-4">
              <span className="ops-kicker">Queue depth</span>
              <span className="text-xl font-semibold text-[var(--text-primary)]">
                {adminState.persistence.queue_depth}
              </span>
            </div>
            <div className="ops-panel-soft flex items-center justify-between px-4 py-4">
              <span className="ops-kicker">Last flush</span>
              <span className="text-xl font-semibold text-[var(--text-primary)]">
                {adminState.persistence.last_flush_latency_ms} ms
              </span>
            </div>
            <div className="ops-panel-soft flex items-center justify-between px-4 py-4">
              <span className="ops-kicker">Tracked markets</span>
              <span className="text-xl font-semibold text-[var(--text-primary)]">
                {adminState.markets.length}
              </span>
            </div>
          </div>
          <LiveTelemetryPanel initialTelemetry={initialTelemetry} />
        </section>

        <section className="ops-panel px-5 py-5">
          <h2 className="ops-section-title">Send message</h2>
          <form action={sendMessageAction} className="mt-4 grid gap-3">
            <input
              className={inputClass}
              name="title"
              placeholder="Optional title"
            />
            <div className="grid gap-3 md:grid-cols-4">
              <select
                className={selectClass}
                defaultValue="single"
                name="audience"
              >
                <option value="single">One user</option>
                <option value="list">User list</option>
                <option value="all">All users</option>
              </select>
              <input
                className={inputClass}
                name="targetUsername"
                placeholder="Single username"
              />
              <input
                className={inputClass}
                name="market"
                placeholder="Market"
              />
              <select
                className={selectClass}
                defaultValue="info"
                name="level"
              >
                <option value="info">Info</option>
                <option value="warning">Warning</option>
                <option value="critical">Critical</option>
              </select>
            </div>
            <textarea
              className={`${textareaClass} min-h-20`}
              name="targetUsernames"
              placeholder="User list: alice, bob, carol"
            />
            <p className="text-sm text-[var(--muted)]">
              Choose one user, all users, or paste a list separated by commas, spaces, or new lines.
            </p>
            <textarea
              className={`${textareaClass} min-h-28`}
              name="body"
              placeholder="Broadcast or targeted message"
              required
            />
            <button
              className={primaryButtonClass}
              type="submit"
            >
              Send message
            </button>
          </form>
        </section>
      </div>

      <div className="grid gap-4 xl:grid-cols-[0.95fr_1.05fr]">
        <section className="ops-panel px-5 py-5">
          <div className="flex flex-wrap items-start justify-between gap-4">
            <div>
              <h2 className="ops-section-title">Admin desk</h2>
              <p className="mt-2 text-base text-[var(--muted-strong)]">
                Submit live orders through a hidden admin-role trader with no position limit.
              </p>
            </div>
            <form action={ensureAdminDeskAction}>
              <button
                className={neutralButtonClass}
                type="submit"
              >
                {adminDesk ? "Refresh desk" : "Provision desk"}
              </button>
            </form>
          </div>
          <div className="mt-4 grid gap-3 md:grid-cols-3 text-[15px] text-[var(--muted-strong)]">
            <div className="ops-panel-soft px-4 py-4">
              <p className="ops-kicker">Trader</p>
              <p className="mt-2 text-xl font-bold text-white">
                {adminDesk?.username ?? "Not provisioned"}
              </p>
            </div>
            <div className="ops-panel-soft px-4 py-4">
              <p className="ops-kicker">Position limit</p>
              <p className="mt-2 text-xl font-bold text-white">
                {formatPositionLimit(adminDesk?.position_limit ?? null)}
              </p>
            </div>
            <div className="ops-panel-soft px-4 py-4">
              <p className="ops-kicker">Created</p>
              <p className="mt-2 text-xl font-bold text-white">
                {adminDesk
                  ? formatTimestamp(adminDesk.created_at)
                  : "On first use"}
              </p>
            </div>
          </div>
          <form action={submitAdminDeskOrderAction} className="mt-4 grid gap-3">
            <div className="grid gap-3 md:grid-cols-2">
              <select
                className={selectClass}
                defaultValue={adminState.markets[0]?.market_id}
                name="marketId"
              >
                {adminState.markets.map((market) => (
                  <option key={market.market_id} value={market.market_id}>
                    {market.display_name} ({market.market_id})
                  </option>
                ))}
              </select>
              <div className="grid gap-3 md:grid-cols-2">
                <select
                  className={selectClass}
                  defaultValue="BUY"
                  name="side"
                >
                  <option value="BUY">Buy</option>
                  <option value="SELL">Sell</option>
                </select>
                <select
                  className={selectClass}
                  defaultValue="limit"
                  name="orderType"
                >
                  <option value="limit">Limit</option>
                  <option value="market">Market</option>
                </select>
              </div>
            </div>
            <div className="grid gap-3 md:grid-cols-2">
              <input
                className={inputClass}
                min="1"
                name="quantity"
                placeholder="Shares"
                required
                type="number"
              />
              <input
                className={inputClass}
                min="0"
                name="price"
                placeholder="Limit price. Ignored for market orders."
                type="number"
              />
            </div>
            <button
              className={primaryButtonClass}
              type="submit"
            >
              Submit admin order
            </button>
          </form>
        </section>

        <section className="ops-panel px-5 py-5">
          <h2 className="ops-section-title">Trading bots</h2>
          <p className="mt-2 text-base text-[var(--muted-strong)]">
            Save a bot config, launch it immediately, then pause or restart it from the roster.
          </p>
          <p className="mt-3 max-w-4xl text-sm text-[var(--muted)]">
            Open order cap is the maximum number of resting orders the bot may keep live at once.
            Offset ticks place each limit order away from the current anchor price. Walk ticks set
            how far that anchor can drift on each cycle. Fallback price is used only when the book
            is empty and the market has no usable reference price.
          </p>
          <div className="mt-4 grid gap-4 2xl:grid-cols-2">
            <form action={saveBotAction} className="ops-panel-soft grid gap-3 px-4 py-4">
              <div>
                <p className="ops-kicker">Single bot</p>
                <p className="mt-2 text-sm text-[var(--muted)]">
                  Save one bot config with an explicit id.
                </p>
              </div>
              <BotConfigurationFields markets={adminState.markets} mode="single" />
              <button
                className={primaryButtonClass}
                type="submit"
              >
                Save bot
              </button>
            </form>

            <form action={saveBotBatchAction} className="ops-panel-soft grid gap-3 px-4 py-4">
              <div>
                <p className="ops-kicker">Bot batch</p>
                <p className="mt-2 text-sm text-[var(--muted)]">
                  Stamp out a numbered range like <code>depth-maker-1</code> through{" "}
                  <code>depth-maker-10</code>. Batch creation is capped at {MAX_BULK_BOT_COUNT} bots
                  per submit.
                </p>
              </div>
              <BotConfigurationFields markets={adminState.markets} mode="batch" />
              <button
                className={primaryButtonClass}
                type="submit"
              >
                Create bot batch
              </button>
            </form>
          </div>

          <div className="mt-5 grid gap-3">
            {bots.length === 0 ? (
              <div className={cardClass}>
                No bots configured yet.
              </div>
            ) : (
              bots.map((bot) => (
                <div
                  className="ops-panel-soft grid gap-4 px-4 py-4 text-[15px] text-[var(--muted-strong)]"
                  key={bot.bot_id}
                >
                  <div className="flex flex-wrap items-center justify-between gap-3">
                    <div>
                      <p className="text-xl font-bold text-white">{bot.display_name}</p>
                      <p>
                        {bot.bot_id} · {bot.market_id} · {bot.trader_username}
                      </p>
                    </div>
                    <span className={`ops-badge ${botStatusClass(bot.status)}`}>
                      {bot.status}
                    </span>
                  </div>
                  <p>
                    {bot.side_mode} · {bot.order_type} · qty {bot.min_quantity} to {bot.max_quantity}
                    {" · "}interval {bot.interval_ms} ms · cap {bot.max_open_orders} open
                    {" · "}offset {bot.price_offset_ticks} ticks · walk {bot.walk_step_ticks} ticks
                    {bot.fallback_price !== null ? ` · fallback ${formatCurrency(bot.fallback_price)}` : ""}
                  </p>
                  <p>
                    Last submit: {bot.last_submitted_at ? formatTimestamp(bot.last_submitted_at) : "Never"}
                    {bot.last_error ? ` · Error: ${bot.last_error}` : ""}
                  </p>
                  <div className="flex flex-wrap gap-3">
                    <form action={startBotAction}>
                      <input name="botId" type="hidden" value={bot.bot_id} />
                      <button
                        className={primaryButtonClass}
                        type="submit"
                      >
                        Start
                      </button>
                    </form>
                    <form action={pauseBotAction}>
                      <input name="botId" type="hidden" value={bot.bot_id} />
                      <button
                        className={neutralButtonClass}
                        type="submit"
                      >
                        Pause
                      </button>
                    </form>
                    <form action={deleteBotAction}>
                      <input name="botId" type="hidden" value={bot.bot_id} />
                      <button
                        className={dangerButtonClass}
                        type="submit"
                      >
                        Delete
                      </button>
                    </form>
                  </div>
                </div>
              ))
            )}
          </div>
        </section>
      </div>

      <div className="grid gap-4 xl:grid-cols-[1.1fr_0.9fr]">
        <section className="ops-panel px-5 py-5">
          <h2 className="ops-section-title">Create market</h2>
          <CreateMarketForm />
        </section>

        <section className="ops-panel px-5 py-5">
          <h2 className="ops-section-title">Load config</h2>
          <form action={loadConfigAction} className="mt-4 grid gap-3">
            <textarea
              className={`${textareaClass} min-h-56 font-mono text-sm`}
              defaultValue={JSON.stringify(
                {
                  trading_enabled: adminState.controls.trading_enabled,
                  markets: adminState.markets.map((market) => {
                    const defaultMarketId = deriveCompetitionMarketId(market.display_name);

                    return {
                      ...(market.market_id !== defaultMarketId
                        ? { market_id: market.market_id }
                        : {}),
                      display_name: market.display_name,
                      ...(market.quote_asset !== COMPETITION_QUOTE_ASSET
                        ? { quote_asset: market.quote_asset }
                        : {}),
                      tick_size: market.tick_size,
                      min_order_quantity: market.min_order_quantity,
                      reference_price: market.reference_price,
                      enabled: market.status === "enabled",
                    };
                  }),
                },
                null,
                2,
              )}
              name="config"
            />
            <button
              className={neutralButtonClass}
              type="submit"
            >
              Apply config JSON
            </button>
          </form>
        </section>
      </div>

      <div className="grid gap-4 xl:grid-cols-[1.2fr_0.8fr]">
        <section className="ops-panel px-5 py-5">
          <h2 className="ops-section-title">Markets</h2>
          <div className="mt-4 grid gap-3">
            {adminState.markets.map((market) => (
              <div
                className="ops-panel-soft grid gap-4 px-4 py-4 text-[15px] text-[var(--muted-strong)]"
                key={market.market_id}
              >
                <div className="flex flex-wrap items-center justify-between gap-3">
                  <div>
                    <p className="text-xl font-bold text-white">{market.display_name}</p>
                    <p>
                      {market.market_id} · tick {market.tick_size} · min qty{" "}
                      {market.min_order_quantity}
                    </p>
                  </div>
                  <span className="ops-badge text-white">
                    {market.status}
                  </span>
                </div>
                <div className="flex flex-wrap gap-3">
                  {market.status !== "settled" ? (
                    <form action={toggleMarketAction}>
                      <input name="marketId" type="hidden" value={market.market_id} />
                      <input
                        name="enable"
                        type="hidden"
                        value={String(market.status !== "enabled")}
                      />
                      <button
                        className={neutralButtonClass}
                        type="submit"
                      >
                        {market.status === "enabled" ? "Disable" : "Enable"}
                      </button>
                    </form>
                  ) : null}
                  {market.status !== "settled" ? (
                    <form action={settleMarketAction} className="flex flex-wrap gap-3">
                      <input name="marketId" type="hidden" value={market.market_id} />
                      <input
                        className={inputClass}
                        min="1"
                        name="settlementPrice"
                        placeholder="True value per share"
                        required
                        type="number"
                      />
                      <input
                        className={inputClass}
                        name="announcement"
                        placeholder="Optional announcement"
                      />
                      <button
                        className={warningButtonClass}
                        type="submit"
                      >
                        Settle
                      </button>
                    </form>
                  ) : null}
                  <form action={deleteMarketAction}>
                    <input name="marketId" type="hidden" value={market.market_id} />
                    <button
                      className={dangerButtonClass}
                      type="submit"
                    >
                      Delete
                    </button>
                  </form>
                </div>
              </div>
            ))}
          </div>
        </section>

        <section className="ops-panel px-5 py-5">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div>
              <h2 className="ops-section-title">Leaderboard</h2>
              <p className="mt-2 text-base text-[var(--muted-strong)]">
                Top 10 shown here. Export downloads the full leaderboard as CSV.
              </p>
            </div>
            <a
              className={neutralButtonClass}
              href="/admin/leaderboard/export"
            >
              Export CSV
            </a>
          </div>
          <div className="mt-4 grid gap-3">
            {leaderboard.map((row) => (
              <div
                className="ops-panel-soft flex items-center justify-between gap-4 px-4 py-4 text-[15px] text-[var(--muted-strong)]"
                key={row.trader_id}
              >
                <div>
                  <p className="text-lg font-bold text-white">
                    #{row.rank} {row.username}
                  </p>
                  <p>
                    Realized {formatSignedCurrency(row.realized_pnl)} · Unrealized{" "}
                    {formatSignedCurrency(row.unrealized_pnl)} · Exposure{" "}
                    {formatCurrency(row.gross_exposure)}
                  </p>
                </div>
                <p className="text-lg font-semibold text-white">
                  {formatSignedCurrency(row.net_pnl)}
                </p>
              </div>
            ))}
          </div>
        </section>
      </div>

      <section className="ops-panel px-5 py-5">
        <h2 className="ops-section-title">Recent messages</h2>
        <div className="mt-4 grid gap-3">
          {recentMessages.length === 0 ? (
            <div className={cardClass}>
              No admin messages have been sent yet.
            </div>
          ) : (
            recentMessages.map((message) => (
              <div
                className="ops-panel-soft px-4 py-4 text-[15px] text-[var(--muted-strong)]"
                key={message.message_id}
              >
                <div className="flex flex-wrap items-center justify-between gap-3">
                  <p className={`ops-kicker ${toneClass(message.level)}`}>
                    {message.level}
                  </p>
                  <p>{formatTimestamp(message.created_at)}</p>
                </div>
                {message.title ? (
                  <p className="mt-2 text-lg font-bold text-white">{message.title}</p>
                ) : null}
                <p className="mt-2">{message.body}</p>
                <p className="mt-2 text-sm">
                  target {message.target_username ?? "broadcast"} · market{" "}
                  {message.market ?? "all"}
                </p>
              </div>
            ))
          )}
        </div>
      </section>
    </main>
  );
}
