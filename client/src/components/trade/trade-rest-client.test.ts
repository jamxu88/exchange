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
    expect(snapshot.loaded).toEqual({
      markets: false,
      user: false,
      positions: false,
      openOrders: false,
      fills: false,
    });
  });

  it("bootstraps markets alongside account state", async () => {
    const responses = [
      [
        {
          market_id: "BTC-USD",
          display_name: "Bitcoin",
          base_asset: "BTC",
          quote_asset: "USD",
          status: "disabled",
        },
      ],
      { trader_id: "trader-1", team_number: "TEAM-ALICE" },
      [{ market: "BTC-USD", net_quantity: 2, average_entry_price: 100, realized_pnl: 5 }],
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
      {
        id: "BTC-USD",
        name: "Bitcoin",
        baseAsset: "BTC",
        quoteAsset: "USD",
        minPrice: null,
        maxPrice: null,
        status: "disabled",
      },
    ]);
    expect(snapshot.user?.teamNumber).toBe("TEAM-ALICE");
    expect(snapshot.loaded).toEqual({
      markets: true,
      user: true,
      positions: true,
      openOrders: true,
      fills: true,
    });
  });

  it("can refresh only user-stream state without refetching markets or user", async () => {
    const responses = [
      [{ market: "BTC-USD", net_quantity: 2, average_entry_price: 100, realized_pnl: 5 }],
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

    const snapshot = await client.bootstrapAccountData({
      positions: true,
      openOrders: true,
      fills: true,
    });

    expect(fetchMock).toHaveBeenCalledTimes(3);
    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      "http://localhost:8080/api/v1/positions",
      expect.any(Object),
    );
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      "http://localhost:8080/api/v1/open-orders",
      expect.any(Object),
    );
    expect(fetchMock).toHaveBeenNthCalledWith(
      3,
      "http://localhost:8080/api/v1/fills",
      expect.any(Object),
    );
    expect(snapshot.loaded).toEqual({
      markets: false,
      user: false,
      positions: true,
      openOrders: true,
      fills: true,
    });
    expect(snapshot.user).toBeNull();
    expect(snapshot.markets).toEqual([]);
  });

  it("marks open orders as not loaded when that specific bootstrap request fails", async () => {
    const responses = [
      { ok: true, payload: [{ market_id: "BTC-USD", display_name: "Bitcoin", base_asset: "BTC", quote_asset: "USD" }] },
      { ok: true, payload: { trader_id: "trader-1", team_number: "TEAM-ALICE" } },
      { ok: true, payload: [{ market: "BTC-USD", net_quantity: 2, average_entry_price: 100, realized_pnl: 5 }] },
      { ok: false, status: 429, payload: { error: "per-user rate limit exceeded: max 500 ops per 10s" } },
      { ok: true, payload: [] },
    ];
    const fetchMock = vi.fn().mockImplementation(async () => {
      const next = responses.shift();
      return {
        ok: next?.ok ?? true,
        status: next?.status ?? 200,
        text: async () => JSON.stringify(next?.payload ?? {}),
      };
    });
    const client = new TradeRestClient(
      { httpUrl: "http://localhost:8080", apiKey: "secret" },
      fetchMock as unknown as typeof fetch,
    );

    const snapshot = await client.bootstrapAccountData();

    expect(snapshot.openOrders).toEqual([]);
    expect(snapshot.loaded.openOrders).toBe(false);
    expect(snapshot.warnings).toContain(
      "Open order bootstrap failed. per-user rate limit exceeded: max 500 ops per 10s",
    );
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
        body: JSON.stringify({
          market: "BTC-USD",
          side: "BUY",
          order_type: "market",
          price: 0,
          quantity: 2,
        }),
        headers: expect.objectContaining({
          "x-api-key": "secret",
          "content-type": "application/json",
        }),
      }),
    );
    expect(result.effectivePrice).toBe(101);
  });

  it("returns the weighted execution price for aggressive limit orders", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      text: async () =>
        JSON.stringify({
          order: {
            id: "order-2",
            market: "BTC-USD",
            side: "BUY",
            price: 105,
            quantity: 4,
            remaining: 1,
            created_at: "2026-03-17T09:31:00Z",
          },
          fills: [
            {
              fill_id: "fill-1",
              market: "BTC-USD",
              maker_order_id: "maker-1",
              taker_order_id: "order-2",
              price: 100,
              quantity: 1,
              occurred_at: "2026-03-17T09:31:00Z",
            },
            {
              fill_id: "fill-2",
              market: "BTC-USD",
              maker_order_id: "maker-2",
              taker_order_id: "order-2",
              price: 101,
              quantity: 2,
              occurred_at: "2026-03-17T09:31:01Z",
            },
          ],
          resting: true,
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
      orderType: "limit",
      quantity: 4,
      requestedPrice: 105,
      effectivePrice: 105,
    });

    expect(result.effectivePrice).toBeCloseTo(100.6666666667);
    expect(result.requestedPrice).toBe(105);
    expect(result.remaining).toBe(1);
  });

  it("cancels an order with the expected auth header and path", async () => {
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
            remaining: 1,
            created_at: "2026-03-17T09:30:00Z",
          },
        }),
    });
    const client = new TradeRestClient(
      { httpUrl: "http://localhost:8080", apiKey: "secret" },
      fetchMock as unknown as typeof fetch,
    );

    const result = await client.cancelOrder("order-1");

    expect(fetchMock).toHaveBeenCalledWith(
      "http://localhost:8080/api/v1/orders/order-1",
      expect.objectContaining({
        method: "DELETE",
        headers: expect.objectContaining({
          "x-api-key": "secret",
        }),
      }),
    );
    expect(result).toEqual(
      expect.objectContaining({
        id: "order-1",
        shares: 1,
        limitPrice: 101,
      }),
    );
  });

  it("invokes fetch with the global context so browser bootstrap does not fail", async () => {
    const fetchMock = vi.fn(function (this: unknown) {
      return Promise.resolve({
        ok: true,
        text: async () => JSON.stringify({ trader_id: "trader-1", team_number: "TEAM-ALICE" }),
      });
    });
    const client = new TradeRestClient(
      { httpUrl: "http://localhost:8080", apiKey: "secret" },
      fetchMock as unknown as typeof fetch,
    );

    await expect(client["request"]("/api/v1/user")).resolves.toEqual({
      trader_id: "trader-1",
      team_number: "TEAM-ALICE",
    });
    expect(fetchMock.mock.contexts[0]).toBe(globalThis);
  });

  it("throws typed api errors for non-ok responses", async () => {
    const client = new TradeRestClient(
      { httpUrl: "http://localhost:8080", apiKey: "secret" },
      vi.fn().mockResolvedValue({
        ok: false,
        status: 409,
        text: async () =>
          JSON.stringify({
            error: "projected net position for BTC-USD would be 1005; limit is +/-1000",
          }),
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
        message: "projected net position for BTC-USD would be 1005; limit is +/-1000",
        status: 409,
      }),
    );
  });
});
