import type {
  AggregatedBookLevel,
  ConnectionStatus,
  MarketBookDelta,
  MarketBookLevel,
  MarketBookState,
  MarketDefinition,
  MarketId,
  MarketTrade,
  MessageEntry,
  OrderType,
  PendingOrder,
  PnlMetric,
  PositionFilter,
  PositionState,
  SubmitOrderResult,
  TradeBootstrapData,
  TradeFill,
  TradeSide,
  TradeUser,
} from "@/components/trade/trade-types";

export type TradeState = {
  availableMarkets: MarketDefinition[];
  selectedMarketId: MarketId;
  connectionStatus: ConnectionStatus;
  bootstrapStatus: "idle" | "loading" | "ready" | "error";
  user: TradeUser | null;
  positions: TradeBootstrapData["positions"];
  positionsByMarket: Record<MarketId, PositionState>;
  pendingOrders: PendingOrder[];
  knownOrderSides: Record<string, TradeSide>;
  fills: TradeFill[];
  marketBooks: Record<MarketId, MarketBookState>;
  marketTradesByMarket: Record<MarketId, MarketTrade[]>;
  ticketSide: TradeSide;
  positionFilter: PositionFilter;
  orderType: OrderType;
  limitPriceInput: string;
  sharesInput: string;
  messages: MessageEntry[];
  submittedOrders: number;
  filledOrders: number;
  isSubmitting: boolean;
};

export type TradeAction =
  | { type: "hydrate-messages"; messages: MessageEntry[] }
  | { type: "select-market"; marketId: MarketId; id: number; time: string }
  | { type: "set-side"; side: TradeSide }
  | { type: "set-position-filter"; filter: PositionFilter }
  | { type: "set-order-type"; orderType: OrderType }
  | { type: "set-limit-price"; value: string }
  | { type: "set-shares"; value: string }
  | { type: "adjust-shares"; delta: number }
  | { type: "bootstrap-start"; id: number; time: string }
  | { type: "bootstrap-success"; data: TradeBootstrapData; id: number; time: string }
  | { type: "account-sync"; data: TradeBootstrapData }
  | { type: "bootstrap-error"; error: string; id: number; time: string }
  | { type: "ws-status"; status: ConnectionStatus; id: number; time: string }
  | { type: "ws-authenticated"; user: TradeUser; id: number; time: string }
  | {
      type: "ws-snapshot";
      marketId: MarketId;
      sequence: number;
      bids: MarketBookLevel[];
      asks: MarketBookLevel[];
    }
  | {
      type: "ws-delta";
      marketId: MarketId;
      sequence: number;
      events: MarketBookDelta[];
      occurredAt: string;
    }
  | {
      type: "ws-reject";
      op: string;
      code: string;
      message: string;
      id: number;
      time: string;
    }
  | {
      type: "ws-fill";
      fill: TradeFill;
      id: number;
      time: string;
    }
  | {
      type: "ws-order-state";
      order: PendingOrder;
      status: "open" | "filled" | "canceled";
      id: number;
      time: string;
    }
  | {
      type: "ws-market-state";
      market: MarketDefinition;
    }
  | {
      type: "ws-market-deleted";
      marketId: MarketId;
    }
  | {
      type: "ws-book-reset";
      marketId: MarketId;
    }
  | {
      type: "ws-resync-required";
      channel: string;
      marketId?: MarketId;
      reason: string;
      id: number;
      time: string;
    }
  | {
      type: "ws-admin-message";
      level: "info" | "warning" | "critical";
      title?: string;
      body: string;
      market?: string;
      id: number;
      time: string;
    }
  | { type: "cancel-success"; orderId: string; id: number; time: string }
  | { type: "cancel-error"; error: string; id: number; time: string }
  | { type: "submit-start" }
  | { type: "submit-success"; result: SubmitOrderResult; id: number; time: string }
  | { type: "submit-error"; error: string; id: number; time: string };

const MAX_MESSAGES = 18;
const MAX_MARKET_TRADES = 240;

const currencyFormatter = new Intl.NumberFormat("en-US", {
  style: "currency",
  currency: "USD",
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
});

const percentFormatter = new Intl.NumberFormat("en-US", {
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
});

export function formatPrice(value: number) {
  return currencyFormatter.format(value);
}

export function formatMaybePrice(value: number | null) {
  if (value === null || value <= 0) {
    return "--";
  }

  return formatPrice(value);
}

export function formatInputPrice(value: number) {
  return String(Math.max(0, Math.trunc(value)));
}

export function formatBookTotal(value: number) {
  return currencyFormatter.format(value);
}

export function formatSignedCurrency(value: number) {
  if (value < 0) {
    return `(${currencyFormatter.format(Math.abs(value))})`;
  }

  return currencyFormatter.format(value);
}

