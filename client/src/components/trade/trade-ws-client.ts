import type {
  ConnectionStatus,
  MarketBookDelta,
  MarketBookOrder,
  MarketId,
  PendingOrder,
  TradeFill,
  TradeSide,
} from "@/components/trade/trade-types";
import type { TradeRuntimeConfig } from "@/components/trade/trade-runtime";

type ApiSide = "BUY" | "SELL";

type RawClientMessage =
  | { op: "authenticate"; api_key: string }
  | { op: "subscribe"; channel: "l3"; market: string; last_sequence?: number | null }
  | { op: "unsubscribe"; channel: "l3"; market: string };

type RawL3Order = {
  order_id: string;
  side: ApiSide;
  price: number;
  remaining: number;
  created_at: string;
};

type RawBookDelta =
  | { kind: "order_added"; order: RawL3Order }
  | { kind: "order_updated"; order: RawL3Order }
  | { kind: "order_removed"; order_id: string; side: ApiSide; price: number }
  | {
      kind: "trade";
      maker_order_id: string;
      taker_order_id: string;
      price: number;
      quantity: number;
    };

type RawServerMessage =
  | { type: "heartbeat" }
  | { type: "authenticated"; trader_id: string; username: string }
  | {
      type: "snapshot";
      channel: "l3";
      market: string;
      sequence: number;
      bids: RawL3Order[];
      asks: RawL3Order[];
    }
  | {
      type: "delta";
      channel: "l3";
      market: string;
      sequence: number;
      events: RawBookDelta[];
    }
  | { type: "ack"; op: string; request_id?: string | null }
  | { type: "reject"; op: string; request_id?: string | null; code: string; message: string }
  | {
      type: "fill";
      fill: {
        fill_id: string;
        market: string;
        maker_order_id: string;
        taker_order_id: string;
        price: number;
        quantity: number;
        occurred_at: string;
      };
    }
  | {
      type: "order_state";
      order: {
        id: string;
        market: string;
        side: ApiSide;
        price: number;
        quantity: number;
        remaining: number;
        created_at: string;
      };
      status: "open" | "filled" | "canceled";
    }
  | {
      type: "admin_message";
      message: {
        level: "info" | "warning" | "critical";
        title?: string | null;
        body: string;
        market?: string | null;
      };
    }
  | {
      type: "resync_required";
      channel: string;
      market?: string | null;
      expected_sequence?: number | null;
      current_sequence?: number | null;
      reason: string;
    }
  | { type: "unsubscribed"; channel: "l3"; market: string }
  | { type: "error"; code: string; message: string };

export type TradeWsSnapshot = {
  marketId: MarketId;
  sequence: number;
  bids: MarketBookOrder[];
  asks: MarketBookOrder[];
};

export type TradeWsDelta = {
  marketId: MarketId;
  sequence: number;
  events: MarketBookDelta[];
};

export type TradeWsCallbacks = {
  onStatusChange: (status: ConnectionStatus) => void;
  onAuthenticated: (payload: { traderId: string; username: string }) => void;
  onSnapshot: (payload: TradeWsSnapshot) => void;
  onDelta: (payload: TradeWsDelta) => void;
  onReject: (payload: { op: string; code: string; message: string }) => void;
  onFill: (payload: TradeFill) => void;
  onOrderState: (payload: {
    order: PendingOrder;
    status: "open" | "filled" | "canceled";
  }) => void;
  onResyncRequired: (payload: { channel: string; marketId?: string; reason: string }) => void;
  onAdminMessage: (payload: {
    level: "info" | "warning" | "critical";
    title?: string;
    body: string;
    market?: string;
  }) => void;
  onError: (message: string) => void;
};

export type WebSocketLike = {
  onopen: (() => void) | null;
  onmessage: ((event: { data: string }) => void) | null;
  onerror: (() => void) | null;
  onclose: (() => void) | null;
  readyState: number;
  send: (data: string) => void;
  close: () => void;
};

export type WebSocketFactory = (url: string) => WebSocketLike;

function toTradeSide(side: ApiSide): TradeSide {
  return side === "BUY" ? "buy" : "sell";
}

function mapOrder(order: RawL3Order): MarketBookOrder {
  return {
    orderId: order.order_id,
    side: toTradeSide(order.side),
    price: order.price,
    remaining: order.remaining,
    createdAt: order.created_at,
  };
}

function mapPendingOrder(
  order: Extract<RawServerMessage, { type: "order_state" }>["order"],
): PendingOrder {
  return {
    id: order.id,
    createdAt: order.created_at,
    marketId: order.market,
    marketName: order.market,
    side: toTradeSide(order.side),
    shares: order.remaining,
    limitPrice: order.price,
    status: order.remaining < order.quantity ? "partial" : "open",
  };
}

function mapFill(fill: Extract<RawServerMessage, { type: "fill" }>["fill"]): TradeFill {
  return {
    fillId: fill.fill_id,
    market: fill.market,
    makerOrderId: fill.maker_order_id,
    takerOrderId: fill.taker_order_id,
    price: fill.price,
    quantity: fill.quantity,
    occurredAt: fill.occurred_at,
  };
}

