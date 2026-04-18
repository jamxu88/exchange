import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { TradeConsoleView } from "@/components/trade/trade-console";
import {
  DEFAULT_TRADE_KEYBINDS,
  TRADE_PREFERENCES_STORAGE_KEY,
} from "@/components/trade/trade-preferences";
import { createInitialTradeState } from "@/components/trade/trade-store";
import type { TradeRuntimeConfig } from "@/components/trade/trade-runtime";

const runtime: TradeRuntimeConfig = {
  httpUrl: "http://localhost:8080",
  wsUrl: "ws://localhost:8080/ws",
  apiKey: "secret",
  reconnectDelayMs: 1000,
  markets: [
    { id: "BTC-USD", name: "BTC-USD", baseAsset: "BTC", quoteAsset: "USD", status: "enabled" },
  ],
};

describe("TradeConsoleView", () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  it("renders live connection, market data, and messages from controller state", async () => {
    const state = createInitialTradeState(runtime.markets);
    state.connectionStatus = "connected";
    state.user = { traderId: "trader-1", teamNumber: "TEAM-ALICE" };
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
    expect(screen.queryByText("Sharpe")).not.toBeInTheDocument();
    expect(screen.getByText("Active")).toBeInTheDocument();
    expect(screen.getByText("Pending")).toBeInTheDocument();
    expect(screen.getByText("+4")).toBeInTheDocument();
    expect(screen.getByText(/B \$102\.00/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Cancel order order-1" })).toBeInTheDocument();
    expect(screen.getByText("Market data connected.")).toBeInTheDocument();
    expect(screen.getAllByText("$101.00").length).toBeGreaterThan(0);
    expect(screen.queryByRole("link", { name: "API Docs" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Open profile menu" }));

    expect(screen.getByText("TEAM-ALICE")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Settings" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Log out" })).toBeInTheDocument();
  });

  it("lets the user change ticket keybinds from settings", async () => {
    const state = createInitialTradeState(runtime.markets);
    state.connectionStatus = "connected";
    const setSide = vi.fn();

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

    await user.click(screen.getByRole("button", { name: "Open profile menu" }));
    await user.click(screen.getByRole("button", { name: "Settings" }));

    expect(screen.getByRole("button", { name: "Switch to light mode" })).toBeInTheDocument();

    const buyKeybindInput = screen.getByLabelText("Buy keybind");
    buyKeybindInput.focus();
    await user.keyboard("x");
    const sellKeybindInput = screen.getByLabelText("Sell keybind");
    sellKeybindInput.focus();
    await user.keyboard("c");
    await user.click(screen.getByRole("button", { name: "Save settings" }));

    expect(screen.getByText("(X)")).toBeInTheDocument();
    expect(screen.getByText("(C)")).toBeInTheDocument();

    await user.keyboard("c");

    expect(setSide).toHaveBeenCalledWith("sell");
    expect(
      JSON.parse(window.localStorage.getItem(TRADE_PREFERENCES_STORAGE_KEY) ?? "{}").keybinds,
    ).toMatchObject({
      ...DEFAULT_TRADE_KEYBINDS,
      buy: "X",
      sell: "C",
    });
  });

  it("plays the configured execution sound when a new fill arrives", async () => {
    const play = vi.fn().mockResolvedValue(undefined);
    class AudioMock {
      src: string;
      volume = 1;

      constructor(src: string) {
        this.src = src;
      }

      play() {
        return play();
      }
    }

    vi.stubGlobal("Audio", AudioMock);
    window.localStorage.setItem(
      TRADE_PREFERENCES_STORAGE_KEY,
      JSON.stringify({
        keybinds: {
          ...DEFAULT_TRADE_KEYBINDS,
          buy: "X",
        },
        executionSound: {
          name: "fill.wav",
          dataUrl: "data:audio/wav;base64,AAAA",
        },
      }),
    );

    const state = createInitialTradeState(runtime.markets);
    state.connectionStatus = "connected";
    const controller = {
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
    };

    const { rerender } = render(<TradeConsoleView controller={controller} />);

    await waitFor(() => {
      expect(screen.getByText("(X)")).toBeInTheDocument();
    });

    rerender(
      <TradeConsoleView
        controller={{
          ...controller,
          state: {
            ...state,
            fills: [
              {
                fillId: "fill-1",
                market: "BTC-USD",
                makerOrderId: "maker-1",
                takerOrderId: "taker-1",
                price: 101,
                quantity: 1,
                occurredAt: "2026-03-25T10:00:00Z",
              },
            ],
          },
        }}
      />,
    );

    await waitFor(() => {
      expect(play).toHaveBeenCalledTimes(1);
    });

    vi.unstubAllGlobals();
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

  it("distinguishes disabled markets and blocks ticket submission for them", () => {
    const state = createInitialTradeState([
      { id: "BTC-USD", name: "BTC-USD", baseAsset: "BTC", quoteAsset: "USD", status: "enabled" },
      { id: "ETH-USD", name: "ETH-USD", baseAsset: "ETH", quoteAsset: "USD", status: "disabled" },
    ]);
    state.connectionStatus = "connected";
    state.selectedMarketId = "ETH-USD";

    render(
      <TradeConsoleView
        controller={{
          runtime: {
            ...runtime,
            markets: state.availableMarkets,
          },
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

    expect(screen.getByRole("button", { name: /ETH-USD.*Disabled/i })).toBeInTheDocument();
    expect(screen.getByText("This market is disabled. New orders are unavailable.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Market Disabled/i })).toBeDisabled();
  });

  it("shows an explicit empty state when no markets are configured", () => {
    const state = createInitialTradeState([]);
    state.connectionStatus = "connected";
    const submitOrder = vi.fn();

    render(
      <TradeConsoleView
        controller={{
          runtime: {
            ...runtime,
            markets: [],
          },
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
            cancelPendingOrder: vi.fn(),
            submitOrder,
          },
        }}
      />,
    );

    expect(screen.getByText("No live markets")).toBeInTheDocument();
    expect(screen.getByText("No active market")).toBeInTheDocument();
    expect(
      screen.getByText(
        "No markets are available yet. Waiting for the exchange to publish market definitions.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /No Market Available/i })).toBeDisabled();
  });

  it("supports trade ticket keybinds", async () => {
    const marketRuntime: TradeRuntimeConfig = {
      ...runtime,
      markets: [
        { id: "BTC-USD", name: "BTC-USD", baseAsset: "BTC", quoteAsset: "USD", status: "enabled" },
        { id: "ETH-USD", name: "ETH-USD", baseAsset: "ETH", quoteAsset: "USD", status: "enabled" },
        { id: "SOL-USD", name: "SOL-USD", baseAsset: "SOL", quoteAsset: "USD", status: "enabled" },
      ],
    };
    const state = createInitialTradeState(marketRuntime.markets);
    state.connectionStatus = "connected";
    const setSide = vi.fn();
    const setOrderType = vi.fn();
    const submitOrder = vi.fn();
    const selectMarket = vi.fn();

    render(
      <TradeConsoleView
        controller={{
          runtime: marketRuntime,
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
            selectMarket,
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
    await user.keyboard("]");
    await user.keyboard("{[}");
    await user.keyboard("q");
    expect(screen.getByLabelText("Shares")).toHaveFocus();
    screen.getByLabelText("Shares").blur();
    await user.keyboard("p");
    expect(screen.getByLabelText("Limit Price")).toHaveFocus();
    await user.keyboard("{Enter}");

    expect(setSide).toHaveBeenCalledWith("sell");
    expect(setOrderType).toHaveBeenCalledWith("market");
    expect(setOrderType).toHaveBeenCalledWith("limit");
    expect(selectMarket).toHaveBeenCalledWith("ETH-USD");
    expect(selectMarket).toHaveBeenCalledWith("SOL-USD");
    expect(submitOrder).toHaveBeenCalled();
    expect(screen.getByText("(B)")).toBeInTheDocument();
    expect(screen.getByText("(S)")).toBeInTheDocument();
    expect(screen.getByText("Prev", { exact: false })).toBeInTheDocument();
    expect(screen.getByText("Next", { exact: false })).toBeInTheDocument();
    expect(screen.getByText("([)")).toBeInTheDocument();
    expect(screen.getByText("(])")).toBeInTheDocument();
    expect(screen.getByText("(Q)")).toBeInTheDocument();
    expect(screen.getByText("(P)")).toBeInTheDocument();
  });

  it("keeps ticket keybinds active while editing price and shares", async () => {
    const state = createInitialTradeState(runtime.markets);
    state.connectionStatus = "connected";
    const setSide = vi.fn();
    const setOrderType = vi.fn();
    const setShares = vi.fn();
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
            setShares,
            adjustShares: vi.fn(),
            cancelPendingOrder: vi.fn(),
            submitOrder,
          },
        }}
      />,
    );

    const user = userEvent.setup();

    await user.click(screen.getByLabelText("Shares"));
    await user.keyboard("9");
    await user.keyboard("s");
    await user.keyboard("m");
    await user.keyboard("{Enter}");

    expect(setShares).toHaveBeenCalled();
    expect(setSide).toHaveBeenCalledWith("sell");
    expect(setOrderType).toHaveBeenCalledWith("market");
    expect(submitOrder).toHaveBeenCalled();
  });

  it("renders asks with the best ask closest to the spread", () => {
    const state = createInitialTradeState(runtime.markets);
    state.connectionStatus = "connected";
    state.marketBooks["BTC-USD"] = {
      marketId: "BTC-USD",
      sequence: 1,
      bids: [{ price: 100, quantity: 1 }],
      asks: [
        { price: 201.99, quantity: 1 },
        { price: 205.75, quantity: 1 },
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
                { price: 201.99, liquidity: 1, total: 201.99 },
                { price: 205.75, liquidity: 1, total: 205.75 },
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
    const higherAskIndex = orderbookText.indexOf("$205");
    const bestAskIndex = orderbookText.indexOf("$201");

    expect(higherAskIndex).toBeGreaterThanOrEqual(0);
    expect(bestAskIndex).toBeGreaterThanOrEqual(0);
    expect(higherAskIndex).toBeLessThan(bestAskIndex);
    expect(orderbookText).not.toContain("201.99");
    expect(orderbookText).not.toContain("205.75");
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