export function sanitizeWholeNumberInput(value: string) {
  const [wholePart] = value.split(".");
  return wholePart.replace(/[^0-9]/g, "");
}

export function parseNumberInput(value: string) {
  const numeric = Number(sanitizeWholeNumberInput(value));
  return Number.isFinite(numeric) ? numeric : 0;
}

export function parseSharesInput(value: string) {
  const numeric = Number(value.replace(/[^0-9]/g, ""));
  return Number.isFinite(numeric) ? Math.max(0, Math.floor(numeric)) : 0;
}

function createEmptyMarketBook(marketId: MarketId): MarketBookState {
  return {
    marketId,
    sequence: 0,
    bids: [],
    asks: [],
    lastTradePrice: null,
    lastTradeQuantity: null,
  };
}

function createEmptyMarketTrades(markets: MarketDefinition[]) {
  return markets.reduce<Record<MarketId, MarketTrade[]>>((next, market) => {
    next[market.id] = [];
    return next;
  }, {});
}

function pushMessage(messages: MessageEntry[], message: MessageEntry) {
  const nextId =
    messages.length > 0
      ? Math.max(...messages.map((entry) => entry.id), message.id) + 1
      : Math.max(1, message.id);

  return [...messages, { ...message, id: nextId }].slice(-MAX_MESSAGES);
}

function normalizeMessages(messages: MessageEntry[]) {
  return messages.slice(-MAX_MESSAGES).map((message, index) => ({
    ...message,
    id: index + 1,
  }));
}

function mergeMessages(existing: MessageEntry[], incoming: MessageEntry[]) {
  const merged = [...incoming, ...existing];
  const deduped: MessageEntry[] = [];
  const seen = new Set<string>();

  for (const message of merged) {
    const signature = `${message.time}|${message.tone}|${message.text}`;
    if (seen.has(signature)) {
      continue;
    }

    seen.add(signature);
    deduped.push(message);
  }

  return normalizeMessages(deduped);
}

function positionMapFromSnapshots(
  markets: MarketDefinition[],
  positions: TradeBootstrapData["positions"],
  previous: Record<MarketId, PositionState>,
) {
  return markets.reduce<Record<MarketId, PositionState>>((next, market) => {
    const snapshot = positions.find((position) => position.market === market.id);
    const previousPosition = previous[market.id] ?? {
      netQuantity: 0,
      avgCost: null,
      realizedPnl: 0,
    };

    next[market.id] = {
      netQuantity: snapshot?.netQuantity ?? 0,
      avgCost: snapshot?.averageEntryPrice ?? previousPosition.avgCost,
      realizedPnl: snapshot?.realizedPnl ?? previousPosition.realizedPnl,
    };
    return next;
  }, {});
}

function upsertPositionSnapshot(
  positions: TradeBootstrapData["positions"],
  marketId: MarketId,
  nextPosition: PositionState,
) {
  const nextSnapshot = {
    market: marketId,
    netQuantity: nextPosition.netQuantity,
    averageEntryPrice: nextPosition.avgCost,
    realizedPnl: nextPosition.realizedPnl,
  };
  const nextPositions = positions.filter((position) => position.market !== marketId);
  return [...nextPositions, nextSnapshot];
}

function mergeOrderSides(
  current: Record<string, TradeSide>,
  orders: PendingOrder[],
) {
  return orders.reduce<Record<string, TradeSide>>((next, order) => {
    next[order.id] = order.side;
    return next;
  }, { ...current });
}

function syncMarketDefinitions(
  currentState: TradeState,
  markets: MarketDefinition[],
  positions: TradeBootstrapData["positions"],
) {
  const nextMarkets = markets.length > 0 ? markets : currentState.availableMarkets;
  const nextMarketBooks = nextMarkets.reduce<Record<MarketId, MarketBookState>>((next, market) => {
    next[market.id] = currentState.marketBooks[market.id] ?? createEmptyMarketBook(market.id);
    return next;
  }, {});
  const nextSelectedMarketId = nextMarkets.some(
    (market) => market.id === currentState.selectedMarketId,
  )
    ? currentState.selectedMarketId
    : nextMarkets[0]?.id ?? currentState.selectedMarketId;
  const nextMarketTrades = nextMarkets.reduce<Record<MarketId, MarketTrade[]>>((next, market) => {
    next[market.id] = currentState.marketTradesByMarket[market.id] ?? [];
    return next;
  }, {});

  return {
    availableMarkets: nextMarkets,
    marketBooks: nextMarketBooks,
    marketTradesByMarket: nextMarketTrades,
    positionsByMarket: positionMapFromSnapshots(
      nextMarkets,
      positions,
      currentState.positionsByMarket,
    ),
    selectedMarketId: nextSelectedMarketId,
  };
}

