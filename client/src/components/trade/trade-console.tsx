"use client";

import Image from "next/image";
import { useEffect, useRef, useState } from "react";
import { useTradeController } from "@/components/trade/use-trade-controller";
import type { TradeRuntimeConfig } from "@/components/trade/trade-runtime";
import {
  formatBookTotal,
  formatMaybePrice,
  formatPrice,
  initialsForUser,
  selectActiveRows,
  selectPendingRows,
  selectPnlMetrics,
  selectSelectedMarket,
} from "@/components/trade/trade-store";
import type { AggregatedBookLevel, MessageTone, PnlMetric } from "@/components/trade/trade-types";

const contentColumns = "minmax(0, 373fr) minmax(0, 722fr) minmax(0, 298fr)";
const leftColumnRows = "minmax(0, 500fr) minmax(0, 360fr)";
const rightColumnRows = "minmax(0, 430fr) minmax(0, 430fr)";
const panelBaseClass = "rounded-[10px] border border-[#26272b] bg-[#141416]";
const quickAdjustments = [-100, -10, 10, 100];

function ShortcutHint({ keys }: { keys: string }) {
  return (
    <span className="text-[10px] font-medium uppercase tracking-[0.08em] text-[#9a9aa2]">
      ({keys})
    </span>
  );
}

function metricToneClass(tone: PnlMetric["tone"]) {
  if (tone === "positive") {
    return "text-[#70ff6c]";
  }

  if (tone === "negative") {
    return "text-[#ff6c6c]";
  }

  if (tone === "primary") {
    return "text-[#f5f5f5]";
  }

  return "text-[#bababa]";
}

function messageToneClass(tone: MessageTone) {
  if (tone === "positive") {
    return "text-[#70ff6c]";
  }

  if (tone === "negative") {
    return "text-[#ff6c6c]";
  }

  return "text-[#c7c7cb]";
}

function messageCardToneClass(tone: MessageTone) {
  if (tone === "positive") {
    return "border-[#24452a] bg-[#101611]";
  }

  if (tone === "negative") {
    return "border-[#4c2626] bg-[#171011]";
  }

  return "border-[#222327] bg-[#111114]";
}

function OrderBookRow({
  level,
}: {
  level: AggregatedBookLevel | null;
}) {
  const priceClass = "text-white";

  return (
    <div className="grid grid-cols-[1fr_1fr_1fr] items-center text-[14px] leading-[18px] font-medium font-mono tabular-nums">
      <span className={level ? priceClass : "text-transparent"}>
        {level ? formatPrice(level.price) : "--"}
      </span>
      <span className="justify-self-center text-white">
        {level ? level.liquidity : ""}
      </span>
      <span className="justify-self-end text-[#a4a4a4]">
        {level ? formatBookTotal(level.total) : ""}
      </span>
    </div>
  );
}

function padLevels(levels: AggregatedBookLevel[], count: number) {
  return Array.from({ length: count }, (_, index) => levels[index] ?? null);
}

function connectionPresentation(status: string) {
  if (status === "connected") {
    return { label: "Connected", dotClass: "bg-[#40d9ff]" };
  }

  if (status === "reconnecting") {
    return { label: "Reconnecting", dotClass: "bg-[#f0c15b]" };
  }

  if (status === "disconnected") {
    return { label: "Disconnected", dotClass: "bg-[#ff6c6c]" };
  }

  return { label: "Connecting", dotClass: "bg-[#8a8a92]" };
}

function teamLabelForUser(traderId?: string) {
  if (!traderId) {
    return "Team --";
  }

  const match = traderId.match(/(\d+)(?!.*\d)/);
  return match ? `Team ${match[1]}` : `Team ${traderId}`;
}

function formatNetQuantity(value: number) {
  return value > 0 ? `+${value}` : String(value);
}

function isEditableTarget(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) {
    return false;
  }

  return (
    target.isContentEditable ||
    target.tagName === "INPUT" ||
    target.tagName === "TEXTAREA" ||
    target.tagName === "SELECT"
  );
}

type TradeConsoleViewProps = {
  controller: ReturnType<typeof useTradeController>;
};

