import type {
  AccountPosition,
  MarketDefinition,
  MarketStatus,
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
  team_number: string;
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
  min_price?: number | null;
  max_price?: number | null;
  status?: MarketStatus;
};

type BootstrapRequest = Partial<TradeBootstrapData["loaded"]>;

const EMPTY_BOOTSTRAP_LOAD_STATE: TradeBootstrapData["loaded"] = {
  markets: false,
  user: false,
  positions: false,
  openOrders: false,
  fills: false,
};

const FULL_BOOTSTRAP_REQUEST: TradeBootstrapData["loaded"] = {
  markets: true,
  user: true,
  positions: true,
  openOrders: true,
  fills: true,
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
    minPrice: market.min_price ?? null,
    maxPrice: market.max_price ?? null,
    status: market.status ?? "enabled",
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

  async bootstrapAccountData(requested: BootstrapRequest = FULL_BOOTSTRAP_REQUEST): Promise<TradeBootstrapData> {
    const request = {
      ...EMPTY_BOOTSTRAP_LOAD_STATE,
      ...requested,
    };

    if (!this.apiKey) {
      return {
        markets: [],
        user: null,
        positions: [],
        openOrders: [],
        fills: [],
        warnings: ["No exchange API key configured. Account bootstrap skipped."],
        loaded: EMPTY_BOOTSTRAP_LOAD_STATE,
      };
    }

    const warnings: string[] = [];
    const loaded = { ...EMPTY_BOOTSTRAP_LOAD_STATE };
    let markets: MarketDefinition[] = [];
    let user: TradeBootstrapData["user"] = null;
    let positions: AccountPosition[] = [];
    let openOrders: PendingOrder[] = [];
    let fills: TradeFill[] = [];
    let userError: unknown = null;

    await Promise.all([
      request.markets
        ? this.request<MarketResponse[]>("/api/v1/markets", { includeAuth: false })
            .then((value) => {
              markets = value.map(normalizeMarket);
              loaded.markets = true;
            })
            .catch((error: unknown) => {
              warnings.push(buildWarningMessage("Market bootstrap failed.", error));
            })
        : Promise.resolve(),
      request.user
        ? this.request<UserResponse>("/api/v1/user")
            .then((value) => {
              user = {
                traderId: value.trader_id,
                teamNumber: value.team_number,
              };
              loaded.user = true;
            })
            .catch((error: unknown) => {
              userError = error;
            })
        : Promise.resolve(),
      request.positions
        ? this.request<PositionResponse[]>("/api/v1/positions")
            .then((value) => {
              positions = value.map((position) => ({
                market: position.market,
                netQuantity: position.net_quantity,
                averageEntryPrice: position.average_entry_price,
                realizedPnl: position.realized_pnl,
              }));
              loaded.positions = true;
            })
            .catch((error: unknown) => {
              warnings.push(buildWarningMessage("Position bootstrap failed.", error));
            })
        : Promise.resolve(),
      request.openOrders
        ? this.request<OpenOrderResponse[]>("/api/v1/open-orders")
            .then((value) => {
              openOrders = value.map(normalizePendingOrder);
              loaded.openOrders = true;
            })
            .catch((error: unknown) => {
              warnings.push(buildWarningMessage("Open order bootstrap failed.", error));
            })
        : Promise.resolve(),
      request.fills
        ? this.request<FillResponse[]>("/api/v1/fills")
            .then((value) => {
              fills = value.map(normalizeFill);
              loaded.fills = true;
            })
            .catch((error: unknown) => {
              warnings.push(buildWarningMessage("Fill bootstrap failed.", error));
            })
        : Promise.resolve(),
    ]);

    if (userError) {
      throw userError;
    }

    return {
      markets,
      user: user ?? null,
      positions,
      openOrders,
      fills,
      warnings,
      loaded,
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

function buildWarningMessage(prefix: string, error: unknown) {
  const reason = error instanceof Error ? error.message : String(error);
  return `${prefix} ${reason}`;
}
