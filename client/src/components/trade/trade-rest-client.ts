import type {
  AccountPosition,
  MarketDefinition,
  PendingOrder,
  SubmitOrderIntent,
  SubmitOrderResult,
  TradeBootstrapData,
  TradeFill,
  TradeSide,
} from "@/components/trade/trade-types";
import type { TradeRuntimeConfig } from "@/components/trade/trade-runtime";

type FetchLike = typeof fetch;
type ApiSide = "BUY" | "SELL";

type ApiErrorPayload = {
  error?: string;
};

type UserResponse = {
  trader_id: string;
  username: string;
};

type PositionResponse = {
  market: string;
  net_quantity: number;
  average_entry_price: number | null;
  realized_pnl: number;
};

type OpenOrderResponse = {
  id: string;
  market: string;
  side: ApiSide;
  price: number;
  quantity: number;
  remaining: number;
  created_at: string;
};

type FillResponse = {
  fill_id: string;
  market: string;
  maker_order_id: string;
  taker_order_id: string;
  price: number;
  quantity: number;
  occurred_at: string;
};

type SubmitOrderResponse = {
  order: OpenOrderResponse;
  fills: FillResponse[];
  resting: boolean;
};

type CancelOrderResponse = {
  order: OpenOrderResponse;
};

type MarketResponse = {
  market_id: string;
  display_name: string;
  base_asset: string;
  quote_asset: string;
};

export class ExchangeApiError extends Error {
  status: number;

  constructor(message: string, status = 500) {
    super(message);
    this.name = "ExchangeApiError";
    this.status = status;
  }
}

function toTradeSide(side: ApiSide): TradeSide {
  return side === "BUY" ? "buy" : "sell";
}

function toApiSide(side: TradeSide): ApiSide {
  return side === "buy" ? "BUY" : "SELL";
}

