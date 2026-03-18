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
      JSON.stringify({ op: "subscribe", channel: "l3", market: "BTC-USD" }),
    ]);

    client.updateMarket("ETH-USD");
    expect(socket.sent.slice(2)).toEqual([
      JSON.stringify({ op: "unsubscribe", channel: "l3", market: "BTC-USD" }),
      JSON.stringify({ op: "subscribe", channel: "l3", market: "ETH-USD" }),
    ]);
  });

  it("maps snapshot and delta payloads into controller-friendly shapes", () => {
    const socket = new MockSocket();
    const callbacks = {
      onStatusChange: vi.fn(),
      onAuthenticated: vi.fn(),
      onSnapshot: vi.fn(),
      onDelta: vi.fn(),
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
        channel: "l3",
        market: "BTC-USD",
        sequence: 4,
        bids: [],
        asks: [
          {
            order_id: "ask-1",
            side: "SELL",
            price: 101,
            remaining: 2,
            created_at: "2026-03-17T09:30:00Z",
          },
        ],
      }),
    });
    socket.onmessage?.({
      data: JSON.stringify({
        type: "delta",
        channel: "l3",
        market: "BTC-USD",
        sequence: 5,
        events: [
          {
            kind: "trade",
            maker_order_id: "ask-1",
            taker_order_id: "bid-1",
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
          orderId: "ask-1",
          side: "sell",
          price: 101,
          remaining: 2,
          createdAt: "2026-03-17T09:30:00Z",
        },
      ],
    });
    expect(callbacks.onDelta).toHaveBeenCalledWith({
      marketId: "BTC-USD",
      sequence: 5,
      events: [
        {
          kind: "trade",
          makerOrderId: "ask-1",
          takerOrderId: "bid-1",
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
});
