import type {
  AccountBalance,
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

type BalanceResponse = {
  asset: string;
  free: number;
  locked: number;
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
    this.fetchImpl = fetchImpl ?? fetch;
  }

  async bootstrapAccountData(): Promise<TradeBootstrapData> {
    if (!this.apiKey) {
      return {
        user: null,
        balances: [],
        openOrders: [],
        fills: [],
        warnings: ["No exchange API key configured. Account bootstrap skipped."],
      };
    }

    const [userResult, balanceResult, openOrdersResult, fillsResult] =
      await Promise.allSettled([
        this.request<UserResponse>("/api/v1/user"),
        this.request<BalanceResponse[]>("/api/v1/balance"),
        this.request<OpenOrderResponse[]>("/api/v1/open-orders"),
        this.request<FillResponse[]>("/api/v1/fills"),
      ]);

    const warnings: string[] = [];
    const user = pickSettledValue(userResult, (value) => ({
      traderId: value.trader_id,
      username: value.username,
    }));
    const balances = pickSettledValue(
      balanceResult,
      (value): AccountBalance[] =>
        value.map((balance) => ({
          asset: balance.asset,
          free: balance.free,
          locked: balance.locked,
        })),
      warnings,
      "Balance bootstrap failed.",
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
      user: user ?? null,
      balances,
      openOrders,
      fills,
      warnings,
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
        price: intent.effectivePrice,
        quantity: intent.quantity,
      }),
    });

    return {
      orderId: payload.order.id,
      marketId: payload.order.market,
      marketName: intent.marketName,
      side: intent.side,
      orderType: intent.orderType,
      quantity: intent.quantity,
      requestedPrice: intent.requestedPrice,
      effectivePrice: intent.effectivePrice,
      resting: payload.resting,
      remaining: payload.order.remaining,
      fills: payload.fills.map(normalizeFill),
      createdAt: payload.order.created_at,
      syntheticMarket: intent.orderType === "market",
    };
  }

  private async request<T>(path: string, init?: RequestInit): Promise<T> {
    const response = await this.fetchImpl(joinUrl(this.baseUrl, path), {
      ...init,
      headers: {
        accept: "application/json",
        ...(init?.body ? { "content-type": "application/json" } : {}),
        ...(this.apiKey ? { "x-api-key": this.apiKey } : {}),
        ...init?.headers,
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