function upsertMarketDefinition(
  markets: MarketDefinition[],
  nextMarket: MarketDefinition,
) {
  const index = markets.findIndex((market) => market.id === nextMarket.id);
  if (index === -1) {
    return [...markets, nextMarket];
  }

  return markets.map((market, currentIndex) =>
    currentIndex === index ? nextMarket : market,
  );
}

function removeMarketDefinition(markets: MarketDefinition[], marketId: MarketId) {
  return markets.filter((market) => market.id !== marketId);
}

function tradesFromFills(fills: TradeFill[], marketId: MarketId): MarketTrade[] {
  return fills
    .filter((fill) => fill.market === marketId)
    .map((fill) => ({
      marketId,
      price: fill.price,
      quantity: fill.quantity,
      occurredAt: fill.occurredAt,
    }));
}

function mergeMarketTrades(existing: MarketTrade[], incoming: MarketTrade[]) {
  if (incoming.length === 0) {
    return existing;
  }

  return [...existing, ...incoming]
    .sort((left, right) => Date.parse(left.occurredAt) - Date.parse(right.occurredAt))
    .slice(-MAX_MARKET_TRADES);
}

function seedMarketTrades(
  markets: MarketDefinition[],
  currentTrades: Record<MarketId, MarketTrade[]>,
  fills?: TradeFill[],
) {
  return markets.reduce<Record<MarketId, MarketTrade[]>>((next, market) => {
    const existing = currentTrades[market.id] ?? [];
    next[market.id] =
      existing.length > 0 ? existing : fills ? tradesFromFills(fills, market.id).slice(-MAX_MARKET_TRADES) : [];
    return next;
  }, {});
}

function updateMarketBookForDelta(
  book: MarketBookState,
  event: MarketBookDelta,
): MarketBookState {
  if (event.kind === "trade") {
    return {
      ...book,
      lastTradePrice: event.price,
      lastTradeQuantity: event.quantity,
    };
  }

  const sideKey = event.side === "buy" ? "bids" : "asks";
  return {
    ...book,
    [sideKey]: upsertBookLevel(book[sideKey], event.side, {
      price: event.price,
      quantity: event.quantity,
    }),
  };
}

function sortBookLevels(levels: MarketBookLevel[], side: TradeSide) {
  return [...levels].sort((left, right) =>
    side === "buy" ? right.price - left.price : left.price - right.price,
  );
}

function upsertBookLevel(levels: MarketBookLevel[], side: TradeSide, nextLevel: MarketBookLevel) {
  const remainingLevels = levels.filter((level) => level.price !== nextLevel.price);
  if (nextLevel.quantity <= 0) {
    return sortBookLevels(remainingLevels, side);
  }

  remainingLevels.push(nextLevel);
  return sortBookLevels(remainingLevels, side);
}

function effectiveQuoteForSide(book: MarketBookState, side: TradeSide) {
  if (side === "buy") {
    return Math.min(...book.asks.map((level) => level.price), Number.POSITIVE_INFINITY);
  }

  return Math.max(...book.bids.map((level) => level.price), 0);
}

function maybeLimitInputForMarket(state: TradeState, marketId: MarketId, side: TradeSide) {
  const quote = effectiveQuoteForSide(
    state.marketBooks[marketId] ?? createEmptyMarketBook(marketId),
    side,
  );

  return Number.isFinite(quote) && quote > 0
    ? formatInputPrice(quote)
    : state.limitPriceInput;
}

function ensurePendingOrder(
  pendingOrders: PendingOrder[],
  result: SubmitOrderResult,
): PendingOrder[] {
  const nextOrder: PendingOrder = {
    id: result.orderId,
    createdAt: result.createdAt,
    marketId: result.marketId,
    marketName: result.marketName,
    side: result.side,
    shares: result.remaining,
    limitPrice: result.requestedPrice,
    status: result.remaining < result.quantity ? "partial" : "open",
  };

  const withoutCurrent = pendingOrders.filter((order) => order.id !== result.orderId);
  if (!result.resting || result.remaining <= 0) {
    return withoutCurrent;
  }

  return [...withoutCurrent, nextOrder];
}

function upsertPendingOrder(pendingOrders: PendingOrder[], nextOrder: PendingOrder) {
  const withoutCurrent = pendingOrders.filter((order) => order.id !== nextOrder.id);
  return [...withoutCurrent, nextOrder];
}

function upsertFill(fills: TradeFill[], nextFill: TradeFill) {
  const withoutCurrent = fills.filter((fill) => fill.fillId !== nextFill.fillId);
  return [...withoutCurrent, nextFill].slice(-50);
}

