import { cookies } from "next/headers";
import { redirect } from "next/navigation";
import {
  createMarketAction,
  deleteMarketAction,
  loadConfigAction,
  sendMessageAction,
  settleMarketAction,
  startTradingAction,
  stopTradingAction,
  toggleMarketAction,
} from "@/app/(dashboard)/admin/actions";
import {
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
    return "text-[#ffb2b2]";
  }
  if (level === "warning") {
    return "text-[#ffd37a]";
  }
  return "text-[var(--green)]";
}

export default async function AdminPage({ searchParams }: AdminPageProps) {
  const cookieStore = await cookies();
  const session = readSessionFromCookieValue(cookieStore.get(SESSION_COOKIE)?.value);

  if (!session) {
    redirect("/login");
  }

  let adminState;
  let leaderboard;
  try {
    [adminState, leaderboard] = await Promise.all([
      getAdminState(session.apiKey),
      getAdminLeaderboard(session.apiKey, 10),
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

  return (
    <main className="mx-auto flex min-h-screen w-full max-w-7xl flex-col gap-6 px-6 py-8 lg:px-10">
      <section className="surface-panel flex flex-col gap-6 px-6 py-6 lg:flex-row lg:items-end lg:justify-between lg:px-8">
        <div>
          <p className="text-sm font-semibold uppercase tracking-[0.28em] text-[var(--muted)]">
            Admin
          </p>
          <h1 className="mt-3 text-5xl font-extrabold leading-none text-white">
            Event operations panel
          </h1>
          <p className="mt-4 max-w-3xl text-2xl text-[var(--muted-strong)]">
            Live exchange controls, market lifecycle, settlement, messaging,
            and leaderboard state from the backend control plane.
          </p>
        </div>
        <div className="surface-panel-soft flex gap-6 px-5 py-4 text-xl text-[var(--muted-strong)]">
          <div>
            <p className="text-3xl font-bold text-white">
              {adminState.controls.trading_enabled ? "Live" : "Stopped"}
            </p>
            <p>Trading state</p>
          </div>
          <div>
            <p className="text-3xl font-bold text-white">
              {adminState.persistence.mode}
            </p>
            <p>Writer mode</p>
          </div>
          <div>
            <p className="text-3xl font-bold text-white">{session.apiKeyPreview}</p>
            <p>Signed-in key</p>
          </div>
          <form action="/api/auth/logout" className="flex items-start" method="post">
            <button
              className="rounded-2xl border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-4 py-3 text-base font-semibold text-white hover:border-[rgba(66,204,78,0.45)]"
              type="submit"
            >
              Log out
            </button>
          </form>
        </div>
      </section>

      {notice ? (
        <p className="rounded-2xl border border-[rgba(66,204,78,0.35)] bg-[rgba(66,204,78,0.1)] px-4 py-3 text-lg text-[#b8ffbd]">
          {notice}
        </p>
      ) : null}
      {error ? (
        <p className="rounded-2xl border border-[rgba(216,91,91,0.42)] bg-[rgba(216,91,91,0.1)] px-4 py-3 text-lg text-[#ffb2b2]">
          {error}
        </p>
      ) : null}

      <div className="grid gap-4 xl:grid-cols-[1.2fr_0.8fr]">
        <section className="surface-panel px-6 py-6">
          <div className="flex items-center justify-between gap-4">
            <div>
              <h2 className="surface-title">Exchange controls</h2>
              <p className="mt-2 text-lg text-[var(--muted-strong)]">
                Trading is currently{" "}
                <span className="font-semibold text-white">
                  {adminState.controls.trading_enabled ? "enabled" : "stopped"}
                </span>
                .
              </p>
            </div>
            <div className="flex gap-3">
              <form action={startTradingAction}>
                <button
                  className="rounded-2xl bg-[var(--green)] px-4 py-3 text-base font-semibold text-white"
                  type="submit"
                >
                  Start trading
                </button>
              </form>
              <form action={stopTradingAction}>
                <button
                  className="rounded-2xl border border-[rgba(216,91,91,0.42)] bg-[rgba(216,91,91,0.12)] px-4 py-3 text-base font-semibold text-white"
                  type="submit"
                >
                  Stop trading
                </button>
              </form>
            </div>
          </div>
          <div className="mt-5 grid gap-3 md:grid-cols-3 text-xl text-[var(--muted-strong)]">
            <div className="surface-panel-soft flex items-center justify-between px-4 py-4">
              <span>Queue depth</span>
              <span className="font-semibold text-white">
                {adminState.persistence.queue_depth}
              </span>
            </div>
            <div className="surface-panel-soft flex items-center justify-between px-4 py-4">
              <span>Last flush</span>
              <span className="font-semibold text-white">
                {adminState.persistence.last_flush_latency_ms} ms
              </span>
            </div>
            <div className="surface-panel-soft flex items-center justify-between px-4 py-4">
              <span>Tracked markets</span>
              <span className="font-semibold text-white">
                {adminState.markets.length}
              </span>
            </div>
          </div>
        </section>

        <section className="surface-panel px-6 py-6">
          <h2 className="surface-title">Send message</h2>
          <form action={sendMessageAction} className="mt-5 grid gap-3">
            <input
              className="rounded-2xl border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-4 py-3 text-lg text-white outline-none"
              name="title"
              placeholder="Optional title"
            />
            <div className="grid gap-3 md:grid-cols-3">
              <input
                className="rounded-2xl border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-4 py-3 text-lg text-white outline-none"
                name="targetUsername"
                placeholder="Target username"
              />
              <input
                className="rounded-2xl border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-4 py-3 text-lg text-white outline-none"
                name="market"
                placeholder="Market"
              />
              <select
                className="rounded-2xl border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-4 py-3 text-lg text-white outline-none"
                defaultValue="info"
                name="level"
              >
                <option value="info">Info</option>
                <option value="warning">Warning</option>
                <option value="critical">Critical</option>
              </select>
            </div>
            <textarea
              className="min-h-28 rounded-2xl border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-4 py-3 text-lg text-white outline-none"
              name="body"
              placeholder="Broadcast or user-specific message"
              required
            />
            <button
              className="rounded-2xl bg-[var(--green)] px-4 py-3 text-base font-semibold text-white"
              type="submit"
            >
              Send message
            </button>
          </form>
        </section>
      </div>

      <div className="grid gap-4 xl:grid-cols-[1.1fr_0.9fr]">
        <section className="surface-panel px-6 py-6">
          <h2 className="surface-title">Create market</h2>
          <form action={createMarketAction} className="mt-5 grid gap-3">
            <div className="grid gap-3 md:grid-cols-2">
              <input
                className="rounded-2xl border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-4 py-3 text-lg text-white outline-none"
                name="marketId"
                placeholder="BTC-USD"
                required
              />
              <input
                className="rounded-2xl border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-4 py-3 text-lg text-white outline-none"
                name="displayName"
                placeholder="Bitcoin"
              />
            </div>
            <div className="grid gap-3 md:grid-cols-2">
              <input
                className="rounded-2xl border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-4 py-3 text-lg text-white outline-none"
                name="baseAsset"
                placeholder="BTC"
                required
              />
              <input
                className="rounded-2xl border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-4 py-3 text-lg text-white outline-none"
                name="quoteAsset"
                placeholder="USD"
                required
              />
            </div>
            <div className="grid gap-3 md:grid-cols-3">
              <input
                className="rounded-2xl border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-4 py-3 text-lg text-white outline-none"
                defaultValue="1"
                min="1"
                name="tickSize"
                required
                type="number"
              />
              <input
                className="rounded-2xl border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-4 py-3 text-lg text-white outline-none"
                defaultValue="1"
                min="1"
                name="minOrderQuantity"
                required
                type="number"
              />
              <input
                className="rounded-2xl border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-4 py-3 text-lg text-white outline-none"
                min="0"
                name="referencePrice"
                placeholder="Reference price"
                type="number"
              />
            </div>
            <label className="flex items-center gap-3 text-lg text-[var(--muted-strong)]">
              <input defaultChecked name="enabled" type="checkbox" />
              Enable immediately
            </label>
            <button
              className="rounded-2xl bg-[var(--green)] px-4 py-3 text-base font-semibold text-white"
              type="submit"
            >
              Save market
            </button>
          </form>
        </section>

        <section className="surface-panel px-6 py-6">
          <h2 className="surface-title">Load config</h2>
          <form action={loadConfigAction} className="mt-5 grid gap-3">
            <textarea
              className="min-h-56 rounded-2xl border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-4 py-3 font-mono text-sm text-white outline-none"
              defaultValue={JSON.stringify(
                {
                  trading_enabled: adminState.controls.trading_enabled,
                  markets: adminState.markets.map((market) => ({
                    market_id: market.market_id,
                    display_name: market.display_name,
                    base_asset: market.base_asset,
                    quote_asset: market.quote_asset,
                    tick_size: market.tick_size,
                    min_order_quantity: market.min_order_quantity,
                    reference_price: market.reference_price,
                    enabled: market.status === "enabled",
                  })),
                },
                null,
                2,
              )}
              name="config"
            />
            <button
              className="rounded-2xl border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-4 py-3 text-base font-semibold text-white"
              type="submit"
            >
              Apply config JSON
            </button>
          </form>
        </section>
      </div>

      <div className="grid gap-4 xl:grid-cols-[1.2fr_0.8fr]">
        <section className="surface-panel px-6 py-6">
          <h2 className="surface-title">Markets</h2>
          <div className="mt-5 grid gap-3">
            {adminState.markets.map((market) => (
              <div
                className="surface-panel-soft grid gap-4 px-4 py-4 text-lg text-[var(--muted-strong)]"
                key={market.market_id}
              >
                <div className="flex flex-wrap items-center justify-between gap-3">
                  <div>
                    <p className="text-2xl font-bold text-white">{market.display_name}</p>
                    <p>
                      {market.market_id} · tick {market.tick_size} · min qty{" "}
                      {market.min_order_quantity}
                    </p>
                  </div>
                  <span className="rounded-full border border-[var(--surface-stroke)] px-3 py-1 text-sm uppercase tracking-[0.2em] text-white">
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
                        className="rounded-2xl border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-4 py-2 text-base font-semibold text-white"
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
                        className="rounded-2xl border border-[var(--surface-stroke)] bg-black/20 px-4 py-2 text-base text-white outline-none"
                        min="1"
                        name="settlementPrice"
                        placeholder="Settlement price"
                        required
                        type="number"
                      />
                      <input
                        className="rounded-2xl border border-[var(--surface-stroke)] bg-black/20 px-4 py-2 text-base text-white outline-none"
                        name="announcement"
                        placeholder="Optional announcement"
                      />
                      <button
                        className="rounded-2xl border border-[rgba(255,211,122,0.35)] bg-[rgba(255,211,122,0.12)] px-4 py-2 text-base font-semibold text-white"
                        type="submit"
                      >
                        Settle
                      </button>
                    </form>
                  ) : null}
                  <form action={deleteMarketAction}>
                    <input name="marketId" type="hidden" value={market.market_id} />
                    <button
                      className="rounded-2xl border border-[rgba(216,91,91,0.42)] bg-[rgba(216,91,91,0.12)] px-4 py-2 text-base font-semibold text-white"
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

        <section className="surface-panel px-6 py-6">
          <h2 className="surface-title">Leaderboard</h2>
          <div className="mt-5 grid gap-3">
            {leaderboard.map((row) => (
              <div
                className="surface-panel-soft flex items-center justify-between gap-4 px-4 py-4 text-lg text-[var(--muted-strong)]"
                key={row.trader_id}
              >
                <div>
                  <p className="text-xl font-bold text-white">
                    #{row.rank} {row.username}
                  </p>
                  <p>
                    Cash {formatCurrency(row.available_cash)} · Positions{" "}
                    {formatCurrency(row.position_value)}
                  </p>
                </div>
                <p className="text-xl font-semibold text-white">
                  {formatCurrency(row.equity)}
                </p>
              </div>
            ))}
          </div>
        </section>
      </div>

      <section className="surface-panel px-6 py-6">
        <h2 className="surface-title">Recent messages</h2>
        <div className="mt-5 grid gap-3">
          {adminState.recent_messages.length === 0 ? (
            <div className="surface-panel-soft px-4 py-4 text-lg text-[var(--muted-strong)]">
              No admin messages have been sent yet.
            </div>
          ) : (
            adminState.recent_messages.map((message) => (
              <div
                className="surface-panel-soft px-4 py-4 text-lg text-[var(--muted-strong)]"
                key={message.message_id}
              >
                <div className="flex flex-wrap items-center justify-between gap-3">
                  <p className={`text-base font-semibold uppercase tracking-[0.2em] ${toneClass(message.level)}`}>
                    {message.level}
                  </p>
                  <p>{formatTimestamp(message.created_at)}</p>
                </div>
                {message.title ? (
                  <p className="mt-2 text-xl font-bold text-white">{message.title}</p>
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
