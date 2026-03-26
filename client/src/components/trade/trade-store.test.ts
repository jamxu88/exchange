import {
  createInitialTradeState,
  parseNumberInput,
  sanitizeWholeNumberInput,
  selectPendingRows,
  selectPnlMetrics,
  selectSelectedMarketSummary,
  tradeReducer,
} from "@/components/trade/trade-store";
import type { MarketDefinition, SubmitOrderResult, TradeBootstrapData } from "@/components/trade/trade-types";

const markets: MarketDefinition[] = [
  { id: "BTC-USD", name: "BTC-USD", baseAsset: "BTC", quoteAsset: "USD" },
  { id: "ETH-USD", name: "ETH-USD", baseAsset: "ETH", quoteAsset: "USD" },
];

function bootstrapData(): TradeBootstrapData {
  return {
    markets,
    user: { traderId: "trader-1", username: "alice" },
    positions: [
      { market: "BTC-USD", netQuantity: 3, averageEntryPrice: 95, realizedPnl: 10 },
    ],
    openOrders: [
      {
        id: "order-1",
        createdAt: "2026-03-17T09:30:00Z",
        marketId: "BTC-USD",
        marketName: "BTC-USD",
        side: "buy",
        shares: 3,
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
  };
}

describe("tradeReducer", () => {
  it("sanitizes limit price input to whole numbers", () => {
    const initial = createInitialTradeState(markets);
    const next = tradeReducer(initial, {
      type: "set-limit-price",
      value: "1.2a0",
    });

    expect(next.limitPriceInput).toBe("1");
    expect(sanitizeWholeNumberInput("1.2a0")).toBe("1");
    expect(parseNumberInput("1.2a0")).toBe(1);
  });

  it("resolves pending rows to configured market names", () => {
    const initial = createInitialTradeState([
      { id: "BTC-USD", name: "Bitcoin", baseAsset: "BTC", quoteAsset: "USD" },
    ]);
    initial.pendingOrders = [
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
    ];

    expect(selectPendingRows(initial)).toEqual([
      expect.objectContaining({
        marketId: "BTC-USD",
        marketName: "Bitcoin",
      }),
    ]);
  });

  it("hydrates positions and open orders from bootstrap data", () => {
    const initial = createInitialTradeState(markets);
    const next = tradeReducer(initial, {
      type: "bootstrap-success",
      data: bootstrapData(),
      id: 1,
      time: "09:30:00",
    });

    expect(next.user?.username).toBe("alice");
    expect(next.positionsByMarket["BTC-USD"]).toEqual({
      netQuantity: 3,
      avgCost: 95,
      realizedPnl: 10,
    });
    expect(next.pendingOrders).toHaveLength(1);
    expect(next.bootstrapStatus).toBe("ready");
  });

  it("does not add a message when switching markets", () => {
    const initial = createInitialTradeState(markets);
    const next = tradeReducer(initial, {
      type: "select-market",
      marketId: "ETH-USD",
      id: 1,
      time: "09:30:00",
    });

    expect(next.selectedMarketId).toBe("ETH-USD");
    expect(next.messages).toEqual([]);
  });

  it("applies snapshots and deltas to the market book", () => {
    let state = createInitialTradeState(markets);
    state = tradeReducer(state, {
      type: "ws-snapshot",
      marketId: "BTC-USD",
      sequence: 3,
      bids: [{ price: 100, quantity: 2 }],
      asks: [{ price: 101, quantity: 4 }],
    });

    state = tradeReducer(state, {
      type: "ws-delta",
      marketId: "BTC-USD",
      sequence: 4,
      events: [
        {
          kind: "level_updated",
          side: "buy",
          price: 100,
          quantity: 5,
        },
        {
          kind: "trade",
          price: 101,
          quantity: 1,
        },
      ],
    });

    const summary = selectSelectedMarketSummary(state);
    expect(summary.bestBid).toBe(100);
    expect(summary.bestAsk).toBe(101);
    expect(summary.lastPrice).toBe(101);
    expect(summary.bids[0]).toEqual({
      price: 100,
      liquidity: 5,
      total: 500,
    });
  });

  it("updates positions and resting orders after submit success", () => {
    let state = createInitialTradeState(markets);
    state = tradeReducer(state, {
      type: "bootstrap-success",
      data: bootstrapData(),
      id: 1,
      time: "09:30:00",
    });

    const result: SubmitOrderResult = {
      orderId: "order-2",
      marketId: "BTC-USD",
      marketName: "BTC-USD",
      side: "buy",
      orderType: "limit",
      quantity: 4,
      requestedPrice: 105,
      effectivePrice: 100.6666666667,
      resting: true,
      remaining: 1,
      fills: [
        {
          fillId: "fill-1",
          market: "BTC-USD",
          makerOrderId: "resting-1",
          takerOrderId: "order-2",
          price: 100,
          quantity: 1,
          occurredAt: "2026-03-17T09:31:00Z",
        },
        {
          fillId: "fill-2",
          market: "BTC-USD",
          makerOrderId: "resting-2",
          takerOrderId: "order-2",
          price: 101,
          quantity: 2,
          occurredAt: "2026-03-17T09:31:01Z",
        },
      ],
      createdAt: "2026-03-17T09:31:00Z",
    };

    state = tradeReducer(state, { type: "submit-start" });
    state = tradeReducer(state, {
      type: "submit-success",
      result,
      id: 2,
      time: "09:31:00",
    });

    expect(state.isSubmitting).toBe(false);
    expect(state.positionsByMarket["BTC-USD"].netQuantity).toBe(6);
    expect(state.pendingOrders.find((order) => order.id === "order-2")?.shares).toBe(1);
    expect(state.pendingOrders.find((order) => order.id === "order-2")?.limitPrice).toBe(105);
    expect(state.filledOrders).toBe(1);
    expect(state.marketTradesByMarket["BTC-USD"]).toEqual([]);
    expect(state.messages.at(-1)?.text).toBe(
      "Accepted buy BTC-USD for 4 shares. Filled 3 at avg $100.67 and 1 remain resting at $105.00.",
    );
  });

  it("preserves existing pending orders when account sync cannot reload open orders", () => {
    let state = createInitialTradeState(markets);
    state = tradeReducer(state, {
      type: "bootstrap-success",
      data: bootstrapData(),
      id: 1,
      time: "09:30:00",
    });

    state = tradeReducer(state, {
      type: "account-sync",
      data: {
        ...bootstrapData(),
        openOrders: [],
        warnings: ["Open order bootstrap failed. per-user rate limit exceeded: max 500 ops per 10s"],
        loaded: {
          markets: true,
          user: true,
          positions: true,
          openOrders: false,
          fills: true,
        },
      },
    });

    expect(state.pendingOrders).toHaveLength(1);
    expect(state.pendingOrders[0]?.id).toBe("order-1");
  });

  it("removes only the canceled order on cancel success", () => {
    let state = createInitialTradeState(markets);
    state = tradeReducer(state, {
      type: "bootstrap-success",
      data: {
        ...bootstrapData(),
        openOrders: [
          bootstrapData().openOrders[0],
          {
            id: "order-2",
            createdAt: "2026-03-17T09:31:00Z",
            marketId: "ETH-USD",
            marketName: "ETH-USD",
            side: "sell",
            shares: 4,
            limitPrice: 202,
            status: "open",
          },
        ],
      },
      id: 1,
      time: "09:30:00",
    });

    state = tradeReducer(state, {
      type: "cancel-success",
      orderId: "order-1",
      id: 2,
      time: "09:31:00",
    });

    expect(state.pendingOrders.map((order) => order.id)).toEqual(["order-2"]);
  });

  it("assigns unique message ids even when incoming event ids collide", () => {
    let state = createInitialTradeState(markets);

    state = tradeReducer(state, {
      type: "bootstrap-error",
      error: "first",
      id: 5,
      time: "09:30:00",
    });
    state = tradeReducer(state, {
      type: "bootstrap-error",
      error: "second",
      id: 5,
      time: "09:30:00",
    });

    expect(state.messages).toHaveLength(2);
    expect(new Set(state.messages.map((message) => message.id)).size).toBe(2);
  });

  it("computes pnl metrics from known cost basis and live marks", () => {
    let state = createInitialTradeState(markets);
    state.positionsByMarket["BTC-USD"] = { netQuantity: 2, avgCost: 90, realizedPnl: 20 };
    state = tradeReducer(state, {
      type: "ws-snapshot",
      marketId: "BTC-USD",
      sequence: 1,
      bids: [{ price: 99, quantity: 2 }],
      asks: [{ price: 101, quantity: 2 }],
    });

    const metrics = selectPnlMetrics(state);
    expect(metrics[0].value).toBe("$20.00");
    expect(metrics[1].value).toBe("$20.00");
    expect(metrics[2].value).toBe("$40.00");
    expect(metrics[3]).toEqual({
      label: "Exposure",
      value: "$200.00",
      tone: "primary",
    });
    expect(metrics[4]).toEqual({
      label: "Open Orders",
      value: "0",
      tone: "neutral",
    });
  });
});