function weightedFillPrice(fills: TradeFill[]) {
  const totalQuantity = fills.reduce((sum, fill) => sum + fill.quantity, 0);
  if (totalQuantity === 0) {
    return null;
  }

  const weightedSum = fills.reduce((sum, fill) => sum + fill.price * fill.quantity, 0);
  return weightedSum / totalQuantity;
}

function fillPriceLabel(fills: TradeFill[], fallbackPrice: number) {
  const executionPrice = weightedFillPrice(fills) ?? fallbackPrice;
  const distinctPrices = new Set(fills.map((fill) => fill.price));
  return distinctPrices.size > 1
    ? `avg ${formatPrice(executionPrice)}`
    : formatPrice(executionPrice);
}

function applyTradeFillToPosition(
  position: PositionState,
  fill: Pick<TradeFill, "price" | "quantity">,
  side: TradeSide,
): PositionState {
  if (fill.quantity <= 0) {
    return position;
  }

  const executionPrice = fill.price;
  const fillDelta = side === "buy" ? fill.quantity : -fill.quantity;
  const currentNet = position.netQuantity;

  if (currentNet === 0) {
    return {
      ...position,
      netQuantity: fillDelta,
      avgCost: executionPrice,
    };
  }

  if (Math.sign(currentNet) === Math.sign(fillDelta)) {
    const currentAbs = Math.abs(currentNet);
    const fillAbs = Math.abs(fillDelta);
    const nextAbs = currentAbs + fillAbs;
    const avgCost =
      position.avgCost === null
        ? executionPrice
        : (position.avgCost * currentAbs + executionPrice * fillAbs) / nextAbs;

    return {
      ...position,
      netQuantity: currentNet + fillDelta,
      avgCost,
    };
  }

  const closedQuantity = Math.min(Math.abs(currentNet), Math.abs(fillDelta));
  const realizedDelta =
    position.avgCost === null
      ? 0
      : currentNet > 0
        ? (executionPrice - position.avgCost) * closedQuantity
        : (position.avgCost - executionPrice) * closedQuantity;
  const nextNet = currentNet + fillDelta;

  return {
    netQuantity: nextNet,
    avgCost: nextNet === 0 ? null : Math.sign(nextNet) === Math.sign(currentNet) ? position.avgCost : executionPrice,
    realizedPnl: position.realizedPnl + realizedDelta,
  };
}

function applyOwnFillToPosition(
  position: PositionState,
  result: SubmitOrderResult,
): PositionState {
  return result.fills.reduce(
    (nextPosition, fill) => applyTradeFillToPosition(nextPosition, fill, result.side),
    position,
  );
}

function sideForFill(
  knownOrderSides: Record<string, TradeSide>,
  fill: Pick<TradeFill, "makerOrderId" | "takerOrderId">,
): TradeSide | null {
  const makerOrderSide = knownOrderSides[fill.makerOrderId];
  if (makerOrderSide) {
    return makerOrderSide;
  }

  return knownOrderSides[fill.takerOrderId] ?? null;
}

export function createInitialTradeState(markets: MarketDefinition[]): TradeState {
  const marketBooks = markets.reduce<Record<MarketId, MarketBookState>>((next, market) => {
    next[market.id] = createEmptyMarketBook(market.id);
    return next;
  }, {});

  const positionsByMarket = markets.reduce<Record<MarketId, PositionState>>((next, market) => {
    next[market.id] = { netQuantity: 0, avgCost: null, realizedPnl: 0 };
    return next;
  }, {});

  return {
    availableMarkets: markets,
    selectedMarketId: markets[0]?.id ?? "",
    connectionStatus: "connecting",
    bootstrapStatus: "idle",
    user: null,
    positions: [],
    positionsByMarket,
    pendingOrders: [],
    knownOrderSides: {},
    fills: [],
    marketBooks,
    marketTradesByMarket: createEmptyMarketTrades(markets),
    ticketSide: "buy",
    positionFilter: "active",
    orderType: "limit",
    limitPriceInput: "0",
    sharesInput: "20",
    messages: [],
    submittedOrders: 0,
    filledOrders: 0,
    isSubmitting: false,
  };
}