export function TradeConsoleView({ controller }: TradeConsoleViewProps) {
  const [isOrderTypeMenuOpen, setIsOrderTypeMenuOpen] = useState(false);
  const [isProfileMenuOpen, setIsProfileMenuOpen] = useState(false);
  const [cancelingOrderIds, setCancelingOrderIds] = useState<string[]>([]);
  const priceInputRef = useRef<HTMLInputElement | null>(null);
  const sharesInputRef = useRef<HTMLInputElement | null>(null);
  const { state, derived, actions } = controller;
  const selectedMarket = selectSelectedMarket(state);
  const activeRows = selectActiveRows(state);
  const pendingRows = selectPendingRows(state);
  const hasPositionRows = activeRows.length > 0 || pendingRows.length > 0;
  const pnlMetrics = selectPnlMetrics(state);
  const visibleMessages = [...state.messages].reverse();
  const summary = derived.summary;
  const askLevels = padLevels(summary.asks, 7).reverse();
  const bidLevels = padLevels(summary.bids, 7);
  const connection = connectionPresentation(state.connectionStatus);
  const initials = initialsForUser(state.user);
  const profileName = state.user?.username ?? "Competition User";
  const profileTeam = teamLabelForUser(state.user?.traderId);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.metaKey || event.ctrlKey || event.altKey) {
        return;
      }

      const targetIsEditable = isEditableTarget(event.target);
      const key = event.key.toLowerCase();

      if (event.key === "Enter") {
        event.preventDefault();
        void actions.submitOrder();
        return;
      }

      if (targetIsEditable) {
        return;
      }

      switch (key) {
        case "b":
          event.preventDefault();
          actions.setSide("buy");
          break;
        case "s":
          event.preventDefault();
          actions.setSide("sell");
          break;
        case "l":
          event.preventDefault();
          actions.setOrderType("limit");
          setIsOrderTypeMenuOpen(false);
          break;
        case "m":
          event.preventDefault();
          actions.setOrderType("market");
          setIsOrderTypeMenuOpen(false);
          break;
        case "p":
          event.preventDefault();
          if (state.orderType === "market") {
            actions.setOrderType("limit");
            setIsOrderTypeMenuOpen(false);
            requestAnimationFrame(() => {
              priceInputRef.current?.focus();
              priceInputRef.current?.select();
            });
          } else {
            priceInputRef.current?.focus();
            priceInputRef.current?.select();
          }
          break;
        case "q":
          event.preventDefault();
          sharesInputRef.current?.focus();
          sharesInputRef.current?.select();
          break;
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [actions, state.orderType]);

  async function handleCancelPendingOrder(orderId: string) {
    if (cancelingOrderIds.includes(orderId)) {
      return;
    }

    setCancelingOrderIds((current) => [...current, orderId]);
    try {
      await actions.cancelPendingOrder(orderId);
    } finally {
      setCancelingOrderIds((current) => current.filter((currentOrderId) => currentOrderId !== orderId));
    }
  }

  return (
    <div
      className="h-screen w-screen overflow-hidden bg-black"
      data-testid="trade-console-root"
    >
      <div
        className="grid h-full w-full grid-rows-[88px_14px_minmax(0,1fr)] bg-black"
        data-testid="trade-console-shell"
      >
        <header
          className="h-[88px] bg-black px-[clamp(18px,2.6vw,40px)] pt-[10px]"
          data-testid="trade-console-header"
        >
          <div className="surface-panel-soft rounded-[10px] grid h-[68px] grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-[14px] px-[14px]">
            <div className="flex min-w-0 items-center gap-[12px]">
              <div className="flex h-[44px] items-center rounded-[8px] border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-[14px] shadow-[inset_0_1px_0_rgba(255,255,255,0.03)]">
                <Image alt="Quant" height={32} src="/quant.png" width={124} />
              </div>
              <div className="min-w-0">
                <p className="text-[11px] font-semibold uppercase tracking-[0.24em] text-[var(--muted)]">
                  Exchange
                </p>
                <p className="truncate text-[15px] font-medium leading-none text-[var(--muted-strong)]">
                  Competition Console
                </p>
              </div>
            </div>

            <nav aria-label="Markets" className="min-w-0 px-[4px]">
              <div className="flex max-w-full items-center justify-center overflow-x-auto">
                <div className="flex items-center gap-[8px] rounded-[10px] border border-[var(--surface-stroke)] bg-[var(--surface-soft)] p-[4px]">
                  {state.availableMarkets.map((market) => {
                    const isSelected = state.selectedMarketId === market.id;

                    return (
                      <button
                        className={
                          isSelected
                            ? "rounded-[6px] bg-white px-[16px] py-[10px] text-[15px] font-bold leading-none whitespace-nowrap text-black"
                            : "rounded-[6px] px-[16px] py-[10px] text-[15px] font-semibold leading-none whitespace-nowrap text-[var(--muted-strong)] hover:bg-[rgba(255,255,255,0.04)] hover:text-white"
                        }
                        key={market.id}
                        onClick={() => actions.selectMarket(market.id)}
                        type="button"
                      >
                        {market.name}
                      </button>
                    );
                  })}
                </div>
              </div>
            </nav>

            <div className="flex items-center justify-end gap-[10px]">
              <a
                className="inline-flex items-center rounded-[8px] border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-[14px] py-[10px] text-[13px] font-semibold leading-none whitespace-nowrap text-[var(--muted-strong)] hover:border-[rgba(66,204,78,0.42)] hover:text-white"
                href="https://jamesxu.mintlify.app/"
                rel="noreferrer"
                target="_blank"
              >
                API Docs
              </a>
              <div className="inline-flex items-center gap-[8px] rounded-[8px] border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-[12px] py-[10px] text-[13px] font-medium leading-none whitespace-nowrap text-white">
                <span
                  className={`h-[8px] w-[8px] rounded-full shadow-[0_0_14px_rgba(255,255,255,0.18)] ${connection.dotClass}`}
                />
                <span>{connection.label}</span>
              </div>
              <div className="relative">
                <button
                  aria-expanded={isProfileMenuOpen}
                  aria-label="Open profile menu"
                  aria-haspopup="menu"
                  className="flex h-[44px] items-center gap-[8px] rounded-[8px] border border-[var(--surface-stroke)] bg-[var(--surface-soft)] py-[6px] pl-[6px] pr-[10px] hover:border-[rgba(66,204,78,0.42)]"
                  onClick={() => setIsProfileMenuOpen((current) => !current)}
                  title={`${profileName} · ${profileTeam}`}
                  type="button"
                >
                  <span className="flex h-[32px] w-[32px] items-center justify-center rounded-[8px] bg-[#efebe3] text-[13px] font-semibold leading-none text-black">
                    {initials}
                  </span>
                  <span className="h-[6px] w-[6px] rounded-full bg-[var(--muted)]" />
                </button>

                {isProfileMenuOpen ? (
                  <div className="surface-panel absolute right-0 top-[calc(100%+10px)] z-20 w-[240px] rounded-[10px] p-[8px]">
                    <div className="surface-panel-soft rounded-[8px] px-[12px] py-[10px]">
                      <p className="text-[15px] font-semibold leading-none text-white">{profileName}</p>
                      <p className="mt-[8px] text-[13px] font-medium leading-none text-[var(--muted)]">
                        {profileTeam}
                      </p>
                    </div>
                    <form action="/api/auth/logout" className="mt-[8px]" method="post">
                      <button
                        className="w-full rounded-[8px] border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-[12px] py-[10px] text-[13px] font-semibold leading-none text-[var(--muted-strong)] hover:border-[rgba(66,204,78,0.42)] hover:text-white"
                        type="submit"
                      >
                        Log out
                      </button>
                    </form>
                  </div>
                ) : null}
              </div>
            </div>
          </div>
        </header>

          <div />

          <div
            className="grid min-h-0 px-[clamp(18px,2.6vw,40px)] pb-[clamp(12px,2vh,20px)]"
            data-testid="trade-console-content"
            style={{
              columnGap: "clamp(12px, 1.4vw, 20px)",
              gridTemplateColumns: contentColumns,
            }}
          >
            <div
              className="grid min-h-0"
              style={{
                gridTemplateRows: leftColumnRows,
                rowGap: "clamp(12px, 1.6vh, 20px)",
              }}
            >
              <section
                className={`${panelBaseClass} grid h-full min-h-0 grid-rows-[48px_1fr] overflow-hidden`}
                data-testid="positions-panel"
              >
                <div className="flex items-center justify-between border-b border-[#2c2d31] px-[20px] pt-[10px]">
                  <h2 className="text-[21px] font-bold leading-none text-white">
                    Positions
                  </h2>
                  <p className="text-[13px] font-medium leading-none text-[#8a8a92]">
                    {activeRows.length} active · {pendingRows.length} pending
                  </p>
                </div>

                <div className="min-h-0 overflow-y-auto px-[20px] py-[12px] text-[16px] font-medium leading-none text-white">
                  {hasPositionRows ? (
                    <div className="grid content-start gap-y-[22px]">
                      <div className="grid content-start gap-y-[14px]">
                        <div className="flex items-center justify-between">
                          <p className="text-[13px] font-semibold uppercase tracking-[0.18em] text-[#8a8a92]">
                            Active
                          </p>
                          <p className="text-[12px] font-medium text-[#6f6f76]">
                            Net exposure by market
                          </p>
                        </div>
                        <div className="grid grid-cols-[1.25fr_0.7fr_0.8fr] items-center border-b border-[#2c2d31] pb-[10px] text-[14px] font-bold leading-none text-white">
                          <span>Product</span>
                          <span>Net</span>
                          <span className="justify-self-end">Avg. Cost</span>
                        </div>
                        {activeRows.length > 0 ? (
                          <div className="grid content-start gap-y-[14px]">
                            {activeRows.map((position) => (
                              <div
                                className="grid grid-cols-[1.25fr_0.7fr_0.8fr] items-start gap-x-[10px]"
                                key={position.marketId}
                              >
                                <span
                                  className={
                                    position.marketId === state.selectedMarketId
                                      ? "max-h-[34px] min-w-0 overflow-hidden break-words leading-[17px] text-[#f5f5f5]"
                                      : "max-h-[34px] min-w-0 overflow-hidden break-words leading-[17px] text-[#b8b8bc]"
                                  }
                                >
                                  {position.product}
                                </span>
                                <span>{formatNetQuantity(position.netQuantity)}</span>
                                <span className="justify-self-end">
                                  {formatMaybePrice(position.avgCost)}
                                </span>
                              </div>
                            ))}
                          </div>
                        ) : (
                          <p className="text-[15px] leading-[1.2] text-[#8a8a92]">
                            No active positions.
                          </p>
                        )}
                      </div>

                      <div className="grid content-start gap-y-[14px]">
                        <div className="flex items-center justify-between">
                          <p className="text-[13px] font-semibold uppercase tracking-[0.18em] text-[#8a8a92]">
                            Pending
                          </p>
                          <p className="text-[12px] font-medium text-[#6f6f76]">
                            Resting orders
                          </p>
                        </div>
                        <div className="grid grid-cols-[1.15fr_0.6fr_0.75fr_auto] items-center gap-x-[10px] border-b border-[#2c2d31] pb-[10px] text-[14px] font-bold leading-none text-white">
                          <span>Product</span>
                          <span>Qty</span>
                          <span className="justify-self-end">Order</span>
                          <span className="justify-self-end">Action</span>
                        </div>
                        {pendingRows.length > 0 ? (
                          <div className="grid content-start gap-y-[14px]">
                            {pendingRows.map((order) => {
                              const isCanceling = cancelingOrderIds.includes(order.id);
                              return (
                                <div
                                  className="grid grid-cols-[1.15fr_0.6fr_0.75fr_auto] items-start gap-x-[10px]"
                                  key={order.id}
                                >
                                  <span className="max-h-[34px] min-w-0 overflow-hidden break-words leading-[17px] text-[#f5f5f5]">
                                    {order.marketName}
                                  </span>
                                  <span>{order.shares}</span>
                                  <span className="justify-self-end text-right">
                                    {order.side === "buy" ? "B" : "S"} {formatPrice(order.limitPrice)}
                                  </span>
                                  <button
                                    aria-label={`Cancel order ${order.id}`}
                                    className="justify-self-end rounded-[6px] border border-[#32333a] px-[10px] py-[4px] text-[11px] font-semibold uppercase tracking-[0.08em] text-[#d9d9dc] transition hover:border-[#50515a] hover:text-white disabled:cursor-not-allowed disabled:border-[#26272b] disabled:text-[#6f6f76]"
                                    disabled={isCanceling}
                                    onClick={() => void handleCancelPendingOrder(order.id)}
                                    type="button"
                                  >
                                    {isCanceling ? "Canceling" : "Cancel"}
                                  </button>
                                </div>
                              );
                            })}
                          </div>
                        ) : (
                          <p className="text-[15px] leading-[1.2] text-[#8a8a92]">
                            No pending orders.
                          </p>
                        )}
                      </div>
                    </div>
                  ) : (
                    <div className="flex h-full items-center justify-center text-center text-[16px] leading-[1.2] text-[#8a8a92]">
                      No positions or pending orders yet.
                    </div>
                  )}
                </div>
              </section>

              <section className={`${panelBaseClass} grid min-h-0 grid-rows-[46px_1fr] overflow-hidden`}>
                <div className="flex items-center border-b border-[#2c2d31] px-[20px]">
                  <h2 className="text-[21px] font-bold leading-none text-white">Statistics</h2>
                </div>

                <div className="min-h-0 px-[20px] py-[14px]">
                  <div className="space-y-[6px]">
                    {pnlMetrics.map((metric) => (
                      <div
                        className="flex items-center justify-between text-[18px] font-semibold leading-none"
                        key={metric.label}
                      >
                        <span
                          className={
                            metric.tone === "primary"
                              ? "text-[#f5f5f5]"
                              : "text-[#949494]"
                          }
                        >
                          {metric.label}
                        </span>
                        <span className={metricToneClass(metric.tone)}>
                          {metric.value}
                        </span>
                      </div>
                    ))}
                  </div>
                </div>
              </section>
            </div>

            <section
              className={`${panelBaseClass} grid h-full min-h-0 grid-rows-[52px_1fr_63px] overflow-hidden`}
              data-testid="orderbook-panel"
            >
              <div className="grid grid-cols-[1fr_1fr_1fr] items-start border-b border-[#26272b] px-[66px] pt-[14px] text-[16px] font-bold leading-none text-[#aaa]">
                <span>Price</span>
                <span className="justify-self-center">Liquidity</span>
                <span className="justify-self-end">Total</span>
              </div>

              <div className="min-h-0 overflow-hidden">
                <div className="h-full px-[66px] pt-[52px]">
                  <div className="space-y-[12px]">
                    {askLevels.slice(0, 6).map((level, index) => (
                      <OrderBookRow
                        key={`ask-${selectedMarket?.id ?? "market"}-${index}`}
                        level={level}
                      />
                    ))}
                  </div>

                  <div className="mt-[12px]">
                    <OrderBookRow level={askLevels[6]} />
                  </div>

                  <div className="-mx-[66px] mt-[24px] grid grid-cols-[1fr_1fr_1fr] border-y border-[#26272b] px-[66px] py-[9px] text-[14px] font-medium leading-none text-[#aaa]">
                    <p>
                      Last:{" "}
                      <span className="font-mono font-bold text-white">
                        {formatMaybePrice(summary.lastPrice)}
                      </span>
                    </p>
                    <p className="justify-self-center">
                      Mid:{" "}
                      <span className="font-mono font-bold text-white">
                        {formatMaybePrice(summary.midPrice)}
                      </span>
                    </p>
                    <p className="justify-self-end">
                      Spread:{" "}
                      <span className="font-mono font-bold text-white">
                        {formatMaybePrice(summary.spread)}
                      </span>
                    </p>
                  </div>

                  <div className="mt-[15px] space-y-[12px]">
                    {bidLevels.map((level, index) => (
                      <OrderBookRow
                        key={`bid-${selectedMarket?.id ?? "market"}-${index}`}
                        level={level}
                      />
                    ))}
                  </div>
                </div>
              </div>

              <div className="flex items-center justify-end gap-[12px] border-t border-[#26272b] px-[20px]">
                <span className="rounded-[4px] border border-[#2c2d31] bg-[#111114] px-[12px] py-[8px] text-[13px] font-semibold text-[#d8d8dc]">
                  Live orderbook
                </span>
              </div>
            </section>

            <div
              className="grid min-h-0"
              style={{
                gridTemplateRows: rightColumnRows,
                rowGap: "clamp(12px, 1.6vh, 20px)",
              }}
            >
              <section
                className="grid h-full min-h-0 grid-rows-[44px_1fr] overflow-hidden rounded-[6px] border-[0.595px] border-[#26272b] bg-[rgba(24,24,27,0.82)]"
                data-testid="ticket-panel"
              >
                <div className="flex items-center justify-between border-b border-[#2c2d31] px-[13px] py-[10px]">
                  <p className="max-w-[180px] text-[15.477px] font-bold leading-none text-white">
                    {selectedMarket?.name ?? "--"}
                  </p>

                  <div className="relative">
                    <button
                      className="flex items-center gap-[5px] text-[15.477px] font-medium leading-none text-white"
                      onClick={() => setIsOrderTypeMenuOpen((current) => !current)}
                      type="button"
                    >
                      <span>{state.orderType === "limit" ? "Limit" : "Market"}</span>
                      <ShortcutHint keys={state.orderType === "limit" ? "L" : "M"} />
                      <Image alt="" height={14} src="/chevron.svg" width={14} />
                    </button>

                    {isOrderTypeMenuOpen ? (
                      <div className="absolute right-0 top-[calc(100%+8px)] z-10 w-[126px] rounded-[7px] border border-[#2c2d31] bg-[#18181b] p-[6px] shadow-[0_12px_32px_rgba(0,0,0,0.35)]">
                        {(["limit", "market"] as const).map((orderType) => (
                          <button
                            className={
                              state.orderType === orderType
                                ? "flex w-full items-center justify-between rounded-[5px] bg-[#26272b] px-[10px] py-[8px] text-left text-[15px] font-semibold text-white"
                                : "flex w-full items-center justify-between rounded-[5px] px-[10px] py-[8px] text-left text-[15px] font-medium text-[#b8b8bc]"
                            }
                            key={orderType}
                            onClick={() => {
                              actions.setOrderType(orderType);
                              setIsOrderTypeMenuOpen(false);
                            }}
                            type="button"
                          >
                            <span>{orderType === "limit" ? "Limit" : "Market"}</span>
                            <ShortcutHint keys={orderType === "limit" ? "L" : "M"} />
                          </button>
                        ))}
                      </div>
                    ) : null}
                  </div>
                </div>

                <div className="flex h-full flex-col px-[13px] pt-[12px] pb-[10px]">
                  <div className="grid grid-cols-2 gap-[12px]">
                    <button
                      className={
                        state.ticketSide === "buy"
                          ? "h-[42px] rounded-[3px] bg-[#42cc4e] text-[16px] font-bold leading-none text-white"
                          : "h-[42px] rounded-[3px] bg-[#26272b] text-[16px] font-bold leading-none text-white"
                      }
                      onClick={() => actions.setSide("buy")}
                      type="button"
                    >
                      <span className="inline-flex items-center gap-[4px] text-[#e2e2e2]">
                        <span>Buy</span>
                        <ShortcutHint keys="B" />
                      </span>{" "}
                      {formatMaybePrice(summary.buyQuote)}
                    </button>
                    <button
                      className={
                        state.ticketSide === "sell"
                          ? "h-[42px] rounded-[3px] bg-[#d85b5b] text-[16px] font-bold leading-none text-white"
                          : "h-[42px] rounded-[3px] bg-[#26272b] text-[16px] font-bold leading-none text-white"
                      }
                      onClick={() => actions.setSide("sell")}
                      type="button"
                    >
                      <span className="inline-flex items-center gap-[4px] text-[#e2e2e2]">
                        <span>Sell</span>
                        <ShortcutHint keys="S" />
                      </span>{" "}
                      {formatMaybePrice(summary.sellQuote)}
                    </button>
                  </div>

                  <div className="mt-[20px] grid grid-cols-[1fr_148px] items-center">
                    <span className="inline-flex items-center gap-[6px] text-[16px] font-medium leading-none text-white">
                      <span>{state.orderType === "market" ? "Market Price" : "Limit Price"}</span>
                      <ShortcutHint keys="P" />
                    </span>
                    <label className="flex h-[34px] items-center justify-center rounded-[6px] border border-[#666] bg-[#18181b] text-[16px] font-bold leading-none text-white">
                      <input
                        aria-label={state.orderType === "market" ? "Market Price" : "Limit Price"}
                        className="w-full bg-transparent px-[14px] text-center outline-none disabled:text-[#b8b8bc]"
                        disabled={state.orderType === "market"}
                        inputMode="numeric"
                        onChange={(event) => actions.setLimitPrice(event.target.value)}
                        pattern="[0-9]*"
                        ref={priceInputRef}
                        value={
                          state.orderType === "market"
                            ? derived.estimated.derivedPrice > 0
                              ? String(Math.trunc(derived.estimated.derivedPrice))
                              : "--"
                            : state.limitPriceInput
                        }
                      />
                    </label>
                  </div>

                  <div className="mt-[16px] grid grid-cols-[1fr_148px] items-center">
                    <span className="inline-flex items-center gap-[6px] text-[16px] font-medium leading-none text-white">
                      <span>Shares</span>
                      <ShortcutHint keys="Q" />
                    </span>
                    <div className="flex h-[34px] items-center justify-between rounded-[6px] border border-[#666] bg-[#18181b] px-[5px]">
                      <button
                        className="flex h-[24px] w-[24px] items-center justify-center"
                        onClick={() => actions.adjustShares(-1)}
                        type="button"
                      >
                        <Image alt="" height={14} src="/minus.svg" width={14} />
                      </button>
                      <input
                        aria-label="Shares"
                        className="w-[52px] bg-transparent text-center text-[16px] font-bold leading-none text-white outline-none"
                        inputMode="numeric"
                        onChange={(event) => actions.setShares(event.target.value)}
                        ref={sharesInputRef}
                        value={state.sharesInput}
                      />
                      <button
                        className="flex h-[24px] w-[24px] items-center justify-center"
                        onClick={() => actions.adjustShares(1)}
                        type="button"
                      >
                        <Image alt="" height={14} src="/plus.svg" width={14} />
                      </button>
                    </div>
                  </div>

                  <div className="mt-[8px] flex justify-end gap-[8px]">
                    {quickAdjustments.map((adjustment) => (
                      <button
                        className="flex h-[22px] min-w-[34px] items-center justify-center rounded-[4px] border border-[#d5d5d5] px-[6px] text-[11px] font-semibold leading-none text-[#d5d5d5]"
                        key={adjustment}
                        onClick={() => actions.adjustShares(adjustment)}
                        type="button"
                      >
                        {adjustment > 0 ? `+${adjustment}` : adjustment}
                      </button>
                    ))}
                  </div>

                  <div className="mt-[16px] flex items-center justify-between text-[16px] font-bold leading-none text-white">
                    <span>{state.orderType === "market" ? "Est. Cost" : "Cost"}</span>
                    <span>{formatMaybePrice(derived.estimated.estimatedCost)}</span>
                  </div>

                  <button
                    className={
                      state.ticketSide === "buy"
                        ? "mt-auto h-[44px] w-full rounded-[6px] bg-[#42cc4e] text-[16px] font-bold leading-none text-white disabled:cursor-not-allowed disabled:opacity-60"
                        : "mt-auto h-[44px] w-full rounded-[6px] bg-[#d85b5b] text-[16px] font-bold leading-none text-white disabled:cursor-not-allowed disabled:opacity-60"
                    }
                    disabled={state.isSubmitting}
                    onClick={() => {
                      void actions.submitOrder();
                    }}
                    type="button"
                  >
                    <span className="inline-flex items-center gap-[6px]">
                      <span>
                        {state.isSubmitting
                          ? "Submitting..."
                          : `${state.orderType === "market" ? "Market" : "Limit"} ${
                              state.ticketSide === "buy" ? "Buy" : "Sell"
                            }`}
                      </span>
                      {!state.isSubmitting ? <ShortcutHint keys="Enter" /> : null}
                    </span>
                  </button>
                </div>
              </section>

              <section
                className={`${panelBaseClass} grid h-full min-h-0 grid-rows-[48px_1fr] overflow-hidden`}
                data-testid="messages-panel"
              >
                <div className="border-b border-[#2c2d31] px-[20px] pt-[10px]">
                  <h2 className="text-[21px] font-bold leading-none text-white">
                    Messages
                  </h2>
                </div>

                <div className="min-h-0 space-y-[10px] overflow-y-auto px-[20px] py-[14px]">
                  {visibleMessages.length > 0 ? (
                    visibleMessages.map((message) => (
                      <div
                        className={`rounded-[5px] border px-[12px] py-[10px] ${messageCardToneClass(message.tone)}`}
                        key={message.id}
                      >
                        <div className="text-[11px] font-medium leading-none text-[#7d7d84]">
                          <span>{message.time}</span>
                        </div>
                        <p className={`mt-[8px] text-[14px] font-medium leading-[1.15] ${messageToneClass(message.tone)}`}>
                          {message.text}
                        </p>
                      </div>
                    ))
                  ) : (
                    <div className="flex h-full items-center justify-center text-center text-[16px] leading-[1.2] text-[#8a8a92]">
                      Waiting for exchange events.
                    </div>
                  )}
                </div>
              </section>
            </div>
          </div>
      </div>
    </div>
  );
}

export function TradeConsole({ runtime }: { runtime?: TradeRuntimeConfig }) {
  const controller = useTradeController(runtime ? { runtime } : undefined);
  return <TradeConsoleView controller={controller} />;
}
