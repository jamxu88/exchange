import type {
  ConnectionStatus,
  MarketBookDelta,
  MarketBookLevel,
  MarketDefinition,
  MarketId,
  PendingOrder,
  TradeFill,
  TradeSide,
} from "@/components/trade/trade-types";
import type { TradeRuntimeConfig } from "@/components/trade/trade-runtime";

type ApiSide = "BUY" | "SELL";

type RawClientMessage =
  | { op: "authenticate"; api_key: string }
  | { op: "subscribe"; channel: "data"; market: string }
  | { op: "unsubscribe"; channel: "data"; market: string };

type RawBookLevel = {
  price: number;
  quantity: number;
};

type RawBookDelta =
  | { kind: "level_updated"; side: ApiSide; price: number; quantity: number }
  | {
      kind: "trade";
      price: number;
      quantity: number;
    };

type RawServerMessage =
  | { type: "heartbeat" }
  | { type: "authenticated"; trader_id: string; team_number: string }
  | {
      type: "snapshot";
      channel: "data";
      market: string;
      sequence: number;
      bids: RawBookLevel[];
      asks: RawBookLevel[];
    }
  | {
      type: "delta";
      channel: "data";
      market: string;
      start_sequence: number;
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
      type: "market_state";
      market: {
        market_id: string;
        display_name: string;
        base_asset: string;
        quote_asset: string;
        min_price?: number | null;
        max_price?: number | null;
        status: "enabled" | "disabled" | "settled";
      };
    }
  | {
      type: "market_deleted";
      market_id: string;
    }
  | {
      type: "resync_required";
      channel: string;
      market?: string | null;
      expected_sequence?: number | null;
      current_sequence?: number | null;
      reason: string;
    }
  | { type: "unsubscribed"; channel: "data"; market: string }
  | { type: "error"; code: string; message: string };

export type TradeWsSnapshot = {
  marketId: MarketId;
  sequence: number;
  bids: MarketBookLevel[];
  asks: MarketBookLevel[];
};

export type TradeWsDelta = {
  marketId: MarketId;
  sequence: number;
  events: MarketBookDelta[];
};

