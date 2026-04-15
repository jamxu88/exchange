"use client";

import {
  startTransition,
  useEffect,
  useEffectEvent,
  useRef,
  useState,
} from "react";
import type {
  ExchangeActionTelemetry,
  ExchangeAdminTelemetry,
  ExchangeBarrierWaitStatus,
  ExchangeCounterTelemetry,
  ExchangeDispatchQueueStatus,
  ExchangePersistenceStatus,
  ExchangeWebSocketTelemetry,
} from "@/lib/exchange-server";

const POLL_INTERVAL_MS = 2_000;

type LiveTelemetryPanelProps = {
  initialTelemetry: ExchangeAdminTelemetry | null;
};

function formatNumber(value: number) {
  return new Intl.NumberFormat("en-US").format(value);
}

function formatPercent(value: number) {
  return `${Math.round(value)}%`;
}

function formatElapsedMs(value: number) {
  return `${formatNumber(value)} ms`;
}

function formatRate(value: number) {
  return `${new Intl.NumberFormat("en-US", {
    maximumFractionDigits: value >= 10 ? 1 : 2,
  }).format(value)}/s`;
}

function formatQueueUtilization(depth: number, capacity: number) {
  if (capacity <= 0) {
    return "0%";
  }
  return formatPercent((depth / capacity) * 100);
}

function queueModeClass(mode: ExchangeDispatchQueueStatus["mode"] | ExchangePersistenceStatus["mode"]) {
  if (mode === "backpressured" || mode === "retrying" || mode === "stopped") {
    return "border-[rgba(216,91,91,0.42)] bg-[rgba(216,91,91,0.08)] text-[#ffb2b2]";
  }

  if (mode === "disabled") {
    return "border-[rgba(183,183,189,0.22)] bg-[rgba(183,183,189,0.08)] text-[var(--muted-strong)]";
  }

  return "border-[rgba(66,204,78,0.35)] bg-[rgba(66,204,78,0.08)] text-[#b8ffbd]";
}

function healthStatusClass(status: ExchangeAdminTelemetry["status"]) {
  return status === "ok"
    ? "border-[rgba(66,204,78,0.35)] bg-[rgba(66,204,78,0.08)] text-[#b8ffbd]"
    : "border-[rgba(216,91,91,0.42)] bg-[rgba(216,91,91,0.08)] text-[#ffb2b2]";
}

