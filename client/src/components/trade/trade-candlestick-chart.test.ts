import { buildCandlestickData } from "@/components/trade/trade-candlestick-chart";
import type { MarketTrade } from "@/components/trade/trade-types";

describe("buildCandlestickData", () => {
  it("groups trades into 10 second candles", () => {
    const trades: MarketTrade[] = [
      {
        marketId: "BTC-USD",
        price: 100,
        quantity: 1,
        occurredAt: "2026-03-25T14:00:01.000Z",
      },
      {
        marketId: "BTC-USD",
        price: 104,
        quantity: 2,
        occurredAt: "2026-03-25T14:00:08.000Z",
      },
      {
        marketId: "BTC-USD",
        price: 102,
        quantity: 1,
        occurredAt: "2026-03-25T14:00:12.000Z",
      },
    ];

    expect(buildCandlestickData(trades)).toEqual([
      {
        time: Math.floor(Date.parse("2026-03-25T14:00:00.000Z") / 1000),
        open: 100,
        high: 104,
        low: 100,
        close: 104,
      },
      {
        time: Math.floor(Date.parse("2026-03-25T14:00:10.000Z") / 1000),
        open: 102,
        high: 102,
        low: 102,
        close: 102,
      },
    ]);
  });
});
