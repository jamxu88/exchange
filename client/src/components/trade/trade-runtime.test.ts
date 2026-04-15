import { createTradeRuntimeConfig } from "@/components/trade/trade-runtime";

describe("createTradeRuntimeConfig", () => {
  it("does not inject placeholder markets when none are configured", () => {
    const runtime = createTradeRuntimeConfig({});

    expect(runtime.markets).toEqual([]);
  });

  it("parses configured market definitions from the environment", () => {
    const runtime = createTradeRuntimeConfig({
      NEXT_PUBLIC_EXCHANGE_MARKETS: "BTC-USD|Bitcoin,ETH-USD",
    });

    expect(runtime.markets).toEqual([
      {
        id: "BTC-USD",
        name: "Bitcoin",
        baseAsset: "BTC",
        quoteAsset: "USD",
        status: "enabled",
      },
      {
        id: "ETH-USD",
        name: "ETH-USD",
        baseAsset: "ETH",
        quoteAsset: "USD",
        status: "enabled",
      },
    ]);
  });
});