function applyBootstrapDataToState(
  currentState: TradeState,
  data: TradeBootstrapData,
) {
  const nextMarkets = data.loaded.markets ? data.markets : currentState.availableMarkets;
  const nextPositions = data.loaded.positions ? data.positions : currentState.positions;
  const synced = syncMarketDefinitions(currentState, nextMarkets, nextPositions);
  return {
    ...currentState,
    availableMarkets: synced.availableMarkets,
    selectedMarketId: synced.selectedMarketId,
    user: data.loaded.user ? data.user : currentState.user,
    positions: nextPositions,
    pendingOrders: data.loaded.openOrders ? data.openOrders : currentState.pendingOrders,
    knownOrderSides: data.loaded.openOrders
      ? mergeOrderSides(currentState.knownOrderSides, data.openOrders)
      : currentState.knownOrderSides,
    fills: data.loaded.fills ? data.fills : currentState.fills,
    marketBooks: synced.marketBooks,
    marketTradesByMarket: seedMarketTrades(
      synced.availableMarkets,
      synced.marketTradesByMarket,
      data.loaded.fills ? data.fills : undefined,
    ),
    positionsByMarket: synced.positionsByMarket,
  };
}

export function tradeReducer(state: TradeState, action: TradeAction): TradeState {
  switch (action.type) {
    case "hydrate-messages":
      return action.messages.length > 0
        ? { ...state, messages: mergeMessages(state.messages, action.messages) }
        : state;

    case "select-market": {
      if (action.marketId === state.selectedMarketId) {
        return state;
      }

      const nextMarket = state.availableMarkets.find((market) => market.id === action.marketId);
      if (!nextMarket) {
        return state;
      }

      return {
        ...state,
        selectedMarketId: action.marketId,
        limitPriceInput: maybeLimitInputForMarket(state, action.marketId, state.ticketSide),
      };
    }

    case "set-side":
      return {
        ...state,
        ticketSide: action.side,
        limitPriceInput: maybeLimitInputForMarket(
          state,
          state.selectedMarketId,
          action.side,
        ),
      };

    case "set-position-filter":
      return { ...state, positionFilter: action.filter };

    case "set-order-type":
      return { ...state, orderType: action.orderType };

    case "set-limit-price":
      return { ...state, limitPriceInput: sanitizeWholeNumberInput(action.value) };

    case "set-shares":
      return { ...state, sharesInput: action.value };

    case "adjust-shares": {
      const currentShares = parseSharesInput(state.sharesInput);
      return {
        ...state,
        sharesInput: String(Math.max(0, currentShares + action.delta)),
      };
    }

    case "bootstrap-start":
      return {
        ...state,
        bootstrapStatus: "loading",
        messages: pushMessage(state.messages, {
          id: action.id,
          time: action.time,
          tone: "neutral",
          text: "Bootstrapping account and order state from the exchange API.",
        }),
      };

    case "bootstrap-success": {
      let messages = pushMessage(state.messages, {
        id: action.id,
        time: action.time,
        tone: "neutral",
        text: action.data.user
          ? `Loaded account state for ${action.data.user.teamNumber}.`
          : "Connected in public market-data mode.",
      });

        messages = action.data.warnings.reduce(
        (next, warning, index) =>
          pushMessage(next, {
            id: action.id + index + 1,
            time: action.time,
            tone: "negative",
            text: warning,
          }),
        messages,
      );

      return {
        ...applyBootstrapDataToState(state, action.data),
        bootstrapStatus: "ready",
        messages,
      };
    }

    case "account-sync":
      return applyBootstrapDataToState(state, action.data);

    case "bootstrap-error":
      return {
        ...state,
        bootstrapStatus: "error",
        messages: pushMessage(state.messages, {
          id: action.id,
          time: action.time,
          tone: "negative",
          text: action.error,
        }),
      };

    case "ws-status": {
      if (action.status === state.connectionStatus) {
        return state;
      }

      const text =
        action.status === "connected"
          ? "Market data connected."
          : action.status === "reconnecting"
            ? "Market data reconnecting."
            : action.status === "disconnected"
              ? "Market data disconnected."
              : "Connecting to market data.";

      return {
        ...state,
        connectionStatus: action.status,
        messages: pushMessage(state.messages, {
          id: action.id,
          time: action.time,
          tone: action.status === "connected" ? "positive" : "neutral",
          text,
        }),
      };
    }

    case "ws-authenticated":
      return {
        ...state,
        user: state.user ?? action.user,
        messages: pushMessage(state.messages, {
          id: action.id,
          time: action.time,
          tone: "positive",
          text: `WebSocket authenticated for ${action.user.teamNumber}.`,
        }),
      };

    case "ws-snapshot": {
      const currentBook = state.marketBooks[action.marketId] ?? createEmptyMarketBook(action.marketId);
      const nextBook: MarketBookState = {
        ...currentBook,
        marketId: action.marketId,
        sequence: action.sequence,
        bids: action.bids,
        asks: action.asks,
      };

      return {
        ...state,
        marketBooks: {
          ...state.marketBooks,
          [action.marketId]: nextBook,
        },
      };
    }

    case "ws-delta": {
      const currentBook = state.marketBooks[action.marketId] ?? createEmptyMarketBook(action.marketId);
      if (action.sequence <= currentBook.sequence) {
        return state;
      }

      const nextBook = action.events.reduce(
        (book, event) => updateMarketBookForDelta(book, event),
        { ...currentBook, sequence: action.sequence },
      );
      const nextTrades = action.events.flatMap((event) =>
        event.kind === "trade"
          ? [
              {
                marketId: action.marketId,
                price: event.price,
                quantity: event.quantity,
                occurredAt: action.occurredAt,
              },
            ]
          : [],
      );

      return {
        ...state,
        marketBooks: {
          ...state.marketBooks,
          [action.marketId]: nextBook,
        },
        marketTradesByMarket: nextTrades.length > 0
          ? {
              ...state.marketTradesByMarket,
              [action.marketId]: mergeMarketTrades(
                state.marketTradesByMarket[action.marketId] ?? [],
                nextTrades,
              ),
            }
          : state.marketTradesByMarket,
      };
    }

    case "ws-reject":
      return {
        ...state,
        isSubmitting: false,
        messages: pushMessage(state.messages, {
          id: action.id,
          time: action.time,
          tone: "negative",
          text: `${action.op} rejected (${action.code}): ${action.message}`,
        }),
      };

    case "ws-fill": {
      if (state.fills.some((fill) => fill.fillId === action.fill.fillId)) {
        return state;
      }

      const wsFillSide = sideForFill(state.knownOrderSides, action.fill);
      const currentPosition =
        state.positionsByMarket[action.fill.market] ?? {
          netQuantity: 0,
          avgCost: null,
          realizedPnl: 0,
        };
      const nextPosition = wsFillSide
        ? applyTradeFillToPosition(currentPosition, action.fill, wsFillSide)
        : currentPosition;

      return {
        ...state,
        fills: upsertFill(state.fills, action.fill),
        positions: wsFillSide
          ? upsertPositionSnapshot(state.positions, action.fill.market, nextPosition)
          : state.positions,
        positionsByMarket: wsFillSide
          ? {
              ...state.positionsByMarket,
              [action.fill.market]: nextPosition,
            }
          : state.positionsByMarket,
        messages: pushMessage(state.messages, {
          id: action.id,
          time: action.time,
          tone: "positive",
          text: `Fill ${action.fill.market} ${action.fill.quantity} @ ${formatPrice(action.fill.price)}.`,
        }),
      };
    }

    case "ws-order-state": {
      const pendingOrders =
        action.status === "open"
          ? upsertPendingOrder(state.pendingOrders, action.order)
          : state.pendingOrders.filter((order) => order.id !== action.order.id);
      const nextState = {
        ...state,
        pendingOrders,
        knownOrderSides: {
          ...state.knownOrderSides,
          [action.order.id]: action.order.side,
        },
      };

      if (action.status === "filled") {
        return nextState;
      }

      const text =
        action.status === "open"
          ? `Order ${action.order.id} is open for ${action.order.shares} shares.`
          : `Order ${action.order.id} canceled.`;

      return {
        ...nextState,
        messages: pushMessage(nextState.messages, {
          id: action.id,
          time: action.time,
          tone: action.status === "canceled" ? "neutral" : "positive",
          text,
        }),
      };
    }

    case "ws-market-state": {
      const synced = syncMarketDefinitions(
        state,
        upsertMarketDefinition(state.availableMarkets, action.market),
        state.positions,
      );

      return {
        ...state,
        availableMarkets: synced.availableMarkets,
        selectedMarketId: synced.selectedMarketId,
        marketBooks: synced.marketBooks,
        marketTradesByMarket: synced.marketTradesByMarket,
        positionsByMarket: synced.positionsByMarket,
      };
    }

    case "ws-market-deleted": {
      const nextMarkets = removeMarketDefinition(state.availableMarkets, action.marketId);
      if (nextMarkets.length === state.availableMarkets.length) {
        return state;
      }

      const synced = syncMarketDefinitions(state, nextMarkets, state.positions);
      const selectedMarketChanged =
        synced.selectedMarketId !== state.selectedMarketId;

      return {
        ...state,
        availableMarkets: synced.availableMarkets,
        selectedMarketId: synced.selectedMarketId,
        marketBooks: synced.marketBooks,
        marketTradesByMarket: synced.marketTradesByMarket,
        positionsByMarket: synced.positionsByMarket,
        limitPriceInput:
          selectedMarketChanged && synced.availableMarkets.length > 0
            ? maybeLimitInputForMarket(
                {
                  ...state,
                  marketBooks: synced.marketBooks,
                },
                synced.selectedMarketId,
                state.ticketSide,
              )
            : state.limitPriceInput,
      };
    }

    case "ws-book-reset": {
      const currentBook = state.marketBooks[action.marketId] ?? createEmptyMarketBook(action.marketId);
      return {
        ...state,
        marketBooks: {
          ...state.marketBooks,
          [action.marketId]: {
            ...createEmptyMarketBook(action.marketId),
            lastTradePrice: currentBook.lastTradePrice,
            lastTradeQuantity: currentBook.lastTradeQuantity,
          },
        },
      };
    }

    case "ws-resync-required":
      return {
        ...state,
        messages: pushMessage(state.messages, {
          id: action.id,
          time: action.time,
          tone: "negative",
          text: `${action.channel} resync required${action.marketId ? ` for ${action.marketId}` : ""}: ${action.reason}`,
        }),
      };

    case "ws-admin-message":
      return {
        ...state,
        messages: pushMessage(state.messages, {
          id: action.id,
          time: action.time,
          tone: action.level === "critical" ? "negative" : action.level === "warning" ? "neutral" : "positive",
          text: `${action.title ? `${action.title}: ` : ""}${action.body}${action.market ? ` (${action.market})` : ""}`,
        }),
      };

    case "cancel-success":
      return {
        ...state,
        pendingOrders: state.pendingOrders.filter((order) => order.id !== action.orderId),
        messages: pushMessage(state.messages, {
          id: action.id,
          time: action.time,
          tone: "neutral",
          text: `Cancel requested for order ${action.orderId}.`,
        }),
      };

    case "cancel-error":
      return {
        ...state,
        messages: pushMessage(state.messages, {
          id: action.id,
          time: action.time,
          tone: "negative",
          text: action.error,
        }),
      };

    case "submit-start":
      return {
        ...state,
        isSubmitting: true,
      };

    case "submit-success": {
      const nextMessages = pushMessage(state.messages, {
        id: action.id,
        time: action.time,
        tone: action.result.fills.length > 0 ? "positive" : "neutral",
        text: buildSubmitSuccessMessage(action.result),
      });
      const currentPosition =
        state.positionsByMarket[action.result.marketId] ?? {
          netQuantity: 0,
          avgCost: null,
          realizedPnl: 0,
        };
      const nextPosition = applyOwnFillToPosition(currentPosition, action.result);
      const nextFills = action.result.fills.reduce(
        (currentFills, fill) => upsertFill(currentFills, fill),
        state.fills,
      );

      return {
        ...state,
        isSubmitting: false,
        submittedOrders: state.submittedOrders + 1,
        filledOrders: state.filledOrders + (action.result.fills.length > 0 ? 1 : 0),
        pendingOrders: ensurePendingOrder(state.pendingOrders, action.result),
        knownOrderSides: {
          ...state.knownOrderSides,
          [action.result.orderId]: action.result.side,
        },
        fills: nextFills,
        positions: upsertPositionSnapshot(state.positions, action.result.marketId, nextPosition),
        positionsByMarket: {
          ...state.positionsByMarket,
          [action.result.marketId]: nextPosition,
        },
        messages: nextMessages,
      };
    }

    case "submit-error":
      return {
        ...state,
        isSubmitting: false,
        submittedOrders: state.submittedOrders + 1,
        messages: pushMessage(state.messages, {
          id: action.id,
          time: action.time,
          tone: "negative",
          text: action.error,
        }),
      };
  }
}