function mapDelta(event: RawBookDelta): MarketBookDelta {
  switch (event.kind) {
    case "order_added":
      return { kind: "order_added", order: mapOrder(event.order) };
    case "order_updated":
      return { kind: "order_updated", order: mapOrder(event.order) };
    case "order_removed":
      return {
        kind: "order_removed",
        orderId: event.order_id,
        side: toTradeSide(event.side),
        price: event.price,
      };
    case "trade":
      return {
        kind: "trade",
        makerOrderId: event.maker_order_id,
        takerOrderId: event.taker_order_id,
        price: event.price,
        quantity: event.quantity,
      };
  }
}

export class TradeWsClient {
  private readonly url: string;
  private readonly apiKey?: string;
  private readonly reconnectDelayMs: number;
  private readonly callbacks: TradeWsCallbacks;
  private readonly createSocket: WebSocketFactory;
  private socket: WebSocketLike | null = null;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private selectedMarket: MarketId;
  private disposed = false;

  constructor(
    config: Pick<TradeRuntimeConfig, "wsUrl" | "apiKey" | "reconnectDelayMs"> & {
      initialMarket: MarketId;
    },
    callbacks: TradeWsCallbacks,
    createSocket?: WebSocketFactory,
  ) {
    this.url = config.wsUrl;
    this.apiKey = config.apiKey;
    this.reconnectDelayMs = config.reconnectDelayMs;
    this.callbacks = callbacks;
    this.selectedMarket = config.initialMarket;
    this.createSocket =
      createSocket ??
      ((url) =>
        new WebSocket(url) as unknown as WebSocketLike);
  }

  connect() {
    this.disposed = false;
    this.open();
  }

  disconnect() {
    this.disposed = true;
    this.clearReconnectTimer();
    this.socket?.close();
    this.socket = null;
    this.callbacks.onStatusChange("disconnected");
  }

  updateMarket(nextMarket: MarketId) {
    if (nextMarket === this.selectedMarket) {
      return;
    }

    const previousMarket = this.selectedMarket;
    this.selectedMarket = nextMarket;

    if (this.socket?.readyState === 1) {
      this.send({
        op: "unsubscribe",
        channel: "l3",
        market: previousMarket,
      });
      this.subscribeCurrentMarket();
    }
  }

  private open() {
    this.callbacks.onStatusChange(this.socket ? "reconnecting" : "connecting");
    const socket = this.createSocket(this.url);
    this.socket = socket;

    socket.onopen = () => {
      this.callbacks.onStatusChange("connected");
      if (this.apiKey) {
        this.send({ op: "authenticate", api_key: this.apiKey });
      }
      this.subscribeCurrentMarket();
    };

    socket.onmessage = (event) => {
      try {
        this.handleMessage(event.data);
      } catch (error) {
        this.callbacks.onError(
          error instanceof Error ? error.message : "Invalid websocket payload.",
        );
      }
    };

    socket.onerror = () => {
      this.callbacks.onError("Exchange websocket encountered a transport error.");
    };

    socket.onclose = () => {
      this.socket = null;
      if (this.disposed) {
        return;
      }

      this.callbacks.onStatusChange("reconnecting");
      this.clearReconnectTimer();
      this.reconnectTimer = setTimeout(() => this.open(), this.reconnectDelayMs);
    };
  }

  private handleMessage(raw: string) {
    const message = JSON.parse(raw) as RawServerMessage;

    switch (message.type) {
      case "heartbeat":
        return;
      case "authenticated":
        this.callbacks.onAuthenticated({
          traderId: message.trader_id,
          username: message.username,
        });
        return;
      case "snapshot":
        this.callbacks.onSnapshot({
          marketId: message.market,
          sequence: message.sequence,
          bids: message.bids.map(mapOrder),
          asks: message.asks.map(mapOrder),
        });
        return;
      case "delta":
        this.callbacks.onDelta({
          marketId: message.market,
          sequence: message.sequence,
          events: message.events.map(mapDelta),
        });
        return;
      case "ack":
        return;
      case "reject":
        this.callbacks.onReject({
          op: message.op,
          code: message.code,
          message: message.message,
        });
        return;
      case "fill":
        this.callbacks.onFill(mapFill(message.fill));
        return;
      case "order_state":
        this.callbacks.onOrderState({
          order: mapPendingOrder(message.order),
          status: message.status,
        });
        return;
      case "admin_message":
        this.callbacks.onAdminMessage({
          level: message.message.level,
          title: message.message.title ?? undefined,
          body: message.message.body,
          market: message.message.market ?? undefined,
        });
        return;
      case "resync_required":
        this.callbacks.onResyncRequired({
          channel: message.channel,
          marketId: message.market ?? undefined,
          reason: message.reason,
        });
        if (message.channel === "l3" && message.market === this.selectedMarket) {
          this.subscribeCurrentMarket();
        }
        return;
      case "unsubscribed":
        return;
      case "error":
        this.callbacks.onError(message.message);
        return;
    }
  }

  private subscribeCurrentMarket() {
    this.send({
      op: "subscribe",
      channel: "l3",
      market: this.selectedMarket,
      last_sequence: null,
    });
  }

  private send(message: RawClientMessage) {
    if (!this.socket || this.socket.readyState !== 1) {
      return;
    }

    this.socket.send(JSON.stringify(message));
  }

  private clearReconnectTimer() {
    if (!this.reconnectTimer) {
      return;
    }

    clearTimeout(this.reconnectTimer);
    this.reconnectTimer = null;
  }
}
