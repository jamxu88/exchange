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

  it("applies snapshots and deltas to the market book", () => {
    let state = createInitialTradeState(markets);
    state = tradeReducer(state, {
      type: "ws-snapshot",
      marketId: "BTC-USD",
      sequence: 3,
      bids: [
        {
          orderId: "bid-1",
          side: "buy",
          price: 100,
          remaining: 2,
          createdAt: "2026-03-17T09:30:00Z",
        },
      ],
      asks: [
        {
          orderId: "ask-1",
          side: "sell",
          price: 101,
          remaining: 4,
          createdAt: "2026-03-17T09:30:00Z",
        },
      ],
    });

    state = tradeReducer(state, {
      type: "ws-delta",
      marketId: "BTC-USD",
      sequence: 4,
      events: [
        {
          kind: "order_added",
          order: {
            orderId: "bid-2",
            side: "buy",
            price: 100,
            remaining: 3,
            createdAt: "2026-03-17T09:30:01Z",
          },
        },
        {
          kind: "trade",
          makerOrderId: "ask-1",
          takerOrderId: "bid-2",
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
      requestedPrice: 101,
      effectivePrice: 101,
      resting: true,
      remaining: 1,
      fills: [
        {
          fillId: "fill-1",
          market: "BTC-USD",
          makerOrderId: "resting-1",
          takerOrderId: "order-2",
          price: 101,
          quantity: 3,
          occurredAt: "2026-03-17T09:31:00Z",
        },
      ],
      createdAt: "2026-03-17T09:31:00Z",
      syntheticMarket: false,
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
    expect(state.filledOrders).toBe(1);
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
      bids: [
        {
          orderId: "bid-1",
          side: "buy",
          price: 99,
          remaining: 2,
          createdAt: "2026-03-17T09:30:00Z",
        },
      ],
      asks: [
        {
          orderId: "ask-1",
          side: "sell",
          price: 101,
          remaining: 2,
          createdAt: "2026-03-17T09:30:00Z",
        },
      ],
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
