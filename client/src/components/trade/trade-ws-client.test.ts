import { TradeWsClient } from "@/components/trade/trade-ws-client";

class MockSocket {
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: string }) => void) | null = null;
  onerror: (() => void) | null = null;
  onclose: (() => void) | null = null;
  readyState = 0;
  sent: string[] = [];

  send(data: string) {
    this.sent.push(data);
  }

  close() {
    this.readyState = 3;
  }
}

describe("TradeWsClient", () => {
  it("authenticates, subscribes, and switches markets", () => {
    const socket = new MockSocket();
    const callbacks = {
      onStatusChange: vi.fn(),
      onAuthenticated: vi.fn(),
      onSnapshot: vi.fn(),
      onDelta: vi.fn(),
      onReject: vi.fn(),
      onFill: vi.fn(),
      onOrderState: vi.fn(),
      onMarketState: vi.fn(),
      onResyncRequired: vi.fn(),
      onAdminMessage: vi.fn(),
      onError: vi.fn(),
    };
    const client = new TradeWsClient(
      {
        wsUrl: "ws://localhost:8080/ws",
        apiKey: "secret",
        reconnectDelayMs: 1000,
        initialMarket: "BTC-USD",
      },
      callbacks,
      () => socket,
    );

    client.connect();
    socket.readyState = 1;
    socket.onopen?.();

    expect(socket.sent).toEqual([
      JSON.stringify({ op: "authenticate", api_key: "secret" }),
      JSON.stringify({ op: "subscribe", channel: "l2", market: "BTC-USD", last_sequence: null }),
    ]);

    client.updateMarket("ETH-USD");
    expect(socket.sent.slice(2)).toEqual([
      JSON.stringify({ op: "unsubscribe", channel: "l2", market: "BTC-USD" }),
      JSON.stringify({ op: "subscribe", channel: "l2", market: "ETH-USD", last_sequence: null }),
    ]);
  });

  it("maps snapshot and delta payloads into controller-friendly shapes", () => {
    const socket = new MockSocket();
    const callbacks = {
      onStatusChange: vi.fn(),
      onAuthenticated: vi.fn(),
      onSnapshot: vi.fn(),
      onDelta: vi.fn(),
      onReject: vi.fn(),
      onFill: vi.fn(),
      onOrderState: vi.fn(),
      onMarketState: vi.fn(),
      onResyncRequired: vi.fn(),
      onAdminMessage: vi.fn(),
      onError: vi.fn(),
    };
    const client = new TradeWsClient(
      {
        wsUrl: "ws://localhost:8080/ws",
        apiKey: undefined,
        reconnectDelayMs: 1000,
        initialMarket: "BTC-USD",
      },
      callbacks,
      () => socket,
    );

    client.connect();
    socket.onmessage?.({
      data: JSON.stringify({
        type: "snapshot",
        channel: "l2",
        market: "BTC-USD",
        sequence: 4,
        bids: [],
        asks: [
          {
            price: 101,
            quantity: 2,
          },
        ],
      }),
    });
    socket.onmessage?.({
      data: JSON.stringify({
        type: "delta",
        channel: "l2",
        market: "BTC-USD",
        sequence: 5,
        events: [
          {
            kind: "trade",
            price: 101,
            quantity: 1,
          },
        ],
      }),
    });

    expect(callbacks.onSnapshot).toHaveBeenCalledWith({
      marketId: "BTC-USD",
      sequence: 4,
      bids: [],
      asks: [
        {
          price: 101,
          quantity: 2,
        },
      ],
    });
    expect(callbacks.onDelta).toHaveBeenCalledWith({
      marketId: "BTC-USD",
      sequence: 5,
      events: [
        {
          kind: "trade",
          price: 101,
          quantity: 1,
        },
      ],
    });
  });

  it("reconnects after unexpected socket closes", () => {
    vi.useFakeTimers();
    const firstSocket = new MockSocket();
    const secondSocket = new MockSocket();
    const factory = vi
      .fn()
      .mockReturnValueOnce(firstSocket)
      .mockReturnValueOnce(secondSocket);

    const client = new TradeWsClient(
      {
        wsUrl: "ws://localhost:8080/ws",
        apiKey: undefined,
        reconnectDelayMs: 750,
        initialMarket: "BTC-USD",
      },
      {
        onStatusChange: vi.fn(),
        onAuthenticated: vi.fn(),
        onSnapshot: vi.fn(),
        onDelta: vi.fn(),
        onReject: vi.fn(),
        onFill: vi.fn(),
        onOrderState: vi.fn(),
        onMarketState: vi.fn(),
        onResyncRequired: vi.fn(),
        onAdminMessage: vi.fn(),
        onError: vi.fn(),
      },
      factory,
    );

    client.connect();
    firstSocket.onclose?.();
    vi.advanceTimersByTime(750);

    expect(factory).toHaveBeenCalledTimes(2);
    vi.useRealTimers();
  });

  it("maps user and admin event payloads", () => {
    const socket = new MockSocket();
    const callbacks = {
      onStatusChange: vi.fn(),
      onAuthenticated: vi.fn(),
      onSnapshot: vi.fn(),
      onDelta: vi.fn(),
      onReject: vi.fn(),
      onFill: vi.fn(),
      onOrderState: vi.fn(),
      onMarketState: vi.fn(),
      onResyncRequired: vi.fn(),
      onAdminMessage: vi.fn(),
      onError: vi.fn(),
    };
    const client = new TradeWsClient(
      {
        wsUrl: "ws://localhost:8080/ws",
        apiKey: "secret",
        reconnectDelayMs: 1000,
        initialMarket: "BTC-USD",
      },
      callbacks,
      () => socket,
    );

    client.connect();
    socket.readyState = 1;
    socket.onmessage?.({
      data: JSON.stringify({
        type: "reject",
        op: "submit_order",
        code: "market_disabled",
        message: "market is disabled",
      }),
    });
    socket.onmessage?.({
      data: JSON.stringify({
        type: "fill",
        fill: {
          fill_id: "fill-1",
          market: "BTC-USD",
          maker_order_id: "maker-1",
          taker_order_id: "taker-1",
          price: 101,
          quantity: 2,
          occurred_at: "2026-03-17T09:30:00Z",
        },
      }),
    });
    socket.onmessage?.({
      data: JSON.stringify({
        type: "order_state",
        order: {
          id: "order-1",
          market: "BTC-USD",
          side: "BUY",
          price: 101,
          quantity: 3,
          remaining: 1,
          created_at: "2026-03-17T09:30:00Z",
        },
        status: "open",
      }),
    });
    socket.onmessage?.({
      data: JSON.stringify({
        type: "market_state",
        market: {
          market_id: "BTC-USD",
          display_name: "Bitcoin",
          base_asset: "BTC",
          quote_asset: "USD",
          status: "disabled",
        },
      }),
    });
    socket.onmessage?.({
      data: JSON.stringify({
        type: "admin_message",
        message: {
          level: "warning",
          title: "Desk notice",
          body: "Trading will pause soon.",
          market: "BTC-USD",
        },
      }),
    });
    socket.onmessage?.({
      data: JSON.stringify({
        type: "resync_required",
        channel: "l2",
        market: "BTC-USD",
        reason: "market sequence gap detected",
      }),
    });

    expect(callbacks.onReject).toHaveBeenCalledWith({
      op: "submit_order",
      code: "market_disabled",
      message: "market is disabled",
    });
    expect(callbacks.onFill).toHaveBeenCalledWith({
      fillId: "fill-1",
      market: "BTC-USD",
      makerOrderId: "maker-1",
      takerOrderId: "taker-1",
      price: 101,
      quantity: 2,
      occurredAt: "2026-03-17T09:30:00Z",
    });
    expect(callbacks.onOrderState).toHaveBeenCalledWith({
      order: {
        id: "order-1",
        createdAt: "2026-03-17T09:30:00Z",
        marketId: "BTC-USD",
        marketName: "BTC-USD",
        side: "buy",
        shares: 1,
        limitPrice: 101,
        status: "partial",
      },
      status: "open",
    });
    expect(callbacks.onMarketState).toHaveBeenCalledWith({
      id: "BTC-USD",
      name: "Bitcoin",
      baseAsset: "BTC",
      quoteAsset: "USD",
      status: "disabled",
    });
    expect(callbacks.onAdminMessage).toHaveBeenCalledWith({
      level: "warning",
      title: "Desk notice",
      body: "Trading will pause soon.",
      market: "BTC-USD",
    });
    expect(callbacks.onResyncRequired).toHaveBeenCalledWith({
      channel: "l2",
      marketId: "BTC-USD",
      reason: "market sequence gap detected",
    });
    expect(socket.sent.at(-1)).toBe(
      JSON.stringify({ op: "subscribe", channel: "l2", market: "BTC-USD", last_sequence: null }),
    );
  });
});
