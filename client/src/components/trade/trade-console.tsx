"use client";

import Image from "next/image";
import { useState } from "react";
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

const desktopFrameColumns =
  "40px 179px 1fr 797px 1fr 86px 27px 30px 46px";
const contentColumns = "349px 746px 298px";
const leftColumnRows = "360px minmax(0,1fr)";
const rightColumnRows = "378px minmax(0,1fr)";
const panelBaseClass = "rounded-[20px] border border-[#26272b] bg-[#141416]";
const quickAdjustments = [-100, -10, 10, 100];

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

function HeaderSeparator() {
  return <div className="h-[23px] w-px bg-[#5a5a5f]" />;
}

function OrderBookRow({
  level,
  side,
}: {
  level: AggregatedBookLevel | null;
  side: "ask" | "bid";
}) {
  const priceClass = side === "ask" ? "text-[#ff8181]" : "text-[#8eff81]";

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

type TradeConsoleViewProps = {
  controller: ReturnType<typeof useTradeController>;
};

export function TradeConsoleView({ controller }: TradeConsoleViewProps) {
  const [isPositionMenuOpen, setIsPositionMenuOpen] = useState(false);
  const [isOrderTypeMenuOpen, setIsOrderTypeMenuOpen] = useState(false);
  const [isProfileMenuOpen, setIsProfileMenuOpen] = useState(false);
  const { state, derived, actions } = controller;
  const selectedMarket = selectSelectedMarket(state);
  const activeRows = selectActiveRows(state);
  const pendingRows = selectPendingRows(state);
  const pnlMetrics = selectPnlMetrics(state);
  const visibleMessages = [...state.messages].reverse();
  const summary = derived.summary;
  const askLevels = padLevels(summary.asks, 7);
  const bidLevels = padLevels(summary.bids, 7);
  const connection = connectionPresentation(state.connectionStatus);
  const initials = initialsForUser(state.user);
  const profileName = state.user?.username ?? "Competition User";
  const profileTeam = teamLabelForUser(state.user?.traderId);

  return (
    <div className="h-screen overflow-hidden bg-black">
      <div className="h-full overflow-x-auto overflow-y-hidden">
        <div className="mx-auto grid h-full min-w-[1512px] max-w-[1512px] grid-rows-[88px_14px_minmax(0,1fr)] bg-black">
          <header
            className="relative grid h-[88px] items-start bg-black shadow-[0px_4px_4px_0px_rgba(0,0,0,0.25)]"
            style={{ gridTemplateColumns: desktopFrameColumns }}
          >
            <div />
            <div className="mt-[28px]">
              <Image alt="Quant" height={40} src="/quant.png" width={156} />
            </div>
            <div />

            <div className="mt-[29px] flex h-[28px] items-center justify-center gap-[22px]">
              {state.availableMarkets.map((market, index) => (
                <div className="flex items-center gap-[22px]" key={market.id}>
                  <button
                    className={
                      state.selectedMarketId === market.id
                        ? "text-[21px] font-bold leading-none text-[#e9e9e9] underline decoration-solid underline-offset-[3px]"
                        : "text-[21px] font-medium leading-none text-[#e9e9e9]"
                    }
                    onClick={() => actions.selectMarket(market.id)}
                    type="button"
                  >
                    {market.name}
                  </button>
                  {index < state.availableMarkets.length - 1 ? (
                    <HeaderSeparator />
                  ) : null}
                </div>
              ))}
            </div>

            <div />
            <div className="mt-[34px] flex items-center gap-[7px]">
              <span className={`h-[6px] w-[6px] rounded-full ${connection.dotClass}`} />
              <span className="text-[16px] font-medium leading-none text-white">
                {connection.label}
              </span>
            </div>
            <div />
            <div className="relative mt-[28px]">
              <button
                aria-expanded={isProfileMenuOpen}
                aria-label="Open profile menu"
                className="flex h-[32px] w-[32px] items-center justify-center rounded-full bg-[#efebe3] text-[14px] font-medium leading-none text-black"
                onClick={() => setIsProfileMenuOpen((current) => !current)}
                type="button"
              >
                {initials}
              </button>

              {isProfileMenuOpen ? (
                <div className="absolute right-0 top-[calc(100%+10px)] z-20 w-[220px] rounded-[14px] border border-[#2c2d31] bg-[#18181b] p-[8px] shadow-[0_16px_36px_rgba(0,0,0,0.42)]">
                  <div className="rounded-[10px] bg-[#141416] px-[12px] py-[10px]">
                    <p className="text-[15px] font-semibold leading-none text-white">{profileName}</p>
                    <p className="mt-[8px] text-[13px] font-medium leading-none text-[#9f9fa6]">
                      {profileTeam}
                    </p>
                  </div>
                  <form action="/api/auth/logout" className="mt-[8px]" method="post">
                    <button
                      className="w-full rounded-[10px] border border-[#2c2d31] bg-[#141416] px-[12px] py-[10px] text-[13px] font-semibold leading-none text-[#d9d9dc] hover:border-[#3a3b41] hover:text-white"
                      type="submit"
                    >
                      Log out
                    </button>
                  </form>
                </div>
              ) : null}
            </div>
            <div />
          </header>

          <div />

          <div
            className="grid min-h-0 px-[40px] pb-[20px]"
            style={{ columnGap: "20px", gridTemplateColumns: contentColumns }}
          >
            <div
              className="grid min-h-0"
              style={{ gridTemplateRows: leftColumnRows, rowGap: "20px" }}
            >
              <section
                className={`${panelBaseClass} grid h-full min-h-0 grid-rows-[48px_40px_1fr] overflow-hidden`}
              >
                <div className="flex items-center justify-between border-b border-[#2c2d31] px-[20px] pt-[10px]">
                  <h2 className="text-[21px] font-bold leading-none text-white">
                    Positions
                  </h2>

                  <div className="relative">
                    <button
                      className="flex items-center gap-[6px] text-[15.477px] font-medium leading-none text-white"
                      onClick={() => setIsPositionMenuOpen((current) => !current)}
                      type="button"
                    >
                      {state.positionFilter === "active" ? "Active" : "Pending"}
                      <Image alt="" height={14} src="/chevron.svg" width={14} />
                    </button>

                    {isPositionMenuOpen ? (
                      <div className="absolute right-0 top-[calc(100%+8px)] z-10 w-[126px] rounded-[12px] border border-[#2c2d31] bg-[#18181b] p-[6px] shadow-[0_12px_32px_rgba(0,0,0,0.35)]">
                        {(["active", "pending"] as const).map((filter) => (
                          <button
                            className={
                              state.positionFilter === filter
                                ? "flex w-full items-center justify-between rounded-[8px] bg-[#26272b] px-[10px] py-[8px] text-left text-[15px] font-semibold text-white"
                                : "flex w-full items-center justify-between rounded-[8px] px-[10px] py-[8px] text-left text-[15px] font-medium text-[#b8b8bc]"
                            }
                            key={filter}
                            onClick={() => {
                              actions.setPositionFilter(filter);
                              setIsPositionMenuOpen(false);
                            }}
                            type="button"
                          >
                            <span>{filter === "active" ? "Active" : "Pending"}</span>
                          </button>
                        ))}
                      </div>
                    ) : null}
                  </div>
                </div>

                <div className="grid grid-cols-[1.25fr_0.7fr_0.8fr] items-center border-b border-[#2c2d31] px-[20px] text-[16px] font-bold leading-none text-white">
                  <span>Product</span>
                  <span>{state.positionFilter === "active" ? "Position" : "Shares"}</span>
                  <span className="justify-self-end">
                    {state.positionFilter === "active" ? "Avg. Cost" : "Order"}
                  </span>
                </div>

                <div className="min-h-0 overflow-y-auto px-[20px] py-[12px] text-[16px] font-medium leading-none text-white">
                  {state.positionFilter === "active" ? (
                    activeRows.length > 0 ? (
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
                            <span>{position.shares}</span>
                            <span className="justify-self-end">
                              {formatMaybePrice(position.avgCost)}
                            </span>
                          </div>
                        ))}
                      </div>
                    ) : (
                      <div className="flex h-full items-center justify-center text-center text-[16px] leading-[1.2] text-[#8a8a92]">
                        No active positions.
                      </div>
                    )
                  ) : pendingRows.length > 0 ? (
                    <div className="grid content-start gap-y-[14px]">
                      {pendingRows.map((order) => (
                        <div
                          className="grid grid-cols-[1.25fr_0.7fr_0.8fr] items-start gap-x-[10px]"
                          key={order.id}
                        >
                          <span className="max-h-[34px] min-w-0 overflow-hidden break-words leading-[17px] text-[#f5f5f5]">
                            {order.marketName}
                          </span>
                          <span>{order.shares}</span>
                          <span className="justify-self-end text-right">
                            {order.side === "buy" ? "B" : "S"} {formatPrice(order.limitPrice)}
                          </span>
                        </div>
                      ))}
                    </div>
                  ) : (
                    <div className="flex h-full items-center justify-center text-center text-[16px] leading-[1.2] text-[#8a8a92]">
                      No pending orders.
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
            >
              <div className="grid grid-cols-[1fr_1fr_1fr] items-start border-b border-[#26272b] px-[66px] pt-[14px] text-[16px] font-bold leading-none text-[#aaa]">
                <span>Price</span>
                <span className="justify-self-center">Liquidity</span>
                <span className="justify-self-end">Total</span>
              </div>

              <div className="min-h-0 overflow-hidden px-[66px] pt-[52px]">
                <div className="space-y-[12px]">
                  {askLevels.slice(0, 6).map((level, index) => (
                    <OrderBookRow
                      key={`ask-${selectedMarket?.id ?? "market"}-${index}`}
                      level={level}
                      side="ask"
                    />
                  ))}
                </div>

                <div className="mt-[12px]">
                  <OrderBookRow level={askLevels[6]} side="ask" />
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
                      side="bid"
                    />
                  ))}
                </div>
              </div>

              <div className="flex items-center justify-end border-t border-[#26272b] px-[34px]">
                <button
                  className="flex h-[37px] w-[37px] items-center justify-center rounded-[4.6px] bg-[#3f3f3f] text-white"
                  type="button"
                >
                  <Image alt="" height={22} src="/book.svg" width={22} />
                </button>
              </div>
            </section>

            <div
              className="grid min-h-0"
              style={{ gridTemplateRows: rightColumnRows, rowGap: "20px" }}
            >
              <section className="grid h-full min-h-0 grid-rows-[44px_1fr] overflow-hidden rounded-[11.906px] border-[0.595px] border-[#26272b] bg-[rgba(24,24,27,0.82)]">
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
                      {state.orderType === "limit" ? "Limit" : "Market"}
                      <Image alt="" height={14} src="/chevron.svg" width={14} />
                    </button>

                    {isOrderTypeMenuOpen ? (
                      <div className="absolute right-0 top-[calc(100%+8px)] z-10 w-[126px] rounded-[12px] border border-[#2c2d31] bg-[#18181b] p-[6px] shadow-[0_12px_32px_rgba(0,0,0,0.35)]">
                        {(["limit", "market"] as const).map((orderType) => (
                          <button
                            className={
                              state.orderType === orderType
                                ? "flex w-full items-center justify-between rounded-[8px] bg-[#26272b] px-[10px] py-[8px] text-left text-[15px] font-semibold text-white"
                                : "flex w-full items-center justify-between rounded-[8px] px-[10px] py-[8px] text-left text-[15px] font-medium text-[#b8b8bc]"
                            }
                            key={orderType}
                            onClick={() => {
                              actions.setOrderType(orderType);
                              setIsOrderTypeMenuOpen(false);
                            }}
                            type="button"
                          >
                            <span>{orderType === "limit" ? "Limit" : "Market"}</span>
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
                          ? "h-[42px] rounded-[5.953px] bg-[#42cc4e] text-[16px] font-bold leading-none text-white shadow-[0px_0px_8.929px_0.298px_#42cc4e]"
                          : "h-[42px] rounded-[5.953px] bg-[#26272b] text-[16px] font-bold leading-none text-white"
                      }
                      onClick={() => actions.setSide("buy")}
                      type="button"
                    >
                      <span className="text-[#e2e2e2]">Buy</span>{" "}
                      {formatMaybePrice(summary.buyQuote)}
                    </button>
                    <button
                      className={
                        state.ticketSide === "sell"
                          ? "h-[42px] rounded-[5.953px] bg-[#d85b5b] text-[16px] font-bold leading-none text-white shadow-[0px_0px_8.929px_0.298px_rgba(216,91,91,0.6)]"
                          : "h-[42px] rounded-[5.953px] bg-[#26272b] text-[16px] font-bold leading-none text-white"
                      }
                      onClick={() => actions.setSide("sell")}
                      type="button"
                    >
                      <span className="text-[#e2e2e2]">Sell</span>{" "}
                      {formatMaybePrice(summary.sellQuote)}
                    </button>
                  </div>

                  <div className="mt-[20px] grid grid-cols-[1fr_148px] items-center">
                    <span className="text-[16px] font-medium leading-none text-white">
                      {state.orderType === "market" ? "Market Price" : "Limit Price"}
                    </span>
                    <label className="flex h-[34px] items-center justify-center rounded-[11.906px] border border-[#666] bg-[#18181b] text-[16px] font-bold leading-none text-white">
                      <input
                        className="w-full bg-transparent px-[14px] text-center outline-none disabled:text-[#b8b8bc]"
                        disabled={state.orderType === "market"}
                        inputMode="decimal"
                        onChange={(event) => actions.setLimitPrice(event.target.value)}
                        value={
                          state.orderType === "market"
                            ? derived.estimated.derivedPrice > 0
                              ? derived.estimated.derivedPrice.toFixed(2)
                              : "--"
                            : state.limitPriceInput
                        }
                      />
                    </label>
                  </div>

                  <div className="mt-[16px] grid grid-cols-[1fr_148px] items-center">
                    <span className="text-[16px] font-medium leading-none text-white">
                      Shares
                    </span>
                    <div className="flex h-[34px] items-center justify-between rounded-[11.906px] border border-[#666] bg-[#18181b] px-[5px]">
                      <button
                        className="flex h-[24px] w-[24px] items-center justify-center"
                        onClick={() => actions.adjustShares(-1)}
                        type="button"
                      >
                        <Image alt="" height={14} src="/minus.svg" width={14} />
                      </button>
                      <input
                        className="w-[52px] bg-transparent text-center text-[16px] font-bold leading-none text-white outline-none"
                        inputMode="numeric"
                        onChange={(event) => actions.setShares(event.target.value)}
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
                        className="flex h-[22px] min-w-[34px] items-center justify-center rounded-[7px] border border-[#d5d5d5] px-[6px] text-[11px] font-semibold leading-none text-[#d5d5d5]"
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
                        ? "mt-auto h-[44px] w-full rounded-[11.906px] bg-[#42cc4e] text-[16px] font-bold leading-none text-white shadow-[0px_0px_8.929px_0.298px_#42cc4e] disabled:cursor-not-allowed disabled:opacity-60"
                        : "mt-auto h-[44px] w-full rounded-[11.906px] bg-[#d85b5b] text-[16px] font-bold leading-none text-white shadow-[0px_0px_8.929px_0.298px_rgba(216,91,91,0.6)] disabled:cursor-not-allowed disabled:opacity-60"
                    }
                    disabled={state.isSubmitting}
                    onClick={() => {
                      void actions.submitOrder();
                    }}
                    type="button"
                  >
                    {state.isSubmitting
                      ? "Submitting..."
                      : `${state.orderType === "market" ? "Market" : "Limit"} ${
                          state.ticketSide === "buy" ? "Buy" : "Sell"
                        }`}
                  </button>
                </div>
              </section>

              <section
                className={`${panelBaseClass} grid h-full min-h-0 grid-rows-[48px_1fr] overflow-hidden`}
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
                        className="rounded-[10px] border border-[#222327] bg-[#111114] px-[12px] py-[10px]"
                        key={message.id}
                      >
                        <div className="flex items-center justify-between text-[11px] font-medium leading-none text-[#7d7d84]">
                          <span>{message.time}</span>
                          <span className={messageToneClass(message.tone)}>
                            {message.tone}
                          </span>
                        </div>
                        <p className="mt-[8px] text-[14px] font-medium leading-[1.15] text-white">
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
    </div>
  );
}

export function TradeConsole({ runtime }: { runtime?: TradeRuntimeConfig }) {
  const controller = useTradeController(runtime ? { runtime } : undefined);
  return <TradeConsoleView controller={controller} />;
}
