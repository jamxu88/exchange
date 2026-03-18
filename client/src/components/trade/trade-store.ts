import type {
  AggregatedBookLevel,
  ConnectionStatus,
  MarketBookDelta,
  MarketBookOrder,
  MarketBookState,
  MarketDefinition,
  MarketId,
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
  balances: { asset: string; free: number; locked: number }[];
  positionsByMarket: Record<MarketId, PositionState>;
  pendingOrders: PendingOrder[];
  fills: TradeFill[];
  marketBooks: Record<MarketId, MarketBookState>;
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
  | { type: "select-market"; marketId: MarketId; id: number; time: string }
  | { type: "set-side"; side: TradeSide }
  | { type: "set-position-filter"; filter: PositionFilter }
  | { type: "set-order-type"; orderType: OrderType }
  | { type: "set-limit-price"; value: string }
  | { type: "set-shares"; value: string }
  | { type: "adjust-shares"; delta: number }
  | { type: "bootstrap-start"; id: number; time: string }
  | { type: "bootstrap-success"; data: TradeBootstrapData; id: number; time: string }
  | { type: "bootstrap-error"; error: string; id: number; time: string }
  | { type: "ws-status"; status: ConnectionStatus; id: number; time: string }
  | { type: "ws-authenticated"; user: TradeUser; id: number; time: string }
  | {
      type: "ws-snapshot";
      marketId: MarketId;
      sequence: number;
      bids: MarketBookOrder[];
      asks: MarketBookOrder[];
    }
  | {
      type: "ws-delta";
      marketId: MarketId;
      sequence: number;
      events: MarketBookDelta[];
    }
  | { type: "submit-start" }
  | { type: "submit-success"; result: SubmitOrderResult; id: number; time: string }
  | { type: "submit-error"; error: string; id: number; time: string };

const MAX_MESSAGES = 18;

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
  return value.toFixed(2);
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

export function parseNumberInput(value: string) {
  const numeric = Number(value.replace(/[^0-9.]/g, ""));
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

function pushMessage(messages: MessageEntry[], message: MessageEntry) {
  const nextId =
    messages.length > 0
      ? Math.max(...messages.map((entry) => entry.id), message.id) + 1
      : Math.max(1, message.id);

  return [...messages, { ...message, id: nextId }].slice(-MAX_MESSAGES);
}

function positionMapFromBalances(
  markets: MarketDefinition[],
  balances: TradeBootstrapData["balances"],
  previous: Record<MarketId, PositionState>,
) {
  return markets.reduce<Record<MarketId, PositionState>>((next, market) => {
    const assetBalance = balances.find((balance) => balance.asset === market.baseAsset);
    const previousPosition = previous[market.id] ?? {
      shares: 0,
      avgCost: null,
      realizedPnl: 0,
    };

    next[market.id] = {
      shares: (assetBalance?.free ?? 0) + (assetBalance?.locked ?? 0),
      avgCost: previousPosition.avgCost,
      realizedPnl: previousPosition.realizedPnl,
    };
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

  const sideKey = event.kind === "order_removed"
    ? event.side === "buy"
      ? "bids"
      : "asks"
    : event.order.side === "buy"
      ? "bids"
      : "asks";
  const sideOrders = book[sideKey];

  if (event.kind === "order_added") {
    const nextOrders = upsertBookOrder(sideOrders, event.order);
    return { ...book, [sideKey]: nextOrders };
  }

  if (event.kind === "order_updated") {
    const nextOrders = upsertBookOrder(sideOrders, event.order);
    return { ...book, [sideKey]: nextOrders };
  }

  return {
    ...book,
    [sideKey]: sideOrders.filter((order) => order.orderId !== event.orderId),
  };
}

function upsertBookOrder(orders: MarketBookOrder[], nextOrder: MarketBookOrder) {
  const next = orders.filter((order) => order.orderId !== nextOrder.orderId);
  next.push(nextOrder);
  return next;
}

function effectiveQuoteForSide(book: MarketBookState, side: TradeSide) {
  if (side === "buy") {
    return Math.min(...book.asks.map((order) => order.price), Number.POSITIVE_INFINITY);
  }

  return Math.max(...book.bids.map((order) => order.price), 0);
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
    limitPrice: result.effectivePrice,
    status: result.remaining < result.quantity ? "partial" : "open",
  };

  const withoutCurrent = pendingOrders.filter((order) => order.id !== result.orderId);
  if (!result.resting || result.remaining <= 0) {
    return withoutCurrent;
  }

  return [...withoutCurrent, nextOrder];
}

function weightedFillPrice(fills: TradeFill[]) {
  const totalQuantity = fills.reduce((sum, fill) => sum + fill.quantity, 0);
  if (totalQuantity === 0) {
    return null;
  }

  const weightedSum = fills.reduce((sum, fill) => sum + fill.price * fill.quantity, 0);
  return weightedSum / totalQuantity;
}

function applyOwnFillToPosition(
  position: PositionState,
  result: SubmitOrderResult,
): PositionState {
  const executedQuantity = result.fills.reduce((sum, fill) => sum + fill.quantity, 0);
  if (executedQuantity <= 0) {
    return position;
  }

  const executionPrice = weightedFillPrice(result.fills) ?? result.effectivePrice;

  if (result.side === "buy") {
    const nextShares = position.shares + executedQuantity;
    const nextAvgCost =
      position.avgCost === null && position.shares > 0
        ? null
        : position.avgCost === null
          ? executionPrice
          : (position.avgCost * position.shares + executionPrice * executedQuantity) /
            nextShares;

    return {
      ...position,
      shares: nextShares,
      avgCost: nextAvgCost,
    };
  }

  const nextShares = Math.max(0, position.shares - executedQuantity);
  const realizedDelta =
    position.avgCost === null ? 0 : (executionPrice - position.avgCost) * executedQuantity;

  return {
    shares: nextShares,
    avgCost: nextShares === 0 ? null : position.avgCost,
    realizedPnl: position.realizedPnl + realizedDelta,
  };
}

export function createInitialTradeState(markets: MarketDefinition[]): TradeState {
  const marketBooks = markets.reduce<Record<MarketId, MarketBookState>>((next, market) => {
    next[market.id] = createEmptyMarketBook(market.id);
    return next;
  }, {});

  const positionsByMarket = markets.reduce<Record<MarketId, PositionState>>((next, market) => {
    next[market.id] = { shares: 0, avgCost: null, realizedPnl: 0 };
    return next;
  }, {});

  return {
    availableMarkets: markets,
    selectedMarketId: markets[0]?.id ?? "BTC-USD",
    connectionStatus: "connecting",
    bootstrapStatus: "idle",
    user: null,
    balances: [],
    positionsByMarket,
    pendingOrders: [],
    fills: [],
    marketBooks,
    ticketSide: "buy",
    positionFilter: "active",
    orderType: "limit",
    limitPriceInput: "0.00",
    sharesInput: "20",
    messages: [],
    submittedOrders: 0,
    filledOrders: 0,
    isSubmitting: false,
  };
}

export function tradeReducer(state: TradeState, action: TradeAction): TradeState {
  switch (action.type) {
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
        messages: pushMessage(state.messages, {
          id: action.id,
          time: action.time,
          tone: "neutral",
          text: `Switched to ${nextMarket.name}.`,
        }),
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
      return { ...state, limitPriceInput: action.value };

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
          ? `Loaded account state for ${action.data.user.username}.`
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
        ...state,
        bootstrapStatus: "ready",
        user: action.data.user,
        balances: action.data.balances,
        pendingOrders: action.data.openOrders,
        fills: action.data.fills,
        positionsByMarket: positionMapFromBalances(
          state.availableMarkets,
          action.data.balances,
          state.positionsByMarket,
        ),
        messages,
      };
    }

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
          text: `WebSocket authenticated for ${action.user.username}.`,
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
        limitPriceInput:
          action.marketId === state.selectedMarketId
            ? maybeLimitInputForMarket(
                {
                  ...state,
                  marketBooks: {
                    ...state.marketBooks,
                    [action.marketId]: nextBook,
                  },
                },
                action.marketId,
                state.ticketSide,
              )
            : state.limitPriceInput,
      };
    }

    case "ws-delta": {
      const currentBook = state.marketBooks[action.marketId] ?? createEmptyMarketBook(action.marketId);
      if (action.sequence < currentBook.sequence) {
        return state;
      }

      const nextBook = action.events.reduce(
        (book, event) => updateMarketBookForDelta(book, event),
        { ...currentBook, sequence: action.sequence },
      );

      return {
        ...state,
        marketBooks: {
          ...state.marketBooks,
          [action.marketId]: nextBook,
        },
      };
    }

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
          shares: 0,
          avgCost: null,
          realizedPnl: 0,
        };

      return {
        ...state,
        isSubmitting: false,
        submittedOrders: state.submittedOrders + 1,
        filledOrders: state.filledOrders + (action.result.fills.length > 0 ? 1 : 0),
        pendingOrders: ensurePendingOrder(state.pendingOrders, action.result),
        fills: [...state.fills, ...action.result.fills].slice(-50),
        positionsByMarket: {
          ...state.positionsByMarket,
          [action.result.marketId]: applyOwnFillToPosition(
            currentPosition,
            action.result,
          ),
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
  const baseText =
    result.resting && result.remaining > 0
      ? `Accepted ${result.side} ${result.marketName} for ${result.quantity} shares at ${formatPrice(result.effectivePrice)}. ${result.remaining} shares remain resting.`
      : `Filled ${result.side} ${result.marketName} for ${filledQuantity || result.quantity} shares at ${formatPrice(result.effectivePrice)}.`;

  return result.syntheticMarket
    ? `${baseText} Routed as an aggressive limit order because the backend WS market-order protocol is not implemented yet.`
    : baseText;
}

export function aggregateBookLevels(orders: MarketBookOrder[], side: TradeSide) {
  const buckets = new Map<number, number>();
  for (const order of orders) {
    buckets.set(order.price, (buckets.get(order.price) ?? 0) + order.remaining);
  }

  const levels = [...buckets.entries()].map(
    ([price, liquidity]): AggregatedBookLevel => ({
      price,
      liquidity,
      total: price * liquidity,
    }),
  );

  return levels.sort((left, right) =>
    side === "buy" ? right.price - left.price : left.price - right.price,
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
      shares: state.positionsByMarket[market.id]?.shares ?? 0,
      avgCost: state.positionsByMarket[market.id]?.avgCost ?? null,
      active: (state.positionsByMarket[market.id]?.shares ?? 0) > 0,
    }))
    .filter((position) => position.active);
}

export function selectPendingRows(state: TradeState) {
  return [...state.pendingOrders].reverse();
}

export function selectSelectedMarketSummary(state: TradeState) {
  const book = selectSelectedMarketBook(state);
  const bids = aggregateBookLevels(book.bids, "buy");
  const asks = aggregateBookLevels(book.asks, "sell");
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
        shares: 0,
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
        next.exposure += position.shares * mark;
      }
      if (mark !== null && position.avgCost !== null) {
        next.unrealized += (mark - position.avgCost) * position.shares;
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
  const sharpe = totals.exposure === 0 ? 0 : (netPnl / totals.exposure) * 12;
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
      label: "Sharpe",
      value: sharpe.toFixed(2),
      tone: "neutral",
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

  return user.username
    .split(/[\s._-]+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase() ?? "")
    .join("");
}