function formatTimestamp(value: string) {
  return new Date(value).toLocaleTimeString("en-US", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function QueueCard({
  title,
  queue,
  flushLatencyMs,
  maxFlushLatencyMs,
  retries,
  lastError,
}: {
  title: string;
  queue: ExchangeDispatchQueueStatus | ExchangePersistenceStatus;
  flushLatencyMs?: number;
  maxFlushLatencyMs?: number;
  retries?: number;
  lastError?: string | null;
}) {
  return (
    <div className="ops-panel-soft grid gap-3 px-4 py-4 text-[14px] text-[var(--muted-strong)]">
      <div className="flex items-center justify-between gap-3">
        <p className="ops-kicker">{title}</p>
        <span className={`ops-badge ${queueModeClass(queue.mode)}`}>
          {queue.mode}
        </span>
      </div>
      <div className="grid gap-2 sm:grid-cols-2">
        <p>
          Depth <span className="font-semibold text-white">{formatNumber(queue.queue_depth)}</span>
          {" / "}
          {formatNumber(queue.queue_capacity)}
          {" · "}
          {formatQueueUtilization(queue.queue_depth, queue.queue_capacity)}
        </p>
        <p>
          High-water <span className="font-semibold text-white">{formatNumber(queue.high_water_mark)}</span>
        </p>
        <p>
          Enqueued <span className="font-semibold text-white">{formatNumber(queue.total_enqueued)}</span>
        </p>
        {"total_dequeued" in queue ? (
          <p>
            Dequeued <span className="font-semibold text-white">{formatNumber(queue.total_dequeued)}</span>
          </p>
        ) : (
          <p>
            Flushed <span className="font-semibold text-white">{formatNumber(queue.total_flushed_ops)}</span>
          </p>
        )}
        <p>
          Blocked enqueues <span className="font-semibold text-white">{formatNumber(queue.total_blocked_enqueues)}</span>
        </p>
        <p>
          Blocked time <span className="font-semibold text-white">{formatElapsedMs(queue.total_enqueue_block_time_ms)}</span>
        </p>
      </div>
      {typeof flushLatencyMs === "number" && typeof maxFlushLatencyMs === "number" ? (
        <div className="grid gap-2 sm:grid-cols-2">
          <p>
            Last flush <span className="font-semibold text-white">{formatElapsedMs(flushLatencyMs)}</span>
          </p>
          <p>
            Max flush <span className="font-semibold text-white">{formatElapsedMs(maxFlushLatencyMs)}</span>
          </p>
        </div>
      ) : null}
      {typeof retries === "number" ? (
        <p>
          Retries <span className="font-semibold text-white">{formatNumber(retries)}</span>
        </p>
      ) : null}
      {lastError ? (
        <p className="text-[#ffb2b2]">
          Last error: {lastError}
        </p>
      ) : null}
    </div>
  );
}

function BarrierCard({
  label,
  status,
}: {
  label: string;
  status: ExchangeBarrierWaitStatus;
}) {
  return (
    <div className="ops-panel-soft grid gap-2 px-4 py-4 text-[14px] text-[var(--muted-strong)]">
      <p className="ops-kicker">{label}</p>
      <p>
        Total waits <span className="font-semibold text-white">{formatNumber(status.total_waits)}</span>
      </p>
      <p>
        Last wait <span className="font-semibold text-white">{formatElapsedMs(status.last_wait_time_ms)}</span>
        {" · "}max {formatElapsedMs(status.max_wait_time_ms)}
      </p>
      <p>
        {">"}1ms {formatNumber(status.waits_over_1ms)}
        {" · "}{">"}5ms {formatNumber(status.waits_over_5ms)}
        {" · "}{">"}25ms {formatNumber(status.waits_over_25ms)}
        {" · "}{">"}100ms {formatNumber(status.waits_over_100ms)}
      </p>
    </div>
  );
}

function ActionCard({
  title,
  stats,
}: {
  title: string;
  stats: ExchangeActionTelemetry;
}) {
  return (
    <div className="ops-panel-soft grid gap-2 px-4 py-4 text-[14px] text-[var(--muted-strong)]">
      <p className="ops-kicker">{title}</p>
      <p>
        Total <span className="font-semibold text-white">{formatNumber(stats.total)}</span>
        {" · "}
        {formatRate(stats.total_per_second_10s)}
      </p>
      <p>
        Accepted <span className="font-semibold text-white">{formatNumber(stats.accepted)}</span>
        {" · "}
        {formatRate(stats.accepted_per_second_10s)}
      </p>
      <p>
        Rejected <span className="font-semibold text-white">{formatNumber(stats.rejected)}</span>
        {" · "}
        {formatRate(stats.rejected_per_second_10s)}
      </p>
    </div>
  );
}

function CounterCard({
  title,
  stats,
}: {
  title: string;
  stats: ExchangeCounterTelemetry;
}) {
  return (
    <div className="ops-panel-soft grid gap-2 px-4 py-4 text-[14px] text-[var(--muted-strong)]">
      <p className="ops-kicker">{title}</p>
      <p>
        Total <span className="font-semibold text-white">{formatNumber(stats.total)}</span>
      </p>
      <p>
        Rate <span className="font-semibold text-white">{formatRate(stats.per_second_10s)}</span>
      </p>
    </div>
  );
}

function WebSocketCard({
  stats,
}: {
  stats: ExchangeWebSocketTelemetry;
}) {
  return (
    <div className="ops-panel-soft grid gap-2 px-4 py-4 text-[14px] text-[var(--muted-strong)]">
      <p className="ops-kicker">WebSocket activity</p>
      <p>
        Connections <span className="font-semibold text-white">{formatNumber(stats.connections_current)}</span>
        {" current · "}
        {formatNumber(stats.connections_total)}
        {" total"}
      </p>
      <p>
        Authenticated <span className="font-semibold text-white">{formatNumber(stats.authenticated_current)}</span>
        {" current · "}
        {formatNumber(stats.authenticated_total)}
        {" total"}
      </p>
      <p>
        Data stream subscribers <span className="font-semibold text-white">{formatNumber(stats.data_stream_subscribers_current)}</span>
      </p>
    </div>
  );
}

function FillCard({
  total,
  shares,
  fillsPerSecond,
  sharesPerSecond,
}: {
  total: number;
  shares: number;
  fillsPerSecond: number;
  sharesPerSecond: number;
}) {
  return (
    <div className="ops-panel-soft grid gap-2 px-4 py-4 text-[14px] text-[var(--muted-strong)]">
      <p className="ops-kicker">Fills</p>
      <p>
        Fills <span className="font-semibold text-white">{formatNumber(total)}</span>
        {" · "}
        {formatRate(fillsPerSecond)}
      </p>
      <p>
        Shares <span className="font-semibold text-white">{formatNumber(shares)}</span>
        {" · "}
        {formatRate(sharesPerSecond)}
      </p>
    </div>
  );
}

export function LiveTelemetryPanel({ initialTelemetry }: LiveTelemetryPanelProps) {
  const [telemetry, setTelemetry] = useState(initialTelemetry);
  const [error, setError] = useState<string | null>(null);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [lastUpdatedAt, setLastUpdatedAt] = useState<string | null>(
    initialTelemetry?.now ?? null,
  );
  const mountedRef = useRef(true);

  const refreshTelemetry = useEffectEvent(async () => {
    setIsRefreshing(true);

    try {
      const response = await fetch("/api/admin/telemetry", {
        cache: "no-store",
      });
      const text = await response.text();
      const payload = text ? JSON.parse(text) : null;

      if (!response.ok) {
        throw new Error(
          payload && typeof payload.error === "string"
            ? payload.error
            : `Telemetry request failed with ${response.status}`,
        );
      }

      if (mountedRef.current) {
        startTransition(() => {
          setTelemetry(payload as ExchangeAdminTelemetry);
          setError(null);
          setLastUpdatedAt((payload as ExchangeAdminTelemetry).now);
        });
      }
    } catch (loadError) {
      if (mountedRef.current) {
        startTransition(() => {
          setError(loadError instanceof Error ? loadError.message : "Failed to refresh telemetry.");
        });
      }
    } finally {
      if (mountedRef.current) {
        startTransition(() => {
          setIsRefreshing(false);
        });
      }
    }
  });

  useEffect(() => {
    let cancelled = false;
    let timeoutId: number | undefined;
    mountedRef.current = true;

    const loop = async () => {
      await refreshTelemetry();
      if (!cancelled) {
        timeoutId = window.setTimeout(loop, POLL_INTERVAL_MS);
      }
    };

    void loop();

    return () => {
      cancelled = true;
      mountedRef.current = false;
      if (timeoutId) {
        window.clearTimeout(timeoutId);
      }
    };
  }, []);

  const stale = lastUpdatedAt
    ? Date.now() - new Date(lastUpdatedAt).getTime() > POLL_INTERVAL_MS * 3
    : false;

  return (
    <div className="mt-4 grid gap-4">
      <div className="ops-panel-soft px-4 py-4 text-[15px] text-[var(--muted-strong)]">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <p className="ops-section-title">Live telemetry</p>
            <p className="mt-2 text-sm text-[var(--muted)]">
              Polling operator telemetry every 2 seconds for traffic, websocket, queue, and account barrier health.
            </p>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            {telemetry ? (
              <span className={`ops-badge ${healthStatusClass(telemetry.status)}`}>
                {telemetry.status}
              </span>
            ) : null}
            <span className={`ops-badge ${stale ? "border-[rgba(216,91,91,0.42)] bg-[rgba(216,91,91,0.08)] text-[#ffb2b2]" : "border-[rgba(183,183,189,0.22)] bg-[rgba(183,183,189,0.08)] text-[var(--muted-strong)]"}`}>
              {isRefreshing ? "refreshing" : stale ? "stale" : "live"}
            </span>
          </div>
        </div>
        <div className="mt-3 flex flex-wrap gap-x-5 gap-y-2 text-sm">
          <p>
            Backend time{" "}
            <span className="font-semibold text-white">
              {telemetry ? formatTimestamp(telemetry.now) : "Unavailable"}
            </span>
          </p>
          <p>
            Last sample{" "}
            <span className="font-semibold text-white">
              {lastUpdatedAt ? formatTimestamp(lastUpdatedAt) : "Never"}
            </span>
          </p>
        </div>
        {error ? (
          <p className="mt-3 text-sm text-[#ffb2b2]">
            {error}
          </p>
        ) : null}
      </div>

      {telemetry ? (
        <>
          <div className="grid gap-3 xl:grid-cols-3">
            <ActionCard
              stats={telemetry.traffic.submits}
              title="Order submits"
            />
            <ActionCard
              stats={telemetry.traffic.cancels}
              title="Cancels"
            />
            <ActionCard
              stats={telemetry.traffic.amends}
              title="Amends"
            />
          </div>

          <div className="grid gap-3 xl:grid-cols-3">
            <FillCard
              fillsPerSecond={telemetry.traffic.fills.fills_per_second_10s}
              shares={telemetry.traffic.fills.shares}
              sharesPerSecond={telemetry.traffic.fills.shares_per_second_10s}
              total={telemetry.traffic.fills.total}
            />
            <CounterCard
              stats={telemetry.traffic.rate_limit_rejections}
              title="Rate-limit rejects"
            />
            <WebSocketCard
              stats={telemetry.traffic.websocket}
            />
          </div>

          <div className="grid gap-3 xl:grid-cols-4">
            <CounterCard
              stats={telemetry.traffic.resyncs.user}
              title="User resyncs"
            />
            <CounterCard
              stats={telemetry.traffic.resyncs.system}
              title="System resyncs"
            />
            <CounterCard
              stats={telemetry.traffic.resyncs.data_stream}
              title="Data stream resyncs"
            />
          </div>

          <div className="grid gap-3 xl:grid-cols-2">
            <QueueCard
              flushLatencyMs={telemetry.persistence.last_flush_latency_ms}
              lastError={telemetry.persistence.last_error}
              maxFlushLatencyMs={telemetry.persistence.max_flush_latency_ms}
              queue={telemetry.persistence}
              retries={telemetry.persistence.total_retries}
              title="Persistence"
            />
            <QueueCard
              queue={telemetry.runtime_dispatch}
              title="Runtime dispatch"
            />
            <QueueCard
              queue={telemetry.account_dispatch}
              title="Account dispatch"
            />
            <QueueCard
              queue={telemetry.persistence_dispatch}
              title="Persistence dispatch"
            />
          </div>

          <div className="grid gap-3 xl:grid-cols-3">
            <BarrierCard
              label="Submit barrier"
              status={telemetry.account_barrier.submit}
            />
            <BarrierCard
              label="Cancel barrier"
              status={telemetry.account_barrier.cancel}
            />
            <BarrierCard
              label="Amend barrier"
              status={telemetry.account_barrier.amend}
            />
          </div>
        </>
      ) : (
        <div className="ops-panel-soft px-4 py-4 text-[15px] text-[var(--muted-strong)]">
          Telemetry is unavailable.
        </div>
      )}
    </div>
  );
}
