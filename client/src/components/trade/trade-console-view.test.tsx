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
    expect(screen.getAllByText("BTC-USD")).toHaveLength(2);
    expect(screen.getByText("Statistics")).toBeInTheDocument();
    expect(screen.getByText("Exposure")).toBeInTheDocument();
    expect(screen.getByText("Open Orders")).toBeInTheDocument();
    expect(screen.getByText("Market data connected.")).toBeInTheDocument();
    expect(screen.getAllByText("$101.00").length).toBeGreaterThan(0);

    await user.click(screen.getByRole("button", { name: "Open profile menu" }));

    expect(screen.getByText("alice")).toBeInTheDocument();
    expect(screen.getByText("Team 1")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Log out" })).toBeInTheDocument();
  });
});
