export type MarketId = string;
export type TradeSide = "buy" | "sell";
export type PositionFilter = "active" | "pending";
export type OrderType = "limit" | "market";
export type MessageTone = "neutral" | "positive" | "negative";
export type ConnectionStatus =
  | "connecting"
  | "connected"
  | "reconnecting"
  | "disconnected";
export type BootstrapStatus = "idle" | "loading" | "ready" | "error";
export type MarketStatus = "enabled" | "disabled" | "settled";

export type MarketDefinition = {
  id: MarketId;
  name: string;
  baseAsset: string;
  quoteAsset: string;
  minPrice?: number | null;
  maxPrice?: number | null;
  status?: MarketStatus;
};

export type MarketBookLevel = {
  price: number;
  quantity: number;
};

export type MarketBookDelta =
  | { kind: "level_updated"; side: TradeSide; price: number; quantity: number }
  | {
      kind: "trade";
      price: number;
      quantity: number;
    };

export type MarketBookState = {
  marketId: MarketId;
  sequence: number;
  bids: MarketBookLevel[];
  asks: MarketBookLevel[];
  lastTradePrice: number | null;
  lastTradeQuantity: number | null;
};

export type MarketTrade = {
  marketId: MarketId;
  price: number;
  quantity: number;
  occurredAt: string;
};

export type PositionState = {
  netQuantity: number;
  avgCost: number | null;
  realizedPnl: number;
};

export type PendingOrder = {
  id: string;
  createdAt: string;
  marketId: MarketId;
  marketName: string;
  side: TradeSide;
  shares: number;
  limitPrice: number;
  status: "open" | "partial";
};

export type MessageEntry = {
  id: number;
  time: string;
  tone: MessageTone;
  text: string;
};

export type TradeUser = {
  traderId: string;
  teamNumber: string;
};

export type AccountPosition = {
  market: string;
  netQuantity: number;
  averageEntryPrice: number | null;
  realizedPnl: number;
};

export type TradeFill = {
  fillId: string;
  market: string;
  makerOrderId: string;
  takerOrderId: string;
  price: number;
  quantity: number;
  occurredAt: string;
};

export type TradeBootstrapData = {
  markets: MarketDefinition[];
  user: TradeUser | null;
  positions: AccountPosition[];
  openOrders: PendingOrder[];
  fills: TradeFill[];
  warnings: string[];
  loaded: {
    markets: boolean;
    user: boolean;
    positions: boolean;
    openOrders: boolean;
    fills: boolean;
  };
};

export type SubmitOrderIntent = {
  marketId: MarketId;
  marketName: string;
  side: TradeSide;
  orderType: OrderType;
  quantity: number;
  requestedPrice: number;
  effectivePrice: number;
};

export type SubmitOrderResult = {
  orderId: string;
  marketId: MarketId;
  marketName: string;
  side: TradeSide;
  orderType: OrderType;
  quantity: number;
  requestedPrice: number;
  effectivePrice: number;
  resting: boolean;
  remaining: number;
  fills: TradeFill[];
  createdAt: string;
};

export type PnlMetric = {
  label: string;
  value: string;
  tone: "positive" | "negative" | "neutral" | "primary";
};

export type AggregatedBookLevel = {
  price: number;
  liquidity: number;
  total: number;
};
