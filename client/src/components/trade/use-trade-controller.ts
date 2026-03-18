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
import type { ConnectionStatus, TradeSide } from "@/components/trade/trade-types";

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

export function useTradeController(options: UseTradeControllerOptions = {}) {
  const runtime = useMemo(
    () => options.runtime ?? createTradeRuntimeConfig(),
    [options.runtime],
  );
  const initialMarketId = runtime.markets[0]?.id ?? "BTC-USD";
  const [state, dispatch] = useReducer(
    tradeReducer,
    runtime.markets,
    createInitialTradeState,
  );
  const socketRef = useRef<TradeWsClient | null>(null);
  const accountSyncRef = useRef({ inFlight: false, queued: false });
  const disposedRef = useRef(false);

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
    (payload: { traderId: string; username: string }) => {
      startTransition(() => {
        dispatch({
          type: "ws-authenticated",
          user: payload,
          ...createStamp(),
        });
      });
    },
  );

  const handleSnapshot = useEffectEvent((payload: TradeWsSnapshot) => {
    startTransition(() => {
      dispatch({ type: "ws-snapshot", ...payload });
    });
  });

  const handleDelta = useEffectEvent((payload: TradeWsDelta) => {
    startTransition(() => {
      dispatch({ type: "ws-delta", ...payload });
    });
  });

  const handleSocketError = useEffectEvent((message: string) => {
    startTransition(() => {
      dispatch({ type: "bootstrap-error", error: message, ...createStamp() });
    });
  });

  const refreshAccountState = useEffectEvent(async () => {
    if (accountSyncRef.current.inFlight) {
      accountSyncRef.current.queued = true;
      return;
    }

    accountSyncRef.current.inFlight = true;
    try {
      do {
        accountSyncRef.current.queued = false;
        const data = await restClient.bootstrapAccountData();
        if (disposedRef.current) {
          return;
        }
        startTransition(() => {
          dispatch({ type: "account-sync", data });
        });
      } while (accountSyncRef.current.queued);
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
  });

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
      void refreshAccountState();
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
      void refreshAccountState();
    },
  );

  const handleResyncRequired = useEffectEvent(
    (payload: { channel: string; marketId?: string; reason: string }) => {
      startTransition(() => {
        dispatch({ type: "ws-resync-required", ...payload, ...createStamp() });
      });
      if (payload.channel !== "l3") {
        void refreshAccountState();
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
    let cancelled = false;
    disposedRef.current = false;
    accountSyncRef.current = { inFlight: false, queued: false };

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

  async function submitOrder() {
    const selectedMarket = selectMarketById(state, state.selectedMarketId);
    if (!selectedMarket) {
      dispatch({
        type: "submit-error",
        error: "No market selected.",
        ...createStamp(),
      });
      return;
    }

    const summary = selectSelectedMarketSummary(state);
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

    dispatch({ type: "submit-start" });

    try {
      const result = await restClient.submitOrder({
        marketId: selectedMarket.id,
        marketName: selectedMarket.name,
        side: state.ticketSide,
        orderType: state.orderType,
        quantity: shares,
        requestedPrice,
        effectivePrice,
      });

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
      submitOrder,
    },
  };
}
