"use client";

import {
  useEffect,
  useEffectEvent,
  useMemo,
  useReducer,
  useRef,
  startTransition,
} from "react";
import { TradeRestClient } from "@/components/trade/trade-rest-client";
import { createTradeRuntimeConfig, type TradeRuntimeConfig } from "@/components/trade/trade-runtime";
import {
  loadTradeMessageHistory,
  saveTradeMessageHistory,
} from "@/components/trade/trade-message-history";
import {
  createInitialTradeState,
  parseNumberInput,
  parseSharesInput,
  selectEstimatedCost,
  selectMarketById,
  selectSelectedMarketSummary,
  tradeReducer,
} from "@/components/trade/trade-store";
import {
  TradeWsClient,
  type TradeWsDelta,
  type TradeWsSnapshot,
  type WebSocketFactory,
} from "@/components/trade/trade-ws-client";
import type {
  ConnectionStatus,
  MarketDefinition,
  OrderType,
  TradeBootstrapData,
  TradeSide,
} from "@/components/trade/trade-types";

type RestClientFactory = (config: Pick<TradeRuntimeConfig, "httpUrl" | "apiKey">) => TradeRestClient;
type WsClientFactory = (
  config: Pick<TradeRuntimeConfig, "wsUrl" | "apiKey" | "reconnectDelayMs"> & {
    initialMarket: string;
  },
  callbacks: ConstructorParameters<typeof TradeWsClient>[1],
  createSocket?: WebSocketFactory,
) => TradeWsClient;

type UseTradeControllerOptions = {
  runtime?: TradeRuntimeConfig;
  restClientFactory?: RestClientFactory;
  wsClientFactory?: WsClientFactory;
  webSocketFactory?: WebSocketFactory;
};

type AccountSyncRequest = Partial<TradeBootstrapData["loaded"]>;

const EMPTY_ACCOUNT_SYNC_REQUEST: TradeBootstrapData["loaded"] = {
  markets: false,
  user: false,
  positions: false,
  openOrders: false,
  fills: false,
};

const timeFormatter = new Intl.DateTimeFormat("en-US", {
  hour: "2-digit",
  minute: "2-digit",
  second: "2-digit",
  hour12: false,
});

function createStamp() {
  const now = new Date();
  return {
    id: now.getTime(),
    time: timeFormatter.format(now),
  };
}

function mergeAccountSyncRequests(
  current: TradeBootstrapData["loaded"],
  next: AccountSyncRequest,
): TradeBootstrapData["loaded"] {
  return {
    markets: current.markets || Boolean(next.markets),
    user: current.user || Boolean(next.user),
    positions: current.positions || Boolean(next.positions),
    openOrders: current.openOrders || Boolean(next.openOrders),
    fills: current.fills || Boolean(next.fills),
  };
}

function hasAccountSyncRequest(request: TradeBootstrapData["loaded"]) {
  return Object.values(request).some(Boolean);
}