export type TradeWsCallbacks = {
  onStatusChange: (status: ConnectionStatus) => void;
  onAuthenticated: (payload: { traderId: string; teamNumber: string }) => void;
  onSnapshot: (payload: TradeWsSnapshot) => void;
  onDelta: (payload: TradeWsDelta) => void;
  onReject: (payload: { op: string; code: string; message: string }) => void;
  onFill: (payload: TradeFill) => void;
  onOrderState: (payload: {
    order: PendingOrder;
    status: "open" | "filled" | "canceled";
  }) => void;
  onMarketState: (payload: MarketDefinition) => void;
  onMarketDeleted: (payload: { marketId: MarketId }) => void;
  onResyncRequired: (payload: {
    channel: string;
    marketId?: string;
    reason: string;
    autoHealing?: boolean;
  }) => void;
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

function mapLevel(level: RawBookLevel): MarketBookLevel {
  return {
    price: level.price,
    quantity: level.quantity,
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

function mapMarketDefinition(
  market: Extract<RawServerMessage, { type: "market_state" }>["market"],
): MarketDefinition {
  return {
    id: market.market_id,
    name: market.display_name,
    baseAsset: market.base_asset,
    quoteAsset: market.quote_asset,
    minPrice: market.min_price ?? null,
    maxPrice: market.max_price ?? null,
    status: market.status,
  };
}

function mapDelta(event: RawBookDelta): MarketBookDelta {
  switch (event.kind) {
    case "level_updated":
      return {
        kind: "level_updated",
        side: toTradeSide(event.side),
        price: event.price,
        quantity: event.quantity,
      };
    case "trade":
      return {
        kind: "trade",
        price: event.price,
        quantity: event.quantity,
      };
  }
}

function hasMarketId(marketId: MarketId) {
  return marketId.trim().length > 0;
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
  private readonly marketSequences = new Map<MarketId, number>();
  private readonly pendingSnapshots = new Set<MarketId>();
  private readonly lastSnapshotRequestAt = new Map<MarketId, number>();
  private snapshotRetryTimer: ReturnType<typeof setTimeout> | null = null;
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
    this.clearSnapshotRetryTimer();
    this.marketSequences.clear();
    this.pendingSnapshots.clear();
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
    if (hasMarketId(previousMarket)) {
      this.pendingSnapshots.delete(previousMarket);
      this.marketSequences.delete(previousMarket);
    }

    if (this.socket?.readyState === 1) {
      if (hasMarketId(nextMarket)) {
        this.requestSnapshot(nextMarket);
      }
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
      if (hasMarketId(this.selectedMarket)) {
        this.requestSnapshot(this.selectedMarket);
      }
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
          teamNumber: message.team_number,
        });
        return;
      case "snapshot":
        if (!this.shouldApplySnapshot(message)) {
          return;
        }
        this.marketSequences.set(message.market, message.sequence);
        this.pendingSnapshots.delete(message.market);
        this.callbacks.onSnapshot({
          marketId: message.market,
          sequence: message.sequence,
          bids: message.bids.map(mapLevel),
          asks: message.asks.map(mapLevel),
        });
        return;
      case "delta":
        if (!this.shouldApplyDelta(message)) {
          return;
        }
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
      case "market_state":
        this.callbacks.onMarketState(mapMarketDefinition(message.market));
        return;
      case "market_deleted":
        this.pendingSnapshots.delete(message.market_id);
        this.marketSequences.delete(message.market_id);
        this.callbacks.onMarketDeleted({
          marketId: message.market_id,
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
      case "resync_required": {
        const autoHealing =
          message.channel === "data" && typeof message.market === "string";
        this.callbacks.onResyncRequired({
          channel: message.channel,
          marketId: message.market ?? undefined,
          reason: message.reason,
          autoHealing,
        });
        if (message.channel === "data" && message.market === this.selectedMarket) {
          this.requestSnapshot(message.market);
        }
        return;
      }
      case "unsubscribed":
        return;
      case "error":
        this.callbacks.onError(message.message);
        return;
    }
  }

  private requestSnapshot(marketId: MarketId) {
    if (!hasMarketId(marketId)) {
      return;
    }
    this.marketSequences.delete(marketId);
    this.pendingSnapshots.add(marketId);

    // If the socket isn't open, onopen will call requestSnapshot when the connection is established.
    if (this.socket?.readyState !== 1) {
      return;
    }

    const now = Date.now();
    const last = this.lastSnapshotRequestAt.get(marketId) ?? 0;
    const remaining = 250 - (now - last);

    if (remaining > 0) {
      // Throttled — prevent subscribe storms on spotty networks, but schedule a retry
      // so the client doesn't get stuck waiting for a snapshot that was never requested.
      this.clearSnapshotRetryTimer();
      this.snapshotRetryTimer = setTimeout(() => {
        this.snapshotRetryTimer = null;
        if (this.pendingSnapshots.has(marketId)) {
          this.lastSnapshotRequestAt.delete(marketId);
          this.requestSnapshot(marketId);
        }
      }, remaining + 10);
      return;
    }

    this.lastSnapshotRequestAt.set(marketId, now);
    this.send({
      op: "subscribe",
      channel: "data",
      market: marketId,
    });
  }

  private clearSnapshotRetryTimer() {
    if (!this.snapshotRetryTimer) {
      return;
    }
    clearTimeout(this.snapshotRetryTimer);
    this.snapshotRetryTimer = null;
  }

  // Resubscribe is intentionally avoided in favor of requesting a fresh snapshot.
  // This keeps the last known-good book visible and prevents flicker on spotty networks.

  private send(message: RawClientMessage) {
    if (!this.socket || this.socket.readyState !== 1) {
      return;
    }

    this.socket.send(JSON.stringify(message));
  }

  private shouldApplySnapshot(message: Extract<RawServerMessage, { type: "snapshot" }>) {
    const previousSequence = this.marketSequences.get(message.market);
    return previousSequence === undefined || message.sequence >= previousSequence;
  }

  private shouldApplyDelta(message: Extract<RawServerMessage, { type: "delta" }>) {
    const previousSequence = this.marketSequences.get(message.market);
    if (previousSequence === undefined) {
      return !this.pendingSnapshots.has(message.market);
    }
    if (previousSequence !== undefined) {
      if (message.sequence <= previousSequence) {
        return false;
      }
      if (message.start_sequence !== previousSequence + 1) {
        this.callbacks.onResyncRequired({
          channel: message.channel,
          marketId: message.market,
          reason: "market sequence gap detected client-side; resubscribing for a fresh snapshot",
          autoHealing: true,
        });
        this.requestSnapshot(message.market);
        return false;
      }
    }

    this.marketSequences.set(message.market, message.sequence);
    return true;
  }

  private clearReconnectTimer() {
    if (!this.reconnectTimer) {
      return;
    }

    clearTimeout(this.reconnectTimer);
    this.reconnectTimer = null;
  }
}
