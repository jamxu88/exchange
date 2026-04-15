import { renderHook, act, waitFor } from "@testing-library/react";
import { TRADE_MESSAGE_HISTORY_STORAGE_KEY } from "@/components/trade/trade-message-history";
import { useTradeController } from "@/components/trade/use-trade-controller";
import type { TradeRuntimeConfig } from "@/components/trade/trade-runtime";
import type { TradeWsCallbacks } from "@/components/trade/trade-ws-client";

const runtime: TradeRuntimeConfig = {
  httpUrl: "http://localhost:8080",
  wsUrl: "ws://localhost:8080/ws",
  apiKey: "secret",
  reconnectDelayMs: 1000,
  markets: [
    { id: "BTC-USD", name: "BTC-USD", baseAsset: "BTC", quoteAsset: "USD", status: "enabled" },
    { id: "ETH-USD", name: "ETH-USD", baseAsset: "ETH", quoteAsset: "USD", status: "enabled" },
  ],
};

describe("useTradeController", () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  it("bootstraps state and updates websocket subscriptions on market changes", async () => {
    const bootstrapAccountData = vi.fn().mockResolvedValue({
      markets: runtime.markets,
      user: { traderId: "trader-1", teamNumber: "TEAM-ALICE" },
      positions: [{ market: "BTC-USD", netQuantity: 2, averageEntryPrice: 100, realizedPnl: 0 }],
      openOrders: [],
      fills: [],
      warnings: [],
      loaded: {
        markets: true,
        user: true,
        positions: true,
        openOrders: true,
        fills: true,
      },
    });
    const submitOrder = vi.fn();
    const cancelOrder = vi.fn();
    const updateMarket = vi.fn();
    const connect = vi.fn();
    const disconnect = vi.fn();
    const restClientFactory = () =>
      ({
        bootstrapAccountData,
        submitOrder,
        cancelOrder,
      }) as never;
    const wsClientFactory = (_config: unknown, callbacks: { onStatusChange: (status: "connected") => void }) =>
      ({
        connect: () => {
          connect();
          callbacks.onStatusChange("connected");
        },
        disconnect,
        updateMarket,
      }) as never;

    const { result } = renderHook(() =>
      useTradeController({
        runtime,
        restClientFactory,
        wsClientFactory,
      }),
    );

    await waitFor(() => {
      expect(result.current.state.bootstrapStatus).toBe("ready");
    });

    expect(connect).toHaveBeenCalled();
    expect(result.current.state.user?.teamNumber).toBe("TEAM-ALICE");

    act(() => {
      result.current.actions.selectMarket("ETH-USD");
    });

    expect(updateMarket).toHaveBeenCalledWith("ETH-USD");
  });

  it("removes deleted markets from state and resubscribes when the selected market is deleted", async () => {
    const bootstrapAccountData = vi.fn().mockResolvedValue({
      markets: runtime.markets,
      user: { traderId: "trader-1", teamNumber: "TEAM-ALICE" },
      positions: [],
      openOrders: [],
      fills: [],
      warnings: [],
      loaded: {
        markets: true,
        user: true,
        positions: true,
        openOrders: true,
        fills: true,
      },
    });
    const updateMarket = vi.fn();
    let wsCallbacks: TradeWsCallbacks | undefined;
    const restClientFactory = () =>
      ({
        bootstrapAccountData,
        submitOrder: vi.fn(),
        cancelOrder: vi.fn(),
      }) as never;
    const wsClientFactory = (_config: unknown, callbacks: TradeWsCallbacks) => {
      wsCallbacks = callbacks;
      return {
        connect: vi.fn(),
        disconnect: vi.fn(),
        updateMarket,
      } as never;
    };

    const { result } = renderHook(() =>
      useTradeController({
        runtime,
        restClientFactory,
        wsClientFactory,
      }),
    );

    await waitFor(() => {
      expect(result.current.state.bootstrapStatus).toBe("ready");
      expect(wsCallbacks).toBeDefined();
    });

    act(() => {
      result.current.actions.selectMarket("ETH-USD");
    });

    await waitFor(() => {
      expect(updateMarket).toHaveBeenCalledWith("ETH-USD");
    });

    act(() => {
      wsCallbacks?.onMarketDeleted({ marketId: "ETH-USD" });
    });

    await waitFor(() => {
      expect(result.current.state.availableMarkets.map((market) => market.id)).toEqual([
        "BTC-USD",
      ]);
      expect(result.current.state.selectedMarketId).toBe("BTC-USD");
    });

    expect(updateMarket).toHaveBeenLastCalledWith("BTC-USD");
  });

  it("submits orders through the rest client and updates local state", async () => {
    const submitOrder = vi.fn().mockResolvedValue({
      orderId: "order-1",
      marketId: "BTC-USD",
      marketName: "BTC-USD",
      side: "buy",
      orderType: "limit",
      quantity: 2,
      requestedPrice: 101,
      effectivePrice: 101,
      resting: true,
      remaining: 1,
      fills: [
        {
          fillId: "fill-1",
          market: "BTC-USD",
          makerOrderId: "maker-1",
          takerOrderId: "order-1",
          price: 101,
          quantity: 1,
          occurredAt: "2026-03-17T09:30:00Z",
        },
      ],
      createdAt: "2026-03-17T09:30:00Z",
    });
    const restClientFactory = () =>
      ({
        bootstrapAccountData: vi.fn().mockResolvedValue({
          markets: runtime.markets,
          user: null,
          positions: [],
          openOrders: [],
          fills: [],
          warnings: [],
          loaded: {
            markets: true,
            user: true,
            positions: true,
            openOrders: true,
            fills: true,
          },
        }),
        submitOrder,
        cancelOrder: vi.fn(),
      }) as never;
    const wsClientFactory = () =>
      ({
        connect: vi.fn(),
        disconnect: vi.fn(),
        updateMarket: vi.fn(),
      }) as never;

    const { result } = renderHook(() =>
      useTradeController({
        runtime,
        restClientFactory,
        wsClientFactory,
      }),
    );

    await waitFor(() => {
      expect(result.current.state.bootstrapStatus).toBe("ready");
    });

    act(() => {
      result.current.actions.setLimitPrice("101");
      result.current.actions.setShares("2");
    });

    await act(async () => {
      await result.current.actions.submitOrder();
    });

    expect(submitOrder).toHaveBeenCalled();
    expect(result.current.state.pendingOrders).toHaveLength(1);
    expect(result.current.state.filledOrders).toBe(1);
  });

  it("truncates decimal limit input before submit", async () => {
    const submitOrder = vi.fn().mockResolvedValue({
      orderId: "order-1",
      marketId: "BTC-USD",
      marketName: "BTC-USD",
      side: "buy",
      orderType: "limit",
      quantity: 2,
      requestedPrice: 1,
      effectivePrice: 1,
      resting: false,
      remaining: 0,
      fills: [],
      createdAt: "2026-03-17T09:30:00Z",
    });
    const restClientFactory = () =>
      ({
        bootstrapAccountData: vi.fn().mockResolvedValue({
          markets: runtime.markets,
          user: null,
          positions: [],
          openOrders: [],
          fills: [],
          warnings: [],
          loaded: {
            markets: true,
            user: true,
            positions: true,
            openOrders: true,
            fills: true,
          },
        }),
        submitOrder,
        cancelOrder: vi.fn(),
      }) as never;
    const wsClientFactory = () =>
      ({
        connect: vi.fn(),
        disconnect: vi.fn(),
        updateMarket: vi.fn(),
      }) as never;

    const { result } = renderHook(() =>
      useTradeController({
        runtime,
        restClientFactory,
        wsClientFactory,
      }),
    );

    await waitFor(() => {
      expect(result.current.state.bootstrapStatus).toBe("ready");
    });

    act(() => {
      result.current.actions.setLimitPrice("1.2");
      result.current.actions.setShares("2");
    });

    expect(result.current.state.limitPriceInput).toBe("1");

    await act(async () => {
      await result.current.actions.submitOrder();
    });

    expect(submitOrder).toHaveBeenCalledWith(
      expect.objectContaining({
        requestedPrice: 1,
        effectivePrice: 1,
      }),
    );
  });

  it("rejects submit locally when the selected market is disabled", async () => {
    const submitOrder = vi.fn();
    const disabledRuntime: TradeRuntimeConfig = {
      ...runtime,
      markets: [
        {
          id: "BTC-USD",
          name: "BTC-USD",
          baseAsset: "BTC",
          quoteAsset: "USD",
          status: "disabled",
        },
      ],
    };
    const restClientFactory = () =>
      ({
        bootstrapAccountData: vi.fn().mockResolvedValue({
          markets: disabledRuntime.markets,
          user: null,
          positions: [],
          openOrders: [],
          fills: [],
          warnings: [],
          loaded: {
            markets: true,
            user: true,
            positions: true,
            openOrders: true,
            fills: true,
          },
        }),
        submitOrder,
        cancelOrder: vi.fn(),
      }) as never;
    const wsClientFactory = () =>
      ({
        connect: vi.fn(),
        disconnect: vi.fn(),
        updateMarket: vi.fn(),
      }) as never;

    const { result } = renderHook(() =>
      useTradeController({
        runtime: disabledRuntime,
        restClientFactory,
        wsClientFactory,
      }),
    );

    await waitFor(() => {
      expect(result.current.state.bootstrapStatus).toBe("ready");
    });

    await act(async () => {
      await result.current.actions.submitOrder();
    });

    expect(submitOrder).not.toHaveBeenCalled();
    expect(result.current.state.messages.at(-1)?.text).toBe("Rejected order: market is disabled.");
  });

  it("hydrates and persists trade messages in local storage", async () => {
    window.localStorage.setItem(
      TRADE_MESSAGE_HISTORY_STORAGE_KEY,
      JSON.stringify({
        messages: [
          {
            id: 1,
            time: "09:29:59",
            tone: "neutral",
            text: "Previous session message.",
          },
        ],
      }),
    );

    const restClientFactory = () =>
      ({
        bootstrapAccountData: vi.fn().mockResolvedValue({
          markets: runtime.markets,
          user: null,
          positions: [],
          openOrders: [],
          fills: [],
          warnings: [],
          loaded: {
            markets: true,
            user: true,
            positions: true,
            openOrders: true,
            fills: true,
          },
        }),
        submitOrder: vi.fn(),
        cancelOrder: vi.fn(),
      }) as never;
    const wsClientFactory = () =>
      ({
        connect: vi.fn(),
        disconnect: vi.fn(),
        updateMarket: vi.fn(),
      }) as never;

    const { result } = renderHook(() =>
      useTradeController({
        runtime,
        restClientFactory,
        wsClientFactory,
      }),
    );

    await waitFor(() => {
      expect(result.current.state.messages.some((message) => message.text === "Previous session message.")).toBe(true);
    });

    await waitFor(() => {
      expect(result.current.state.bootstrapStatus).toBe("ready");
    });

    const stored = JSON.parse(
      window.localStorage.getItem(TRADE_MESSAGE_HISTORY_STORAGE_KEY) ?? "{}",
    ) as { messages?: Array<{ text: string }> };

    expect(stored.messages?.some((message) => message.text === "Previous session message.")).toBe(
      true,
    );
    expect(
      stored.messages?.some((message) => message.text.includes("Loaded account state") || message.text.includes("Connected in public market-data mode.")),
    ).toBe(true);
  });

  it("surfaces a bootstrap failure without retrying on every render", async () => {
    const bootstrapAccountData = vi.fn().mockRejectedValue(new Error("bootstrap failed"));
    const restClientFactory = () =>
      ({
        bootstrapAccountData,
        submitOrder: vi.fn(),
        cancelOrder: vi.fn(),
      }) as never;
    const wsClientFactory = () =>
      ({
        connect: vi.fn(),
        disconnect: vi.fn(),
        updateMarket: vi.fn(),
      }) as never;

    const { result } = renderHook(() =>
      useTradeController({
        runtime,
        restClientFactory,
        wsClientFactory,
      }),
    );

    await waitFor(() => {
      expect(result.current.state.bootstrapStatus).toBe("error");
    });

    expect(result.current.state.messages.at(-1)?.text).toContain("bootstrap failed");
    expect(bootstrapAccountData).toHaveBeenCalledTimes(1);
  });

  it("cancels a pending order without forcing a full account refresh", async () => {
    const bootstrapAccountData = vi
      .fn()
      .mockResolvedValueOnce({
        markets: runtime.markets,
        user: { traderId: "trader-1", teamNumber: "TEAM-ALICE" },
        positions: [],
        openOrders: [
          {
            id: "order-1",
            createdAt: "2026-03-17T09:30:00Z",
            marketId: "BTC-USD",
            marketName: "BTC-USD",
            side: "buy",
            shares: 2,
            limitPrice: 101,
            status: "open",
          },
        ],
        fills: [],
        warnings: [],
        loaded: {
          markets: true,
          user: true,
          positions: true,
          openOrders: true,
          fills: true,
        },
      })
      .mockResolvedValueOnce({
        markets: runtime.markets,
        user: { traderId: "trader-1", teamNumber: "TEAM-ALICE" },
        positions: [],
        openOrders: [],
        fills: [],
        warnings: [],
        loaded: {
          markets: true,
          user: true,
          positions: true,
          openOrders: true,
          fills: true,
        },
      });
    const cancelOrder = vi.fn().mockResolvedValue({
      id: "order-1",
      createdAt: "2026-03-17T09:30:00Z",
      marketId: "BTC-USD",
      marketName: "BTC-USD",
      side: "buy",
      shares: 2,
      limitPrice: 101,
      status: "open",
    });
    const restClientFactory = () =>
      ({
        bootstrapAccountData,
        submitOrder: vi.fn(),
        cancelOrder,
      }) as never;
    const wsClientFactory = () =>
      ({
        connect: vi.fn(),
        disconnect: vi.fn(),
        updateMarket: vi.fn(),
      }) as never;

    const { result } = renderHook(() =>
      useTradeController({
        runtime,
        restClientFactory,
        wsClientFactory,
      }),
    );

    await waitFor(() => {
      expect(result.current.state.bootstrapStatus).toBe("ready");
      expect(result.current.state.pendingOrders).toHaveLength(1);
    });

    await act(async () => {
      await result.current.actions.cancelPendingOrder("order-1");
    });

    await waitFor(() => {
      expect(result.current.state.pendingOrders).toHaveLength(0);
    });

    expect(cancelOrder).toHaveBeenCalledWith("order-1");
    expect(bootstrapAccountData).toHaveBeenCalledTimes(1);
  });

  it("keeps other pending orders visible when the post-cancel open-orders refresh is degraded", async () => {
    const bootstrapAccountData = vi
      .fn()
      .mockResolvedValueOnce({
        markets: runtime.markets,
        user: { traderId: "trader-1", teamNumber: "TEAM-ALICE" },
        positions: [],
        openOrders: [
          {
            id: "order-1",
            createdAt: "2026-03-17T09:30:00Z",
            marketId: "BTC-USD",
            marketName: "BTC-USD",
            side: "buy",
            shares: 2,
            limitPrice: 101,
            status: "open",
          },
          {
            id: "order-2",
            createdAt: "2026-03-17T09:31:00Z",
            marketId: "ETH-USD",
            marketName: "ETH-USD",
            side: "sell",
            shares: 3,
            limitPrice: 202,
            status: "open",
          },
        ],
        fills: [],
        warnings: [],
        loaded: {
          markets: true,
          user: true,
          positions: true,
          openOrders: true,
          fills: true,
        },
      })
      .mockResolvedValueOnce({
        markets: runtime.markets,
        user: { traderId: "trader-1", teamNumber: "TEAM-ALICE" },
        positions: [],
        openOrders: [],
        fills: [],
        warnings: ["Open order bootstrap failed. per-user rate limit exceeded: max 500 ops per 10s"],
        loaded: {
          markets: true,
          user: true,
          positions: true,
          openOrders: false,
          fills: true,
        },
      });
    const cancelOrder = vi.fn().mockResolvedValue({
      id: "order-1",
      createdAt: "2026-03-17T09:30:00Z",
      marketId: "BTC-USD",
      marketName: "BTC-USD",
      side: "buy",
      shares: 2,
      limitPrice: 101,
      status: "open",
    });
    const restClientFactory = () =>
      ({
        bootstrapAccountData,
        submitOrder: vi.fn(),
        cancelOrder,
      }) as never;
    const wsClientFactory = () =>
      ({
        connect: vi.fn(),
        disconnect: vi.fn(),
        updateMarket: vi.fn(),
      }) as never;

    const { result } = renderHook(() =>
      useTradeController({
        runtime,
        restClientFactory,
        wsClientFactory,
      }),
    );

    await waitFor(() => {
      expect(result.current.state.pendingOrders).toHaveLength(2);
    });

    await act(async () => {
      await result.current.actions.cancelPendingOrder("order-1");
    });

    await waitFor(() => {
      expect(result.current.state.pendingOrders.map((order) => order.id)).toEqual(["order-2"]);
    });
  });

  it("suppresses auto-healed L2 resync chatter and refreshes account state after re-authentication", async () => {
    const bootstrapAccountData = vi
      .fn()
      .mockResolvedValue({
        markets: runtime.markets,
        user: { traderId: "trader-1", teamNumber: "TEAM-ALICE" },
        positions: [],
        openOrders: [],
        fills: [],
        warnings: [],
        loaded: {
          markets: true,
          user: true,
          positions: true,
          openOrders: true,
          fills: true,
        },
      });
    const restClientFactory = () =>
      ({
        bootstrapAccountData,
        submitOrder: vi.fn(),
        cancelOrder: vi.fn(),
      }) as never;
    let wsCallbacks: TradeWsCallbacks | undefined;
    const wsClientFactory = (_config: unknown, callbacks: TradeWsCallbacks) => {
      wsCallbacks = callbacks;
      return {
        connect: vi.fn(),
        disconnect: vi.fn(),
        updateMarket: vi.fn(),
      } as never;
    };

    const { result } = renderHook(() =>
      useTradeController({
        runtime,
        restClientFactory,
        wsClientFactory,
      }),
    );

    await waitFor(() => {
      expect(result.current.state.bootstrapStatus).toBe("ready");
      expect(wsCallbacks).toBeDefined();
    });

    act(() => {
      wsCallbacks?.onSnapshot({
        marketId: "BTC-USD",
        sequence: 4,
        bids: [{ price: 100, quantity: 3 }],
        asks: [{ price: 101, quantity: 2 }],
      });
      wsCallbacks?.onAuthenticated({ traderId: "trader-1", teamNumber: "TEAM-ALICE" });
      wsCallbacks?.onResyncRequired({
        channel: "data",
        marketId: "BTC-USD",
        reason: "market sequence gap detected",
        autoHealing: true,
      });
    });

    await waitFor(() => {
      expect(bootstrapAccountData).toHaveBeenCalledTimes(1);
    });
    expect(result.current.derived.summary.bids).toEqual([
      { price: 100, liquidity: 3, total: 300 },
    ]);
    expect(result.current.derived.summary.asks).toEqual([
      { price: 101, liquidity: 2, total: 202 },
    ]);
    expect(
      result.current.state.messages.some((message) => message.text.includes("market sequence gap detected")),
    ).toBe(false);

    act(() => {
      wsCallbacks?.onAuthenticated({ traderId: "trader-1", teamNumber: "TEAM-ALICE" });
    });

    await waitFor(() => {
      expect(bootstrapAccountData).toHaveBeenCalledTimes(2);
    });
    expect(bootstrapAccountData).toHaveBeenNthCalledWith(2, {
      markets: false,
      user: false,
      positions: true,
      openOrders: true,
      fills: true,
    });

    act(() => {
      wsCallbacks?.onResyncRequired({
        channel: "data",
        marketId: "BTC-USD",
        reason: "manual intervention required",
      });
    });

    await waitFor(() => {
      expect(
        result.current.state.messages.some((message) => message.text.includes("manual intervention required")),
      ).toBe(true);
    });
  });

  it("applies private fill and order-state updates without a full account resync", async () => {
    const bootstrapAccountData = vi.fn().mockResolvedValue({
      markets: runtime.markets,
      user: { traderId: "trader-1", teamNumber: "TEAM-ALICE" },
      positions: [],
      openOrders: [
        {
          id: "order-1",
          createdAt: "2026-03-17T09:30:00Z",
          marketId: "BTC-USD",
          marketName: "BTC-USD",
          side: "buy",
          shares: 2,
          limitPrice: 101,
          status: "open",
        },
      ],
      fills: [],
      warnings: [],
      loaded: {
        markets: true,
        user: true,
        positions: true,
        openOrders: true,
        fills: true,
      },
    });
    const restClientFactory = () =>
      ({
        bootstrapAccountData,
        submitOrder: vi.fn(),
        cancelOrder: vi.fn(),
      }) as never;
    let wsCallbacks: TradeWsCallbacks | undefined;
    const wsClientFactory = (_config: unknown, callbacks: TradeWsCallbacks) => {
      wsCallbacks = callbacks;
      return {
        connect: vi.fn(),
        disconnect: vi.fn(),
        updateMarket: vi.fn(),
      } as never;
    };

    const { result } = renderHook(() =>
      useTradeController({
        runtime,
        restClientFactory,
        wsClientFactory,
      }),
    );

    await waitFor(() => {
      expect(result.current.state.bootstrapStatus).toBe("ready");
      expect(wsCallbacks).toBeDefined();
    });

    act(() => {
      wsCallbacks?.onFill({
        fillId: "fill-1",
        market: "BTC-USD",
        makerOrderId: "order-1",
        takerOrderId: "aggressor-1",
        price: 101,
        quantity: 1,
        occurredAt: "2026-03-17T09:31:00Z",
      });
      wsCallbacks?.onOrderState({
        order: {
          id: "order-1",
          createdAt: "2026-03-17T09:30:00Z",
          marketId: "BTC-USD",
          marketName: "BTC-USD",
          side: "buy",
          shares: 1,
          limitPrice: 101,
          status: "partial",
        },
        status: "open",
      });
    });

    await waitFor(() => {
      expect(result.current.state.positionsByMarket["BTC-USD"]).toEqual({
        netQuantity: 1,
        avgCost: 101,
        realizedPnl: 0,
      });
      expect(result.current.state.pendingOrders).toEqual([
        expect.objectContaining({
          id: "order-1",
          shares: 1,
          status: "partial",
        }),
      ]);
    });

    expect(bootstrapAccountData).toHaveBeenCalledTimes(1);
  });
});
