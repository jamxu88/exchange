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

export type MarketDefinition = {
  id: MarketId;
  name: string;
  baseAsset: string;
  quoteAsset: string;
};

export type MarketBookOrder = {
  orderId: string;
  side: TradeSide;
  price: number;
  remaining: number;
  createdAt: string;
};

export type MarketBookDelta =
  | { kind: "order_added"; order: MarketBookOrder }
  | { kind: "order_updated"; order: MarketBookOrder }
  | { kind: "order_removed"; orderId: string; side: TradeSide; price: number }
  | {
      kind: "trade";
      makerOrderId: string;
      takerOrderId: string;
      price: number;
      quantity: number;
    };

export type MarketBookState = {
  marketId: MarketId;
  sequence: number;
  bids: MarketBookOrder[];
  asks: MarketBookOrder[];
  lastTradePrice: number | null;
  lastTradeQuantity: number | null;
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
  username: string;
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
  syntheticMarket: boolean;
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
