"use client";

import type { ComponentProps } from "react";
import { TradeConsoleView } from "@/components/trade/trade-console";
import type { TradeRuntimeConfig } from "@/components/trade/trade-runtime";
import { createInitialTradeState } from "@/components/trade/trade-store";

const previewRuntime: TradeRuntimeConfig = {
  httpUrl: "http://localhost:8080",
  wsUrl: "ws://localhost:8080/ws",
  apiKey: "preview-key",
  reconnectDelayMs: 1_000,
  markets: [
    { id: "BTC-USD", name: "BTC-USD", baseAsset: "BTC", quoteAsset: "USD", status: "enabled" },
    { id: "ETH-USD", name: "ETH-USD", baseAsset: "ETH", quoteAsset: "USD", status: "disabled" },
    { id: "SOL-USD", name: "SOL-USD", baseAsset: "SOL", quoteAsset: "USD", status: "settled" },
  ],
};

type PreviewController = ComponentProps<typeof TradeConsoleView>["controller"];

export function createTradeConsolePreviewController(): PreviewController {
  const state = createInitialTradeState(previewRuntime.markets);
  state.connectionStatus = "connected";
  state.selectedMarketId = "BTC-USD";
  state.user = { traderId: "preview-trader-42", teamNumber: "TEAM-PREVIEW" };
  state.positionsByMarket["BTC-USD"] = {
    netQuantity: 12,
    avgCost: 98,
    realizedPnl: 245,
  };
  state.positionsByMarket["ETH-USD"] = {
    netQuantity: -6,
    avgCost: 101,
    realizedPnl: -40,
  };
  state.pendingOrders = [
    {
      id: "preview-order-1",
      createdAt: "2026-03-25T10:00:00Z",
      marketId: "BTC-USD",
      marketName: "BTC-USD",
      side: "buy",
      shares: 8,
      limitPrice: 99,
      status: "open",
    },
    {
      id: "preview-order-2",
      createdAt: "2026-03-25T10:00:10Z",
      marketId: "BTC-USD",
      marketName: "BTC-USD",
      side: "sell",
      shares: 4,
      limitPrice: 106,
      status: "open",
    },
  ];
  state.marketTradesByMarket["BTC-USD"] = [
    { marketId: "BTC-USD", price: 99, quantity: 3, occurredAt: "2026-03-25T09:56:12Z" },
    { marketId: "BTC-USD", price: 100, quantity: 2, occurredAt: "2026-03-25T09:56:44Z" },
    { marketId: "BTC-USD", price: 101, quantity: 5, occurredAt: "2026-03-25T09:57:08Z" },
    { marketId: "BTC-USD", price: 103, quantity: 4, occurredAt: "2026-03-25T09:57:41Z" },
    { marketId: "BTC-USD", price: 102, quantity: 2, occurredAt: "2026-03-25T09:58:05Z" },
    { marketId: "BTC-USD", price: 104, quantity: 1, occurredAt: "2026-03-25T09:58:29Z" },
    { marketId: "BTC-USD", price: 103, quantity: 3, occurredAt: "2026-03-25T09:59:02Z" },
    { marketId: "BTC-USD", price: 101, quantity: 6, occurredAt: "2026-03-25T09:59:47Z" },
    { marketId: "BTC-USD", price: 102, quantity: 2, occurredAt: "2026-03-25T10:00:16Z" },
  ];
  state.messages = [
    {
      id: 1,
      time: "10:00:03",
      tone: "positive",
      text: "Authenticated and subscribed to BTC-USD.",
    },
    {
      id: 2,
      time: "10:00:09",
      tone: "neutral",
      text: "Order book snapshot synchronized.",
    },
    {
      id: 3,
      time: "10:00:16",
      tone: "negative",
      text: "Sell limit order partially filled for 2 shares.",
    },
  ];
  state.marketBooks["BTC-USD"] = {
    marketId: "BTC-USD",
    sequence: 12,
    bids: [
      { price: 101, quantity: 12 },
      { price: 100, quantity: 8 },
      { price: 99, quantity: 5 },
    ],
    asks: [
      { price: 102, quantity: 6 },
      { price: 103, quantity: 9 },
      { price: 104, quantity: 11 },
    ],
    lastTradePrice: 102,
    lastTradeQuantity: 2,
  };

  return {
    runtime: previewRuntime,
    state,
    derived: {
      summary: {
        bids: [
          { price: 101, liquidity: 12, total: 1_212 },
          { price: 100, liquidity: 8, total: 800 },
          { price: 99, liquidity: 5, total: 495 },
        ],
        asks: [
          { price: 102, liquidity: 6, total: 612 },
          { price: 103, liquidity: 9, total: 927 },
          { price: 104, liquidity: 11, total: 1_144 },
        ],
        bestBid: 101,
        bestAsk: 102,
        buyQuote: 102,
        sellQuote: 101,
        lastPrice: 102,
        midPrice: 101.5,
        spread: 1,
      },
      estimated: {
        shares: 20,
        derivedPrice: 102,
        estimatedCost: 2_040,
      },
    },
    actions: {
      selectMarket: () => {},
      setSide: () => {},
      setPositionFilter: () => {},
      setOrderType: () => {},
      setLimitPrice: () => {},
      setShares: () => {},
      adjustShares: () => {},
      cancelPendingOrder: async () => {},
      submitOrder: async () => {},
    },
  };
}

export function TradeConsolePreview() {
  return <TradeConsoleView controller={createTradeConsolePreviewController()} />;
}