function buildSubmitSuccessMessage(result: SubmitOrderResult) {
  const filledQuantity = result.fills.reduce((sum, fill) => sum + fill.quantity, 0);
  const executionPrice = fillPriceLabel(result.fills, result.effectivePrice);
  const baseText =
    result.resting && result.remaining > 0
      ? filledQuantity > 0
        ? `Accepted ${result.side} ${result.marketName} for ${result.quantity} shares. Filled ${filledQuantity} at ${executionPrice} and ${result.remaining} remain resting at ${formatPrice(result.requestedPrice)}.`
        : `Accepted ${result.side} ${result.marketName} for ${result.quantity} shares at ${formatPrice(result.requestedPrice)}. ${result.remaining} shares remain resting.`
      : `Filled ${result.side} ${result.marketName} for ${filledQuantity || result.quantity} shares at ${executionPrice}.`;

  return baseText;
}

function bookLevelsForSummary(levels: MarketBookLevel[]) {
  return levels.map(
    ({ price, quantity }): AggregatedBookLevel => ({
      price,
      liquidity: quantity,
      total: price * quantity,
    }),
  );
}

export function selectMarketById(state: TradeState, marketId: MarketId) {
  return state.availableMarkets.find((market) => market.id === marketId) ?? null;
}

export function selectSelectedMarket(state: TradeState) {
  return selectMarketById(state, state.selectedMarketId);
}

