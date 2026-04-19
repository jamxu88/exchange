import { cookies } from "next/headers";
import { redirect } from "next/navigation";
import {
  ensureAdminDeskAction,
  loadConfigAction,
  resetAllUsersAction,
  sendMessageAction,
  startTradingAction,
  stopTradingAction,
  submitAdminDeskOrderAction,
} from "@/app/(dashboard)/admin/actions";
import { AdminMarketBotManager } from "@/app/(dashboard)/admin/admin-market-bot-manager";
import { CardDealBroadcaster } from "@/app/(dashboard)/admin/card-deal-broadcaster";
import { CompetitionMessagePresets } from "@/app/(dashboard)/admin/competition-message-presets";
import { LiveTelemetryPanel } from "@/app/(dashboard)/admin/live-telemetry-panel";
import {
  COMPETITION_QUOTE_ASSET,
  deriveCompetitionMarketId,
} from "@/app/(dashboard)/admin/market-utils";
import {
  cardClass,
  formatCurrency,
  formatPositionLimit,
  formatSignedCurrency,
  formatTimestamp,
  inputClass,
  neutralButtonClass,
  primaryButtonClass,
  selectClass,
  textareaClass,
  toneClass,
  warningButtonClass,
  dangerButtonClass,
} from "@/app/(dashboard)/admin/ui";
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
          <h2 className="ops-section-title">Deal cards to teams</h2>
          <p className="mt-2 text-sm text-[var(--muted)]">
            Enter the 10 drawn cards for this round, then broadcast a private 3-card subset to every team. Each team receives 3 positions chosen at random from positions 1, 2, 4, 5, 7, 8, 10.
          </p>
          <div className="mt-4">
            <CardDealBroadcaster />
          </div>
        </section>

        <section className="ops-panel px-5 py-5">
          <h2 className="ops-section-title">Competition messages</h2>
          <p className="mt-2 text-sm text-[var(--muted)]">
            One-click broadcasts for the 5-round card-draw case. Tabs are per round; edit placeholders inline then send to all teams.
          </p>
          <div className="mt-4">
            <CompetitionMessagePresets />
          </div>
        </section>

        <section className="ops-panel px-5 py-5">
          <h2 className="ops-section-title">Send custom message</h2>
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
      </div>

      <AdminMarketBotManager initialBots={bots} initialMarkets={adminState.markets} />

      <div className="grid gap-4 xl:grid-cols-[1.1fr_0.9fr]">
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
                      ...(market.min_price !== null ? { min: market.min_price } : {}),
                      ...(market.max_price !== null ? { max: market.max_price } : {}),
                      reference_price: market.reference_price,
                      enabled: market.status === "enabled",
                    };
                  }),
                  bots: adminState.bots.map((bot) => ({
                    bot_id: bot.bot_id,
                    ...(bot.display_name !== bot.bot_id
                      ? { display_name: bot.display_name }
                      : {}),
                    market_id: bot.market_id,
                    strategy: bot.strategy,
                    side_mode: bot.side_mode,
                    min_quantity: bot.min_quantity,
                    max_quantity: bot.max_quantity,
                    interval_ms: bot.interval_ms,
                    max_open_orders: bot.max_open_orders,
                    min_price: bot.min_price,
                    max_price: bot.max_price,
                    ...(bot.status === "running"
                      ? { start_immediately: true }
                      : {}),
                  })),
                },
                null,
                2,
              )}
              name="config"
            />
            <p className="text-sm text-[var(--muted)]">
              Config JSON can now include both <code>markets</code> and <code>bots</code>.
            </p>
            <button
              className={neutralButtonClass}
              type="submit"
            >
              Apply config JSON
            </button>
          </form>
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
                    #{row.rank} {row.team_number}
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
