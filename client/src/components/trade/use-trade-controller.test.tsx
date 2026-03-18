import { renderHook, act, waitFor } from "@testing-library/react";
import { useTradeController } from "@/components/trade/use-trade-controller";
import type { TradeRuntimeConfig } from "@/components/trade/trade-runtime";

const runtime: TradeRuntimeConfig = {
  httpUrl: "http://localhost:8080",
  wsUrl: "ws://localhost:8080/ws",
  apiKey: "secret",
  reconnectDelayMs: 1000,
  markets: [
    { id: "BTC-USD", name: "BTC-USD", baseAsset: "BTC", quoteAsset: "USD" },
    { id: "ETH-USD", name: "ETH-USD", baseAsset: "ETH", quoteAsset: "USD" },
  ],
};

describe("useTradeController", () => {
  it("bootstraps state and updates websocket subscriptions on market changes", async () => {
    const bootstrapAccountData = vi.fn().mockResolvedValue({
      markets: runtime.markets,
      user: { traderId: "trader-1", username: "alice" },
      balances: [{ asset: "BTC", free: 1, locked: 1 }],
      openOrders: [],
      fills: [],
      warnings: [],
    });
    const submitOrder = vi.fn();
    const updateMarket = vi.fn();
    const connect = vi.fn();
    const disconnect = vi.fn();
    const restClientFactory = () =>
      ({
        bootstrapAccountData,
        submitOrder,
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
    expect(result.current.state.user?.username).toBe("alice");

    act(() => {
      result.current.actions.selectMarket("ETH-USD");
    });

    expect(updateMarket).toHaveBeenCalledWith("ETH-USD");
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
      syntheticMarket: false,
    });
    const restClientFactory = () =>
      ({
        bootstrapAccountData: vi.fn().mockResolvedValue({
          markets: runtime.markets,
          user: null,
          balances: [],
          openOrders: [],
          fills: [],
          warnings: [],
        }),
        submitOrder,
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

  it("surfaces a bootstrap failure without retrying on every render", async () => {
    const bootstrapAccountData = vi.fn().mockRejectedValue(new Error("bootstrap failed"));
    const restClientFactory = () =>
      ({
        bootstrapAccountData,
        submitOrder: vi.fn(),
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
});