export function useTradeController(options: UseTradeControllerOptions = {}) {
  const runtime = useMemo(
    () => options.runtime ?? createTradeRuntimeConfig(),
    [options.runtime],
  );
  const initialMarketId = runtime.markets[0]?.id ?? "";
  const [state, dispatch] = useReducer(
    tradeReducer,
    runtime.markets,
    createInitialTradeState,
  );
  const socketRef = useRef<TradeWsClient | null>(null);
  const accountSyncRef = useRef({
    inFlight: false,
    queued: { ...EMPTY_ACCOUNT_SYNC_REQUEST },
  });
  const disposedRef = useRef(false);
  const hasAuthenticatedOnceRef = useRef(false);
  const hasHydratedMessageHistoryRef = useRef(false);
  const skipNextMessageHistorySaveRef = useRef(true);

  const restClient = useMemo(
    () =>
      (options.restClientFactory ?? ((config) => new TradeRestClient(config)))({
        httpUrl: runtime.httpUrl,
        apiKey: runtime.apiKey,
      }),
    [options.restClientFactory, runtime.apiKey, runtime.httpUrl],
  );

  const handleStatusChange = useEffectEvent((status: ConnectionStatus) => {
    startTransition(() => {
      dispatch({ type: "ws-status", status, ...createStamp() });
    });
  });

  const handleAuthenticated = useEffectEvent(
    (payload: { traderId: string; teamNumber: string }) => {
      startTransition(() => {
        dispatch({
          type: "ws-authenticated",
          user: payload,
          ...createStamp(),
        });
      });

      if (hasAuthenticatedOnceRef.current) {
        void refreshAccountState({
          positions: true,
          openOrders: true,
          fills: true,
        });
      } else {
        hasAuthenticatedOnceRef.current = true;
      }
    },
  );

  const handleSnapshot = useEffectEvent((payload: TradeWsSnapshot) => {
    startTransition(() => {
      dispatch({ type: "ws-snapshot", ...payload });
    });
  });

  const handleDelta = useEffectEvent((payload: TradeWsDelta) => {
    startTransition(() => {
      dispatch({ type: "ws-delta", ...payload, occurredAt: new Date().toISOString() });
    });
  });

  const handleSocketError = useEffectEvent((message: string) => {
    startTransition(() => {
      dispatch({ type: "bootstrap-error", error: message, ...createStamp() });
    });
  });

  async function refreshAccountState(request: AccountSyncRequest) {
    accountSyncRef.current.queued = mergeAccountSyncRequests(
      accountSyncRef.current.queued,
      request,
    );
    if (accountSyncRef.current.inFlight) {
      return;
    }

    accountSyncRef.current.inFlight = true;
    try {
      while (hasAccountSyncRequest(accountSyncRef.current.queued)) {
        const nextRequest = accountSyncRef.current.queued;
        accountSyncRef.current.queued = { ...EMPTY_ACCOUNT_SYNC_REQUEST };
        const data = await restClient.bootstrapAccountData(nextRequest);
        if (disposedRef.current) {
          return;
        }
        startTransition(() => {
          dispatch({ type: "account-sync", data });
        });
      }
    } catch (error) {
      if (disposedRef.current) {
        return;
      }
      startTransition(() => {
        dispatch({
          type: "bootstrap-error",
          error:
            error instanceof Error
              ? error.message
              : "Failed to resync account state from the exchange.",
          ...createStamp(),
        });
      });
    } finally {
      accountSyncRef.current.inFlight = false;
    }
  }

  const handleReject = useEffectEvent(
    (payload: { op: string; code: string; message: string }) => {
      startTransition(() => {
        dispatch({ type: "ws-reject", ...payload, ...createStamp() });
      });
    },
  );

  const handleFill = useEffectEvent(
    (fill: {
      fillId: string;
      market: string;
      makerOrderId: string;
      takerOrderId: string;
      price: number;
      quantity: number;
      occurredAt: string;
    }) => {
      startTransition(() => {
        dispatch({ type: "ws-fill", fill, ...createStamp() });
      });
    },
  );

  const handleOrderState = useEffectEvent(
    (payload: {
      order: {
        id: string;
        createdAt: string;
        marketId: string;
        marketName: string;
        side: "buy" | "sell";
        shares: number;
        limitPrice: number;
        status: "open" | "partial";
      };
      status: "open" | "filled" | "canceled";
    }) => {
      startTransition(() => {
        dispatch({ type: "ws-order-state", ...payload, ...createStamp() });
      });
    },
  );

  const handleMarketState = useEffectEvent((market: MarketDefinition) => {
    startTransition(() => {
      dispatch({ type: "ws-market-state", market });
    });
  });

  const handleMarketDeleted = useEffectEvent((payload: { marketId: string }) => {
    startTransition(() => {
      dispatch({ type: "ws-market-deleted", marketId: payload.marketId });
    });
  });

  const handleResyncRequired = useEffectEvent(
    (payload: {
      channel: string;
      marketId?: string;
      reason: string;
      autoHealing?: boolean;
    }) => {
      startTransition(() => {
        if (payload.channel === "data" && payload.marketId && !payload.autoHealing) {
          dispatch({ type: "ws-book-reset", marketId: payload.marketId });
        }
        if (!payload.autoHealing) {
          dispatch({ type: "ws-resync-required", ...payload, ...createStamp() });
        }
      });
      if (payload.channel === "markets") {
        void refreshAccountState({ markets: true });
      } else if (payload.channel === "user") {
        void refreshAccountState({
          positions: true,
          openOrders: true,
          fills: true,
        });
      }
    },
  );

  const handleAdminMessage = useEffectEvent(
    (payload: {
      level: "info" | "warning" | "critical";
      title?: string;
      body: string;
      market?: string;
    }) => {
      startTransition(() => {
        dispatch({ type: "ws-admin-message", ...payload, ...createStamp() });
      });
    },
  );

  useEffect(() => {
    if (hasHydratedMessageHistoryRef.current) {
      return;
    }

    hasHydratedMessageHistoryRef.current = true;
    const messages = loadTradeMessageHistory();
    if (messages.length === 0) {
      return;
    }

    startTransition(() => {
      dispatch({ type: "hydrate-messages", messages });
    });
  }, []);

  useEffect(() => {
    if (skipNextMessageHistorySaveRef.current) {
      skipNextMessageHistorySaveRef.current = false;
      return;
    }

    saveTradeMessageHistory(state.messages);
  }, [state.messages]);

  useEffect(() => {
    let cancelled = false;
    disposedRef.current = false;
    accountSyncRef.current = {
      inFlight: false,
      queued: { ...EMPTY_ACCOUNT_SYNC_REQUEST },
    };
    hasAuthenticatedOnceRef.current = false;

    startTransition(() => {
      dispatch({ type: "bootstrap-start", ...createStamp() });
    });

    restClient
      .bootstrapAccountData()
      .then((data) => {
        if (cancelled) {
          return;
        }

        startTransition(() => {
          dispatch({ type: "bootstrap-success", data, ...createStamp() });
        });
      })
      .catch((error: unknown) => {
        if (cancelled) {
          return;
        }

        startTransition(() => {
          dispatch({
            type: "bootstrap-error",
            error: error instanceof Error ? error.message : "Failed to bootstrap account state.",
            ...createStamp(),
          });
        });
      });

    const wsClient = (options.wsClientFactory ??
      ((config, callbacks, createSocket) =>
        new TradeWsClient(config, callbacks, createSocket)))(
      {
        wsUrl: runtime.wsUrl,
        apiKey: runtime.apiKey,
        reconnectDelayMs: runtime.reconnectDelayMs,
        initialMarket: initialMarketId,
      },
      {
        onStatusChange: handleStatusChange,
        onAuthenticated: handleAuthenticated,
        onSnapshot: handleSnapshot,
        onDelta: handleDelta,
        onReject: handleReject,
        onFill: handleFill,
        onOrderState: handleOrderState,
        onMarketState: handleMarketState,
        onMarketDeleted: handleMarketDeleted,
        onResyncRequired: handleResyncRequired,
        onAdminMessage: handleAdminMessage,
        onError: handleSocketError,
      },
      options.webSocketFactory,
    );

    socketRef.current = wsClient;
    wsClient.connect();

    return () => {
      cancelled = true;
      disposedRef.current = true;
      socketRef.current = null;
      wsClient.disconnect();
    };
    // Effect events always see the latest callback logic, so the connection
    // lifecycle only needs to track concrete runtime inputs.
  }, [
    options.webSocketFactory,
    options.wsClientFactory,
    restClient,
    runtime.apiKey,
    initialMarketId,
    runtime.reconnectDelayMs,
    runtime.wsUrl,
  ]);

  useEffect(() => {
    socketRef.current?.updateMarket(state.selectedMarketId);
  }, [state.selectedMarketId]);

  function resolveSelectedMarketForSubmit() {
    const selectedMarket = selectMarketById(state, state.selectedMarketId);
    if (!selectedMarket) {
      dispatch({
        type: "submit-error",
        error: "No market selected.",
        ...createStamp(),
      });
      return null;
    }

    const selectedMarketStatus = selectedMarket.status ?? "enabled";
    if (selectedMarketStatus !== "enabled") {
      dispatch({
        type: "submit-error",
        error:
          selectedMarketStatus === "settled"
            ? "Rejected order: market is settled."
            : "Rejected order: market is disabled.",
        ...createStamp(),
      });
      return null;
    }

    return {
      selectedMarket,
      summary: selectSelectedMarketSummary(state),
    };
  }

  async function submitResolvedOrder(intent: {
    marketId: string;
    marketName: string;
    side: TradeSide;
    orderType: OrderType;
    quantity: number;
    requestedPrice: number;
    effectivePrice: number;
  }) {
    dispatch({ type: "submit-start" });

    try {
      const result = await restClient.submitOrder(intent);

      startTransition(() => {
        dispatch({ type: "submit-success", result, ...createStamp() });
      });
    } catch (error) {
      startTransition(() => {
        dispatch({
          type: "submit-error",
          error:
            error instanceof Error
              ? error.message
              : "Order submission failed.",
          ...createStamp(),
        });
      });
    }
  }

  async function submitOrder() {
    const resolvedMarket = resolveSelectedMarketForSubmit();
    if (!resolvedMarket) {
      return;
    }

    const { selectedMarket, summary } = resolvedMarket;
    const shares = parseSharesInput(state.sharesInput);
    const requestedPrice = parseNumberInput(state.limitPriceInput);
    const effectivePrice =
      state.orderType === "market"
        ? state.ticketSide === "buy"
          ? summary.buyQuote ?? 0
          : summary.sellQuote ?? 0
        : requestedPrice;

    if (shares <= 0) {
      dispatch({
        type: "submit-error",
        error: "Rejected order: enter a valid share count.",
        ...createStamp(),
      });
      return;
    }

    if (effectivePrice <= 0) {
      dispatch({
        type: "submit-error",
        error:
          state.orderType === "market"
            ? "Rejected order: no live quote is available for a market order."
            : "Rejected order: enter a valid limit price.",
        ...createStamp(),
      });
      return;
    }

    await submitResolvedOrder({
      marketId: selectedMarket.id,
      marketName: selectedMarket.name,
      side: state.ticketSide,
      orderType: state.orderType,
      quantity: shares,
      requestedPrice,
      effectivePrice,
    });
  }

  async function cancelPendingOrder(orderId: string) {
    try {
      await restClient.cancelOrder(orderId);
      startTransition(() => {
        dispatch({ type: "cancel-success", orderId, ...createStamp() });
      });
    } catch (error) {
      startTransition(() => {
        dispatch({
          type: "cancel-error",
          error:
            error instanceof Error
              ? error.message
              : "Order cancellation failed.",
          ...createStamp(),
        });
      });
    }
  }

  function selectMarket(marketId: string) {
    dispatch({ type: "select-market", marketId, ...createStamp() });
  }

  function setSide(side: TradeSide) {
    dispatch({ type: "set-side", side });
  }

  function setPositionFilter(filter: "active" | "pending") {
    dispatch({ type: "set-position-filter", filter });
  }

  function setOrderType(orderType: "limit" | "market") {
    dispatch({ type: "set-order-type", orderType });
  }

  function setLimitPrice(value: string) {
    dispatch({ type: "set-limit-price", value });
  }

  function setShares(value: string) {
    dispatch({ type: "set-shares", value });
  }

  function adjustShares(delta: number) {
    dispatch({ type: "adjust-shares", delta });
  }

  return {
    runtime,
    state,
    derived: {
      summary: selectSelectedMarketSummary(state),
      estimated: selectEstimatedCost(state),
    },
    actions: {
      selectMarket,
      setSide,
      setPositionFilter,
      setOrderType,
      setLimitPrice,
      setShares,
      adjustShares,
      cancelPendingOrder,
      submitOrder,
    },
  };
}
