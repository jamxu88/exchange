import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AdminMarketBotManager } from "@/app/(dashboard)/admin/admin-market-bot-manager";
import type {
  ExchangeAdminBot,
  ExchangeAdminState,
  ExchangeMarket,
  ExchangePersistenceStatus,
} from "@/lib/exchange-server";

function jsonResponse(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      "content-type": "application/json",
    },
  });
}

function makePersistenceStatus(): ExchangePersistenceStatus {
  return {
    backend: "in_memory",
    mode: "ok",
    queue_capacity: 0,
    backpressure_threshold: 0,
    queue_depth: 0,
    in_flight_ops: 0,
    backlog_depth: 0,
    high_water_mark: 0,
    total_enqueued: 0,
    total_flushes: 0,
    total_flushed_ops: 0,
    total_blocked_enqueues: 0,
    total_enqueue_block_time_ms: 0,
    total_flush_failures: 0,
    total_retries: 0,
    last_batch_size: 0,
    last_flush_latency_ms: 0,
    max_flush_latency_ms: 0,
    last_error: null,
  };
}

function makeMarket(overrides: Partial<ExchangeMarket> = {}): ExchangeMarket {
  return {
    market_id: "BTC-USD",
    display_name: "Bitcoin",
    base_asset: "BTC",
    quote_asset: "USD",
    tick_size: 1,
    min_order_quantity: 1,
    min_price: null,
    max_price: null,
    reference_price: 100,
    settlement_price: null,
    status: "enabled",
    created_at: "2026-04-13T12:00:00Z",
    updated_at: "2026-04-13T12:00:00Z",
    ...overrides,
  };
}

function makeBot(overrides: Partial<ExchangeAdminBot> = {}): ExchangeAdminBot {
  return {
    bot_id: "depth-maker-1",
    display_name: "Depth maker",
    trader_id: "trader-1",
    trader_username: "bot-depth-maker-1",
    market_id: "BTC-USD",
    strategy: "maker",
    side_mode: "both",
    status: "paused",
    min_quantity: 1,
    max_quantity: 2,
    interval_ms: 1000,
    max_open_orders: 2,
    min_price: 99,
    max_price: 101,
    last_error: null,
    last_submitted_at: null,
    created_at: "2026-04-13T12:00:00Z",
    updated_at: "2026-04-13T12:00:00Z",
    ...overrides,
  };
}

function makeAdminState(
  overrides: Partial<ExchangeAdminState> = {},
): ExchangeAdminState {
  return {
    controls: {
      trading_enabled: true,
      updated_at: "2026-04-13T12:00:00Z",
    },
    markets: [makeMarket()],
    bots: [makeBot()],
    admin_desk: null,
    recent_messages: [],
    persistence: makePersistenceStatus(),
    ...overrides,
  };
}

describe("AdminMarketBotManager", () => {
  const fetchMock = vi.fn<typeof fetch>();

  beforeEach(() => {
    fetchMock.mockReset();
    vi.stubGlobal("fetch", fetchMock);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("creates a market in place and updates the local market list", async () => {
    const nextState = makeAdminState({
      markets: [
        makeMarket(),
        makeMarket({
          market_id: "ETHEREUM-MARKET",
          display_name: "Ethereum",
          base_asset: "ETHEREUM",
          reference_price: 200,
        }),
      ],
    });
    fetchMock.mockResolvedValueOnce(jsonResponse({
      adminState: nextState,
      notice: "ETHEREUM-MARKET saved.",
    }));

    render(
      <AdminMarketBotManager
        initialBots={[]}
        initialMarkets={[makeMarket()]}
      />,
    );

    const user = userEvent.setup();
    await user.type(screen.getByLabelText("Display Name"), "Ethereum");
    await user.click(screen.getByRole("button", { name: "Save market" }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        "/api/admin/markets",
        expect.objectContaining({ method: "POST" }),
      );
    });

    expect(await screen.findByText("Ethereum")).toBeInTheDocument();
    expect(
      screen.getByText(/ETHEREUM-MARKET · tick 1 · min qty 1 · price unbounded/),
    ).toBeInTheDocument();
    expect(screen.getByText("ETHEREUM-MARKET saved.")).toBeInTheDocument();
  });

  it("lets operators collapse the bot roster and supports deleting all bots", async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse({
      adminState: makeAdminState({
        bots: [],
      }),
      notice: "All bots deleted.",
    }));

    render(
      <AdminMarketBotManager
        initialBots={[makeBot()]}
        initialMarkets={[makeMarket()]}
      />,
    );

    const user = userEvent.setup();

    expect(screen.getByRole("button", { name: "Hide bot roster" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Hide bot roster" }));

    expect(screen.getByText("1 bot hidden.")).toBeInTheDocument();
    expect(screen.queryByText("Depth maker")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Show bot roster" }));

    const botDetails = screen.getByText("Depth maker").closest("details");
    expect(botDetails).not.toHaveAttribute("open");

    await user.click(screen.getByText("Depth maker"));

    expect(botDetails).toHaveAttribute("open");
    expect(screen.getByText("Trader bot-depth-maker-1")).toBeInTheDocument();
    expect(screen.getByText("Strategy maker")).toBeInTheDocument();
    expect(screen.getByText(/Price bounds/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Delete all bots" }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        "/api/admin/bots",
        expect.objectContaining({ method: "DELETE" }),
      );
    });

    expect(await screen.findByText("All bots deleted.")).toBeInTheDocument();
    expect(screen.getByText("No bots configured yet.")).toBeInTheDocument();
  });
});