export function selectSelectedMarketBook(state: TradeState) {
  return state.marketBooks[state.selectedMarketId] ?? createEmptyMarketBook(state.selectedMarketId);
}

export function selectActiveRows(state: TradeState) {
  return state.availableMarkets
    .map((market) => ({
      marketId: market.id,
      product: market.name,
      netQuantity: state.positionsByMarket[market.id]?.netQuantity ?? 0,
      avgCost: state.positionsByMarket[market.id]?.avgCost ?? null,
      active: (state.positionsByMarket[market.id]?.netQuantity ?? 0) !== 0,
    }))
    .filter((position) => position.active);
}

export function selectPendingRows(state: TradeState) {
  return [...state.pendingOrders]
    .reverse()
    .map((order) => ({
      ...order,
      marketName:
        selectMarketById(state, order.marketId)?.name ?? order.marketName ?? order.marketId,
    }));
}

export function selectSelectedMarketSummary(state: TradeState) {
  const book = selectSelectedMarketBook(state);
  const bids = bookLevelsForSummary(book.bids);
  const asks = bookLevelsForSummary(book.asks);
  const bestBid = bids[0]?.price ?? null;
  const bestAsk = asks[0]?.price ?? null;
  const lastPrice = book.lastTradePrice ?? bestAsk ?? bestBid ?? null;
  const midPrice =
    bestBid !== null && bestAsk !== null ? (bestBid + bestAsk) / 2 : lastPrice;
  const spread =
    bestBid !== null && bestAsk !== null ? Math.max(0, bestAsk - bestBid) : null;

  return {
    bids,
    asks,
    bestBid,
    bestAsk,
    buyQuote: bestAsk,
    sellQuote: bestBid,
    lastPrice,
    midPrice,
    spread,
  };
}

