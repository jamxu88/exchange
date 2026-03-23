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
      bids: [
        {
          orderId: "bid-1",
          side: "buy",
          price: 100,
          remaining: 3,
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

  it("renders asks with the best ask closest to the spread", () => {
    const state = createInitialTradeState(runtime.markets);
    state.connectionStatus = "connected";
    state.marketBooks["BTC-USD"] = {
      marketId: "BTC-USD",
      sequence: 1,
      bids: [
        {
          orderId: "bid-1",
          side: "buy",
          price: 100,
          remaining: 1,
          createdAt: "2026-03-17T09:30:00Z",
        },
      ],
      asks: [
        {
          orderId: "ask-1",
          side: "sell",
          price: 201,
          remaining: 1,
          createdAt: "2026-03-17T09:30:00Z",
        },
        {
          orderId: "ask-2",
          side: "sell",
          price: 205,
          remaining: 1,
          createdAt: "2026-03-17T09:30:01Z",
        },
      ],
      lastTradePrice: 201,
      lastTradeQuantity: 1,
    };

    const { container } = render(
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
            submitOrder: vi.fn(),
          },
        }}
      />,
    );

    const askPriceSpans = Array.from(container.querySelectorAll("span"))
      .filter((element) => typeof element.className === "string")
      .filter((element) => element.className.includes("text-[#ff8181]"))
      .map((element) => element.textContent);

    expect(askPriceSpans).toEqual(["$205.00", "$201.00"]);
  });
});
