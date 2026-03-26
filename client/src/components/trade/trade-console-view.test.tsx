import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { TradeConsoleView } from "@/components/trade/trade-console";
import { createInitialTradeState } from "@/components/trade/trade-store";
import type { TradeRuntimeConfig } from "@/components/trade/trade-runtime";

const runtime: TradeRuntimeConfig = {
  httpUrl: "http://localhost:8080",
  wsUrl: "ws://localhost:8080/ws",
  apiKey: "secret",
  reconnectDelayMs: 1000,
  markets: [
    { id: "BTC-USD", name: "BTC-USD", baseAsset: "BTC", quoteAsset: "USD" },
  ],
};

describe("TradeConsoleView", () => {
  it("renders live connection, market data, and messages from controller state", async () => {
    const state = createInitialTradeState(runtime.markets);
    state.connectionStatus = "connected";
    state.user = { traderId: "trader-1", username: "alice" };
    state.marketTradesByMarket["BTC-USD"] = [
      { marketId: "BTC-USD", price: 100, quantity: 1, occurredAt: "2026-03-17T09:28:00Z" },
      { marketId: "BTC-USD", price: 101, quantity: 2, occurredAt: "2026-03-17T09:29:00Z" },
      { marketId: "BTC-USD", price: 102, quantity: 1, occurredAt: "2026-03-17T09:30:00Z" },
    ];
    state.positionsByMarket["BTC-USD"] = {
      netQuantity: 4,
      avgCost: 96,
      realizedPnl: 0,
    };
    state.pendingOrders = [
      {
        id: "order-1",
        createdAt: "2026-03-17T09:30:00Z",
        marketId: "BTC-USD",
        marketName: "BTC-USD",
        side: "buy",
        shares: 2,
        limitPrice: 102,
        status: "open",
      },
    ];
    state.messages = [
      {
        id: 1,
        time: "09:30:00",
        tone: "positive",
        text: "Market data connected.",
      },
    ];
    state.marketBooks["BTC-USD"] = {
      marketId: "BTC-USD",
      sequence: 1,
      bids: [{ price: 100, quantity: 3 }],
      asks: [{ price: 101, quantity: 2 }],
      lastTradePrice: 101,
      lastTradeQuantity: 1,
    };

    render(
      <TradeConsoleView
        controller={{
          runtime,
          state,
          derived: {
            summary: {
              bids: [{ price: 100, liquidity: 3, total: 300 }],
              asks: [{ price: 101, liquidity: 2, total: 202 }],
              bestBid: 100,
              bestAsk: 101,
              buyQuote: 101,
              sellQuote: 100,
              lastPrice: 101,
              midPrice: 100.5,
              spread: 1,
            },
            estimated: {
              shares: 20,
              derivedPrice: 101,
              estimatedCost: 2020,
            },
          },
          actions: {
            selectMarket: vi.fn(),
            setSide: vi.fn(),
            setPositionFilter: vi.fn(),
            setOrderType: vi.fn(),
            setLimitPrice: vi.fn(),
            setShares: vi.fn(),
            adjustShares: vi.fn(),
            cancelPendingOrder: vi.fn(),
            submitOrder: vi.fn(),
          },
        }}
      />,
    );

    const user = userEvent.setup();

    expect(screen.getByText("Connected")).toBeInTheDocument();
    expect(screen.getAllByText("BTC-USD").length).toBeGreaterThanOrEqual(2);
    expect(screen.getByText("Statistics")).toBeInTheDocument();
    expect(screen.getByText("Exposure")).toBeInTheDocument();
    expect(screen.getByText("Open Orders")).toBeInTheDocument();
    expect(screen.getByText("Active")).toBeInTheDocument();
    expect(screen.getByText("Pending")).toBeInTheDocument();
    expect(screen.getByText("+4")).toBeInTheDocument();
    expect(screen.getByText(/B \$102\.00/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Cancel order order-1" })).toBeInTheDocument();
    expect(screen.getByText("Market data connected.")).toBeInTheDocument();
    expect(screen.getAllByText("$101.00").length).toBeGreaterThan(0);
    expect(screen.getByRole("link", { name: "API Docs" })).toHaveAttribute(
      "href",
      "https://jamesxu.mintlify.app/",
    );

    await user.click(screen.getByRole("button", { name: "Open profile menu" }));

    expect(screen.getByText("alice")).toBeInTheDocument();
    expect(screen.getByText("Team 1")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Log out" })).toBeInTheDocument();
  });

  it("keeps the market depth panel in orderbook mode only", () => {
    const state = createInitialTradeState(runtime.markets);
    state.connectionStatus = "connected";

    render(
      <TradeConsoleView
        controller={{
          runtime,
          state,
          derived: {
            summary: {
              bids: [{ price: 100, liquidity: 1, total: 100 }],
              asks: [{ price: 101, liquidity: 1, total: 101 }],
              bestBid: 100,
              bestAsk: 101,
              buyQuote: 101,
              sellQuote: 100,
              lastPrice: 101,
              midPrice: 100.5,
              spread: 1,
            },
            estimated: {
              shares: 20,
              derivedPrice: 101,
              estimatedCost: 2020,
            },
          },
          actions: {
            selectMarket: vi.fn(),
            setSide: vi.fn(),
            setPositionFilter: vi.fn(),
            setOrderType: vi.fn(),
            setLimitPrice: vi.fn(),
            setShares: vi.fn(),
            adjustShares: vi.fn(),
            cancelPendingOrder: vi.fn(),
            submitOrder: vi.fn(),
          },
        }}
      />,
    );

    expect(screen.getByText("Live orderbook")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Candles" })).not.toBeInTheDocument();
    expect(screen.queryByTestId("candlestick-view")).not.toBeInTheDocument();
  });

  it("supports trade ticket keybinds", async () => {
    const state = createInitialTradeState(runtime.markets);
    state.connectionStatus = "connected";
    const setSide = vi.fn();
    const setOrderType = vi.fn();
    const submitOrder = vi.fn();

    render(
      <TradeConsoleView
        controller={{
          runtime,
          state,
          derived: {
            summary: {
              bids: [{ price: 100, liquidity: 1, total: 100 }],
              asks: [{ price: 101, liquidity: 1, total: 101 }],
              bestBid: 100,
              bestAsk: 101,
              buyQuote: 101,
              sellQuote: 100,
              lastPrice: 101,
              midPrice: 100.5,
              spread: 1,
            },
            estimated: {
              shares: 20,
              derivedPrice: 101,
              estimatedCost: 2020,
            },
          },
          actions: {
            selectMarket: vi.fn(),
            setSide,
            setPositionFilter: vi.fn(),
            setOrderType,
            setLimitPrice: vi.fn(),
            setShares: vi.fn(),
            adjustShares: vi.fn(),
            cancelPendingOrder: vi.fn(),
            submitOrder,
          },
        }}
      />,
    );

    const user = userEvent.setup();

    await user.keyboard("s");
    await user.keyboard("m");
    await user.keyboard("l");
    await user.keyboard("q");
    expect(screen.getByLabelText("Shares")).toHaveFocus();
    screen.getByLabelText("Shares").blur();
    await user.keyboard("p");
    expect(screen.getByLabelText("Limit Price")).toHaveFocus();
    await user.keyboard("{Enter}");

    expect(setSide).toHaveBeenCalledWith("sell");
    expect(setOrderType).toHaveBeenCalledWith("market");
    expect(setOrderType).toHaveBeenCalledWith("limit");
    expect(submitOrder).toHaveBeenCalled();
    expect(screen.getByText("(B)")).toBeInTheDocument();
    expect(screen.getByText("(S)")).toBeInTheDocument();
    expect(screen.getByText("(Q)")).toBeInTheDocument();
    expect(screen.getByText("(P)")).toBeInTheDocument();
  });

  it("renders asks with the best ask closest to the spread", () => {
    const state = createInitialTradeState(runtime.markets);
    state.connectionStatus = "connected";
    state.marketBooks["BTC-USD"] = {
      marketId: "BTC-USD",
      sequence: 1,
      bids: [{ price: 100, quantity: 1 }],
      asks: [
        { price: 201, quantity: 1 },
        { price: 205, quantity: 1 },
      ],
      lastTradePrice: 201,
      lastTradeQuantity: 1,
    };

    render(
      <TradeConsoleView
        controller={{
          runtime,
          state,
          derived: {
            summary: {
              bids: [{ price: 100, liquidity: 1, total: 100 }],
              asks: [
                { price: 201, liquidity: 1, total: 201 },
                { price: 205, liquidity: 1, total: 205 },
              ],
              bestBid: 100,
              bestAsk: 201,
              buyQuote: 201,
              sellQuote: 100,
              lastPrice: 201,
              midPrice: 150.5,
              spread: 101,
            },
            estimated: {
              shares: 20,
              derivedPrice: 201,
              estimatedCost: 4020,
            },
          },
          actions: {
            selectMarket: vi.fn(),
            setSide: vi.fn(),
            setPositionFilter: vi.fn(),
            setOrderType: vi.fn(),
            setLimitPrice: vi.fn(),
            setShares: vi.fn(),
            adjustShares: vi.fn(),
            cancelPendingOrder: vi.fn(),
            submitOrder: vi.fn(),
          },
        }}
      />,
    );

    const orderbookText = screen.getByTestId("orderbook-panel").textContent ?? "";
    const higherAskIndex = orderbookText.indexOf("$205.00");
    const bestAskIndex = orderbookText.indexOf("$201.00");

    expect(higherAskIndex).toBeGreaterThanOrEqual(0);
    expect(bestAskIndex).toBeGreaterThanOrEqual(0);
    expect(higherAskIndex).toBeLessThan(bestAskIndex);
  });

  it("cancels a pending order from the positions panel", async () => {
    const state = createInitialTradeState(runtime.markets);
    state.pendingOrders = [
      {
        id: "order-1",
        createdAt: "2026-03-17T09:30:00Z",
        marketId: "BTC-USD",
        marketName: "BTC-USD",
        side: "buy",
        shares: 2,
        limitPrice: 102,
        status: "open",
      },
    ];
    const cancelPendingOrder = vi.fn().mockResolvedValue(undefined);

    render(
      <TradeConsoleView
        controller={{
          runtime,
          state,
          derived: {
            summary: {
              bids: [],
              asks: [],
              bestBid: null,
              bestAsk: null,
              buyQuote: null,
              sellQuote: null,
              lastPrice: null,
              midPrice: null,
              spread: null,
            },
            estimated: {
              shares: 20,
              derivedPrice: 0,
              estimatedCost: 0,
            },
          },
          actions: {
            selectMarket: vi.fn(),
            setSide: vi.fn(),
            setPositionFilter: vi.fn(),
            setOrderType: vi.fn(),
            setLimitPrice: vi.fn(),
            setShares: vi.fn(),
            adjustShares: vi.fn(),
            cancelPendingOrder,
            submitOrder: vi.fn(),
          },
        }}
      />,
    );

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Cancel order order-1" }));

    expect(cancelPendingOrder).toHaveBeenCalledWith("order-1");
  });
});
