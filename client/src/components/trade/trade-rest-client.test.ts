import { ExchangeApiError, TradeRestClient } from "@/components/trade/trade-rest-client";

describe("TradeRestClient", () => {
  it("skips bootstrap without an api key", async () => {
    const fetchMock = vi.fn();
    const client = new TradeRestClient(
      { httpUrl: "http://localhost:8080", apiKey: undefined },
      fetchMock as unknown as typeof fetch,
    );

    const snapshot = await client.bootstrapAccountData();

    expect(fetchMock).not.toHaveBeenCalled();
    expect(snapshot.warnings).toEqual([
      "No exchange API key configured. Account bootstrap skipped.",
    ]);
    expect(snapshot.markets).toEqual([]);
  });

  it("bootstraps markets alongside account state", async () => {
    const responses = [
      [{ market_id: "BTC-USD", display_name: "Bitcoin", base_asset: "BTC", quote_asset: "USD" }],
      { trader_id: "trader-1", username: "alice" },
      [{ asset: "USD", free: 1000, locked: 0 }],
      [],
      [],
    ];
    const fetchMock = vi.fn().mockImplementation(async () => ({
      ok: true,
      text: async () => JSON.stringify(responses.shift()),
    }));
    const client = new TradeRestClient(
      { httpUrl: "http://localhost:8080", apiKey: "secret" },
      fetchMock as unknown as typeof fetch,
    );

    const snapshot = await client.bootstrapAccountData();

    expect(snapshot.markets).toEqual([
      { id: "BTC-USD", name: "Bitcoin", baseAsset: "BTC", quoteAsset: "USD" },
    ]);
    expect(snapshot.user?.username).toBe("alice");
  });

  it("submits an order with the expected auth header and payload", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      text: async () =>
        JSON.stringify({
          order: {
            id: "order-1",
            market: "BTC-USD",
            side: "BUY",
            price: 101,
            quantity: 2,
            remaining: 0,
            created_at: "2026-03-17T09:30:00Z",
          },
          fills: [],
          resting: false,
        }),
    });
    const client = new TradeRestClient(
      { httpUrl: "http://localhost:8080", apiKey: "secret" },
      fetchMock as unknown as typeof fetch,
    );

    const result = await client.submitOrder({
      marketId: "BTC-USD",
      marketName: "BTC-USD",
      side: "buy",
      orderType: "market",
      quantity: 2,
      requestedPrice: 0,
      effectivePrice: 101,
    });

    expect(fetchMock).toHaveBeenCalledWith(
      "http://localhost:8080/api/v1/orders",
      expect.objectContaining({
        method: "POST",
        headers: expect.objectContaining({
          "x-api-key": "secret",
          "content-type": "application/json",
        }),
      }),
    );
    expect(result.syntheticMarket).toBe(true);
    expect(result.effectivePrice).toBe(101);
  });

  it("invokes fetch with the global context so browser bootstrap does not fail", async () => {
    const fetchMock = vi.fn(function (this: unknown) {
      return Promise.resolve({
        ok: true,
        text: async () => JSON.stringify({ trader_id: "trader-1", username: "alice" }),
      });
    });
    const client = new TradeRestClient(
      { httpUrl: "http://localhost:8080", apiKey: "secret" },
      fetchMock as unknown as typeof fetch,
    );

    await expect(client["request"]("/api/v1/user")).resolves.toEqual({
      trader_id: "trader-1",
      username: "alice",
    });
    expect(fetchMock.mock.contexts[0]).toBe(globalThis);
  });

  it("throws typed api errors for non-ok responses", async () => {
    const client = new TradeRestClient(
      { httpUrl: "http://localhost:8080", apiKey: "secret" },
      vi.fn().mockResolvedValue({
        ok: false,
        status: 409,
        text: async () => JSON.stringify({ error: "insufficient balance for asset USD" }),
      }) as unknown as typeof fetch,
    );

    await expect(
      client.submitOrder({
        marketId: "BTC-USD",
        marketName: "BTC-USD",
        side: "buy",
        orderType: "limit",
        quantity: 2,
        requestedPrice: 101,
        effectivePrice: 101,
      }),
    ).rejects.toEqual(
      expect.objectContaining<Partial<ExchangeApiError>>({
        message: "insufficient balance for asset USD",
        status: 409,
      }),
    );
  });
});