function normalizePendingOrder(order: OpenOrderResponse): PendingOrder {
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

function normalizeFill(fill: FillResponse): TradeFill {
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

function normalizeMarket(market: MarketResponse): MarketDefinition {
  return {
    id: market.market_id,
    name: market.display_name,
    baseAsset: market.base_asset,
    quoteAsset: market.quote_asset,
  };
}

function weightedFillPrice(fills: FillResponse[]) {
  const totalQuantity = fills.reduce((sum, fill) => sum + fill.quantity, 0);
  if (totalQuantity <= 0) {
    return null;
  }

  const weightedSum = fills.reduce((sum, fill) => sum + fill.price * fill.quantity, 0);
  return weightedSum / totalQuantity;
}

function joinUrl(baseUrl: string, path: string) {
  return new URL(path, baseUrl.endsWith("/") ? baseUrl : `${baseUrl}/`).toString();
}

export class TradeRestClient {
  private readonly baseUrl: string;
  private readonly apiKey?: string;
  private readonly fetchImpl: FetchLike;

  constructor(config: Pick<TradeRuntimeConfig, "httpUrl" | "apiKey">, fetchImpl?: FetchLike) {
    this.baseUrl = config.httpUrl;
    this.apiKey = config.apiKey;
    this.fetchImpl = ((input: RequestInfo | URL, init?: RequestInit) =>
      Reflect.apply(fetchImpl ?? fetch, globalThis, [input, init])) as FetchLike;
  }

  async bootstrapAccountData(): Promise<TradeBootstrapData> {
    if (!this.apiKey) {
      return {
        markets: [],
        user: null,
        positions: [],
        openOrders: [],
        fills: [],
        warnings: ["No exchange API key configured. Account bootstrap skipped."],
        loaded: {
          markets: false,
          user: false,
          positions: false,
          openOrders: false,
          fills: false,
        },
      };
    }

    const [marketsResult, userResult, positionsResult, openOrdersResult, fillsResult] =
      await Promise.allSettled([
        this.request<MarketResponse[]>("/api/v1/markets", { includeAuth: false }),
        this.request<UserResponse>("/api/v1/user"),
        this.request<PositionResponse[]>("/api/v1/positions"),
        this.request<OpenOrderResponse[]>("/api/v1/open-orders"),
        this.request<FillResponse[]>("/api/v1/fills"),
      ]);

    const warnings: string[] = [];
    const markets = pickSettledValue(
      marketsResult,
      (value) => value.map(normalizeMarket),
      warnings,
      "Market bootstrap failed.",
    );
    const user = pickSettledValue(userResult, (value) => ({
      traderId: value.trader_id,
      username: value.username,
    }));
    const positions = pickSettledValue(
      positionsResult,
      (value): AccountPosition[] =>
        value.map((position) => ({
          market: position.market,
          netQuantity: position.net_quantity,
          averageEntryPrice: position.average_entry_price,
          realizedPnl: position.realized_pnl,
        })),
      warnings,
      "Position bootstrap failed.",
    );
    const openOrders = pickSettledValue(
      openOrdersResult,
      (value) => value.map(normalizePendingOrder),
      warnings,
      "Open order bootstrap failed.",
    );
    const fills = pickSettledValue(
      fillsResult,
      (value) => value.map(normalizeFill),
      warnings,
      "Fill bootstrap failed.",
    );

    if (userResult.status === "rejected") {
      throw userResult.reason;
    }

    return {
      markets,
      user: user ?? null,
      positions,
      openOrders,
      fills,
      warnings,
      loaded: {
        markets: marketsResult.status === "fulfilled",
        user: userResult.status === "fulfilled",
        positions: positionsResult.status === "fulfilled",
        openOrders: openOrdersResult.status === "fulfilled",
        fills: fillsResult.status === "fulfilled",
      },
    };
  }

  async submitOrder(intent: SubmitOrderIntent): Promise<SubmitOrderResult> {
    if (!this.apiKey) {
      throw new ExchangeApiError(
        "No exchange API key configured. Set NEXT_PUBLIC_EXCHANGE_API_KEY to enable trading.",
        401,
      );
    }

    const payload = await this.request<SubmitOrderResponse>("/api/v1/orders", {
      method: "POST",
      body: JSON.stringify({
        market: intent.marketId,
        side: toApiSide(intent.side),
        order_type: intent.orderType,
        price: intent.orderType === "limit" ? intent.effectivePrice : 0,
        quantity: intent.quantity,
      }),
    });
    const actualPrice = weightedFillPrice(payload.fills) ?? payload.order.price;

    return {
      orderId: payload.order.id,
      marketId: payload.order.market,
      marketName: intent.marketName,
      side: intent.side,
      orderType: intent.orderType,
      quantity: intent.quantity,
      requestedPrice: intent.requestedPrice,
      effectivePrice: actualPrice,
      resting: payload.resting,
      remaining: payload.order.remaining,
      fills: payload.fills.map(normalizeFill),
      createdAt: payload.order.created_at,
    };
  }

  async cancelOrder(orderId: string): Promise<PendingOrder> {
    if (!this.apiKey) {
      throw new ExchangeApiError(
        "No exchange API key configured. Set NEXT_PUBLIC_EXCHANGE_API_KEY to enable trading.",
        401,
      );
    }

    const payload = await this.request<CancelOrderResponse>(
      `/api/v1/orders/${encodeURIComponent(orderId)}`,
      {
        method: "DELETE",
      },
    );

    return normalizePendingOrder(payload.order);
  }

  private async request<T>(
    path: string,
    init?: RequestInit & { includeAuth?: boolean },
  ): Promise<T> {
    const { includeAuth = true, ...requestInit } = init ?? {};
    const response = await this.fetchImpl(joinUrl(this.baseUrl, path), {
      ...requestInit,
      headers: {
        accept: "application/json",
        ...(requestInit.body ? { "content-type": "application/json" } : {}),
        ...(includeAuth && this.apiKey ? { "x-api-key": this.apiKey } : {}),
        ...requestInit.headers,
      },
    });

    const text = await response.text();
    const parsed = text ? (JSON.parse(text) as T | ApiErrorPayload) : null;

    if (!response.ok) {
      const message =
        typeof parsed === "object" && parsed && "error" in parsed && parsed.error
          ? parsed.error
          : `Exchange API request failed with ${response.status}`;
      throw new ExchangeApiError(message, response.status);
    }

    return parsed as T;
  }
}

function pickSettledValue<T, U>(
  result: PromiseSettledResult<T>,
  mapper: (value: T) => U,
  warnings: string[] = [],
  warningText?: string,
): U {
  if (result.status === "fulfilled") {
    return mapper(result.value);
  }

  if (warningText) {
    const reason =
      result.reason instanceof Error ? result.reason.message : String(result.reason);
    warnings.push(`${warningText} ${reason}`);
  }

  return ([] as unknown) as U;
}