export function selectEstimatedCost(state: TradeState) {
  const shares = parseSharesInput(state.sharesInput);
  const summary = selectSelectedMarketSummary(state);
  const derivedPrice =
    state.orderType === "market"
      ? state.ticketSide === "buy"
        ? summary.buyQuote ?? 0
        : summary.sellQuote ?? 0
      : parseNumberInput(state.limitPriceInput);

  return {
    shares,
    derivedPrice,
    estimatedCost: derivedPrice * shares,
  };
}

export function selectPnlMetrics(state: TradeState): PnlMetric[] {
  const totals = state.availableMarkets.reduce(
    (next, market) => {
      const position = state.positionsByMarket[market.id] ?? {
        netQuantity: 0,
        avgCost: null,
        realizedPnl: 0,
      };
      const book = state.marketBooks[market.id] ?? createEmptyMarketBook(market.id);
      const summary = {
        ...selectSelectedMarketSummary({
          ...state,
          selectedMarketId: market.id,
          marketBooks: {
            ...state.marketBooks,
            [market.id]: book,
          },
        }),
      };

      const mark = summary.midPrice ?? summary.lastPrice;
      if (mark !== null) {
        next.exposure += Math.abs(position.netQuantity) * mark;
      }
      if (mark !== null && position.avgCost !== null) {
        next.unrealized += (mark - position.avgCost) * position.netQuantity;
      }
      next.realized += position.realizedPnl;
      return next;
    },
    { unrealized: 0, realized: 0, exposure: 0 },
  );

  const fillRate =
    state.submittedOrders === 0
      ? 100
      : (state.filledOrders / state.submittedOrders) * 100;
  const netPnl = totals.unrealized + totals.realized;
  const openOrders = state.pendingOrders.length;

  return [
    {
      label: "Unrealized PnL",
      value: formatSignedCurrency(totals.unrealized),
      tone:
        totals.unrealized < 0
          ? "negative"
          : totals.unrealized > 0
            ? "positive"
            : "neutral",
    },
    {
      label: "Realized PnL",
      value: formatSignedCurrency(totals.realized),
      tone:
        totals.realized < 0
          ? "negative"
          : totals.realized > 0
            ? "positive"
            : "neutral",
    },
    {
      label: "Net PnL",
      value: formatSignedCurrency(netPnl),
      tone: netPnl < 0 ? "negative" : netPnl > 0 ? "positive" : "primary",
    },
    {
      label: "Exposure",
      value: formatSignedCurrency(totals.exposure),
      tone: totals.exposure > 0 ? "primary" : "neutral",
    },
    {
      label: "Open Orders",
      value: String(openOrders),
      tone: openOrders > 0 ? "primary" : "neutral",
    },
    {
      label: "Fill Rate",
      value: `${percentFormatter.format(fillRate)}%`,
      tone: "neutral",
    },
  ];
}

export function initialsForUser(user: TradeUser | null) {
  if (!user) {
    return "QT";
  }

  return user.teamNumber
    .split(/[\s._-]+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase() ?? "")
    .join("");
}
