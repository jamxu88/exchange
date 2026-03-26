"use client";

import Image from "next/image";
import { useEffect, useRef, useState } from "react";
import {
  DEFAULT_TRADE_KEYBINDS,
  DEFAULT_TRADE_PREFERENCES,
  isTradeKeybindMatch,
  keybindsHaveConflicts,
  loadTradePreferences,
  normalizeTradeKeybindKey,
  saveTradePreferences,
  type ExecutionSoundPreference,
  type TradeKeybindAction,
  type TradeKeybinds,
} from "@/components/trade/trade-preferences";
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
import type {
  AggregatedBookLevel,
  MarketStatus,
  MessageTone,
  PnlMetric,
} from "@/components/trade/trade-types";

const contentColumns = "minmax(0, 373fr) minmax(0, 722fr) minmax(0, 298fr)";
const leftColumnRows = "minmax(0, 500fr) minmax(0, 360fr)";
const rightColumnRows = "minmax(0, 430fr) minmax(0, 430fr)";
const panelBaseClass = "rounded-[10px] border border-[#26272b] bg-[#141416]";
const quickAdjustments = [-100, -10, 10, 100];
const ticketInputEditingKeys = new Set([
  "Backspace",
  "Delete",
  "ArrowLeft",
  "ArrowRight",
  "ArrowUp",
  "ArrowDown",
  "Home",
  "End",
  "Tab",
]);
const keybindFieldDefinitions: Array<{
  action: TradeKeybindAction;
  label: string;
  helper: string;
}> = [
  { action: "buy", label: "Buy", helper: "Select the buy side" },
  { action: "sell", label: "Sell", helper: "Select the sell side" },
  { action: "limit", label: "Limit", helper: "Switch the ticket to limit orders" },
  { action: "market", label: "Market", helper: "Switch the ticket to market orders" },
  { action: "marketPrev", label: "Prev Market", helper: "Select the market to the left" },
  { action: "marketNext", label: "Next Market", helper: "Select the market to the right" },
  { action: "price", label: "Price", helper: "Focus the price field" },
  { action: "shares", label: "Shares", helper: "Focus the share count field" },
  { action: "submit", label: "Submit", helper: "Send the current order" },
];

function ShortcutHint({
  keys,
  tone = "default",
}: {
  keys: string;
  tone?: "default" | "button";
}) {
  return (
    <span
      className={
        tone === "button"
          ? "text-[10px] font-semibold uppercase tracking-[0.08em] text-[rgba(255,255,255,0.92)] drop-shadow-[0_1px_1px_rgba(0,0,0,0.24)]"
          : "text-[10px] font-medium uppercase tracking-[0.08em] text-[#b6b6bc]"
      }
    >
      ({keys})
    </span>
  );
}

function readFileAsDataUrl(file: File) {
  return new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(new Error("Unable to read the selected audio file."));
    reader.onload = () => {
      if (typeof reader.result === "string") {
        resolve(reader.result);
        return;
      }

      reject(new Error("Unable to read the selected audio file."));
    };
    reader.readAsDataURL(file);
  });
}

function playLocalExecutionSound(dataUrl: string) {
  if (typeof Audio === "undefined") {
    return;
  }

  const audio = new Audio(dataUrl);
  audio.volume = 1;
  void audio.play().catch(() => {});
}

type TradeSettingsPanelProps = {
  draftKeybinds: TradeKeybinds;
  draftExecutionSound: ExecutionSoundPreference | null;
  errorMessage: string | null;
  isUploadingSound: boolean;
  onClose: () => void;
  onClearSound: () => void;
  onKeybindChange: (action: TradeKeybindAction, binding: string) => void;
  onPreviewSound: () => void;
  onResetDefaults: () => void;
  onSave: () => void;
  onSoundSelected: (file: File | null) => void;
};

function TradeSettingsPanel({
  draftKeybinds,
  draftExecutionSound,
  errorMessage,
  isUploadingSound,
  onClose,
  onClearSound,
  onKeybindChange,
  onPreviewSound,
  onResetDefaults,
  onSave,
  onSoundSelected,
}: TradeSettingsPanelProps) {
  return (
    <div
      className="fixed inset-0 z-40 bg-[rgba(0,0,0,0.52)] backdrop-blur-[2px] motion-backdrop-in"
      onClick={onClose}
    >
      <div
        className="absolute right-[clamp(18px,2.6vw,40px)] top-[94px] flex max-h-[calc(100vh-112px)] w-[min(460px,calc(100vw-36px))] flex-col overflow-hidden rounded-[10px] border border-[#2b2d31] bg-[#141416] shadow-[0_24px_64px_rgba(0,0,0,0.45)] motion-scale-in"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="flex items-start justify-between border-b border-[#2c2d31] px-[18px] py-[14px]">
          <div>
            <p className="text-[19px] font-bold leading-none text-white">Trade settings</p>
            <p className="mt-[8px] text-[13px] leading-[1.2] text-[#9d9da4]">
              These preferences are stored only in this browser.
            </p>
          </div>
          <button
            aria-label="Close settings"
            className="rounded-[6px] border border-[#32333a] px-[10px] py-[7px] text-[11px] font-semibold uppercase tracking-[0.08em] text-[#d9d9dc] hover:border-[#50515a] hover:text-white motion-hover-soft"
            onClick={onClose}
            type="button"
          >
            Close
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-[18px] py-[16px]">
          <div className="grid gap-[18px]">
          <section className="grid gap-[10px]">
            <div className="flex items-center justify-between">
              <p className="text-[14px] font-semibold uppercase tracking-[0.14em] text-[#8f9098]">
                Keybinds
              </p>
              <button
                className="rounded-[6px] border border-[#32333a] px-[10px] py-[7px] text-[11px] font-semibold uppercase tracking-[0.08em] text-[#d9d9dc] hover:border-[#50515a] hover:text-white motion-hover-soft"
                onClick={onResetDefaults}
                type="button"
              >
                Reset defaults
              </button>
            </div>
            <p className="text-[13px] leading-[1.2] text-[#7f8289]">
              Focus a field, then press the key you want to assign.
            </p>
            <div className="grid gap-[10px] sm:grid-cols-2">
              {keybindFieldDefinitions.map((field) => (
                <label className="grid gap-[6px]" key={field.action}>
                  <span className="text-[13px] font-semibold leading-none text-white">
                    {field.label}
                  </span>
                  <input
                    aria-label={`${field.label} keybind`}
                    className="rounded-[6px] border border-[#2b2d32] bg-[#0d0f12] px-[12px] py-[10px] text-[14px] font-semibold text-white outline-none focus:border-[rgba(64,217,255,0.5)] focus:shadow-[0_0_0_1px_rgba(64,217,255,0.14)]"
                    onKeyDown={(event) => {
                      event.preventDefault();
                      if (event.metaKey || event.ctrlKey || event.altKey) {
                        return;
                      }

                      if (event.key === "Backspace" || event.key === "Delete") {
                        onKeybindChange(field.action, "");
                        return;
                      }

                      const binding = normalizeTradeKeybindKey(event.key);
                      if (binding) {
                        onKeybindChange(field.action, binding);
                      }
                    }}
                    readOnly
                    value={draftKeybinds[field.action]}
                  />
                  <span className="text-[12px] leading-[1.2] text-[#7f8289]">
                    {field.helper}
                  </span>
                </label>
              ))}
            </div>
          </section>

          <section className="grid gap-[10px]">
            <p className="text-[14px] font-semibold uppercase tracking-[0.14em] text-[#8f9098]">
              Execution sound
            </p>
            <p className="text-[13px] leading-[1.2] text-[#7f8289]">
              Pick a local audio file. It stays in this browser and plays when a new fill arrives.
            </p>
            <div className="rounded-[8px] border border-[#2b2d32] bg-[#0f1013] px-[12px] py-[12px]">
              <p className="text-[13px] leading-none text-white">
                {draftExecutionSound ? draftExecutionSound.name : "No sound selected"}
              </p>
              <div className="mt-[10px] flex flex-wrap gap-[8px]">
                <label className="rounded-[6px] border border-[#32333a] px-[10px] py-[7px] text-[11px] font-semibold uppercase tracking-[0.08em] text-[#d9d9dc] hover:border-[#50515a] hover:text-white motion-hover-soft">
                  <span>{isUploadingSound ? "Loading..." : "Choose file"}</span>
                  <input
                    accept="audio/*"
                    aria-label="Execution sound file"
                    className="hidden"
                    disabled={isUploadingSound}
                    onChange={(event) => onSoundSelected(event.target.files?.[0] ?? null)}
                    type="file"
                  />
                </label>
                <button
                  className="rounded-[6px] border border-[#32333a] px-[10px] py-[7px] text-[11px] font-semibold uppercase tracking-[0.08em] text-[#d9d9dc] hover:border-[#50515a] hover:text-white disabled:cursor-not-allowed disabled:border-[#26272b] disabled:text-[#6f6f76] motion-hover-soft"
                  disabled={!draftExecutionSound}
                  onClick={onPreviewSound}
                  type="button"
                >
                  Test sound
                </button>
                <button
                  className="rounded-[6px] border border-[#32333a] px-[10px] py-[7px] text-[11px] font-semibold uppercase tracking-[0.08em] text-[#d9d9dc] hover:border-[#50515a] hover:text-white disabled:cursor-not-allowed disabled:border-[#26272b] disabled:text-[#6f6f76] motion-hover-soft"
                  disabled={!draftExecutionSound}
                  onClick={onClearSound}
                  type="button"
                >
                  Clear
                </button>
              </div>
            </div>
          </section>

          {errorMessage ? (
            <p className="rounded-[8px] border border-[rgba(216,91,91,0.42)] bg-[rgba(216,91,91,0.1)] px-[12px] py-[10px] text-[13px] leading-[1.2] text-[#ffb2b2]">
              {errorMessage}
            </p>
          ) : null}
          </div>
        </div>

        <div className="flex items-center justify-end gap-[8px] border-t border-[#2c2d31] px-[18px] py-[14px]">
          <button
            className="rounded-[6px] border border-[#32333a] px-[12px] py-[9px] text-[12px] font-semibold uppercase tracking-[0.08em] text-[#d9d9dc] hover:border-[#50515a] hover:text-white motion-hover-soft"
            onClick={onClose}
            type="button"
          >
            Cancel
          </button>
          <button
            className="rounded-[6px] bg-[#42cc4e] px-[12px] py-[9px] text-[12px] font-bold uppercase tracking-[0.08em] text-[#081108] motion-hover-soft"
            onClick={onSave}
            type="button"
          >
            Save settings
          </button>
        </div>
      </div>
    </div>
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

function resolveMarketStatus(status?: MarketStatus): MarketStatus {
  return status ?? "enabled";
}

function marketStatusLabel(status?: MarketStatus) {
  const resolvedStatus = resolveMarketStatus(status);
  if (resolvedStatus === "disabled") {
    return "Disabled";
  }

  if (resolvedStatus === "settled") {
    return "Settled";
  }

  return "Enabled";
}

function marketStatusBadgeClass(status?: MarketStatus) {
  const resolvedStatus = resolveMarketStatus(status);
  if (resolvedStatus === "disabled") {
    return "border border-[rgba(242,170,102,0.32)] bg-[rgba(107,71,39,0.22)] text-[#f3c89d]";
  }

  if (resolvedStatus === "settled") {
    return "border border-[rgba(145,151,172,0.26)] bg-[rgba(71,75,90,0.24)] text-[#d7dbe6]";
  }

  return "border border-[rgba(66,204,78,0.28)] bg-[rgba(66,204,78,0.12)] text-[#c8f5cc]";
}

function marketTabClass(status: MarketStatus | undefined, isSelected: boolean) {
  const resolvedStatus = resolveMarketStatus(status);

  if (resolvedStatus === "enabled") {
    return isSelected
      ? "inline-flex items-center gap-[7px] rounded-[6px] bg-white px-[16px] py-[10px] text-[15px] font-bold leading-none whitespace-nowrap text-black motion-hover-soft"
      : "inline-flex items-center gap-[7px] rounded-[6px] px-[16px] py-[10px] text-[15px] font-semibold leading-none whitespace-nowrap text-[var(--muted-strong)] hover:bg-[rgba(255,255,255,0.04)] hover:text-white motion-hover-soft";
  }

  if (resolvedStatus === "disabled") {
    return isSelected
      ? "inline-flex items-center gap-[7px] rounded-[6px] border border-[rgba(242,170,102,0.32)] bg-[rgba(107,71,39,0.18)] px-[16px] py-[10px] text-[15px] font-bold leading-none whitespace-nowrap text-[#f3c89d] motion-hover-soft"
      : "inline-flex items-center gap-[7px] rounded-[6px] bg-[rgba(107,71,39,0.12)] px-[16px] py-[10px] text-[15px] font-semibold leading-none whitespace-nowrap text-[#d5ac83] hover:bg-[rgba(107,71,39,0.18)] hover:text-[#f3c89d] motion-hover-soft";
  }

  return isSelected
    ? "inline-flex items-center gap-[7px] rounded-[6px] border border-[rgba(145,151,172,0.26)] bg-[rgba(71,75,90,0.24)] px-[16px] py-[10px] text-[15px] font-bold leading-none whitespace-nowrap text-[#d7dbe6] motion-hover-soft"
    : "inline-flex items-center gap-[7px] rounded-[6px] bg-[rgba(71,75,90,0.15)] px-[16px] py-[10px] text-[15px] font-semibold leading-none whitespace-nowrap text-[#adb2c0] hover:bg-[rgba(71,75,90,0.2)] hover:text-[#d7dbe6] motion-hover-soft";
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

function isTicketInputTarget(
  target: EventTarget | null,
  priceInput: HTMLInputElement | null,
  sharesInput: HTMLInputElement | null,
) {
  return target === priceInput || target === sharesInput;
}

function shouldReserveTicketInputKeyForEditing(key: string) {
  if (/^[0-9]$/.test(key)) {
    return true;
  }

  return ticketInputEditingKeys.has(key);
}

type TradeConsoleViewProps = {
  controller: ReturnType<typeof useTradeController>;
};

export function TradeConsoleView({ controller }: TradeConsoleViewProps) {
  const [isOrderTypeMenuOpen, setIsOrderTypeMenuOpen] = useState(false);
  const [isProfileMenuOpen, setIsProfileMenuOpen] = useState(false);
  const [isSettingsPanelOpen, setIsSettingsPanelOpen] = useState(false);
  const [cancelingOrderIds, setCancelingOrderIds] = useState<string[]>([]);
  const [tradePreferences, setTradePreferences] = useState(DEFAULT_TRADE_PREFERENCES);
  const [draftKeybinds, setDraftKeybinds] = useState(DEFAULT_TRADE_KEYBINDS);
  const [draftExecutionSound, setDraftExecutionSound] =
    useState<ExecutionSoundPreference | null>(null);
  const [settingsErrorMessage, setSettingsErrorMessage] = useState<string | null>(null);
  const [isUploadingSound, setIsUploadingSound] = useState(false);
  const priceInputRef = useRef<HTMLInputElement | null>(null);
  const sharesInputRef = useRef<HTMLInputElement | null>(null);
  const hasInitializedExecutionSoundRef = useRef(false);
  const lastExecutionFillIdRef = useRef<string | null>(null);
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
  const latestFillId = state.fills[state.fills.length - 1]?.fillId ?? null;
  const selectedMarketStatus = resolveMarketStatus(selectedMarket?.status);
  const selectedMarketCanTrade = selectedMarketStatus === "enabled";

  useEffect(() => {
    const loadedPreferences = loadTradePreferences();
    setTradePreferences(loadedPreferences);
    setDraftKeybinds(loadedPreferences.keybinds);
    setDraftExecutionSound(loadedPreferences.executionSound);
  }, []);

  useEffect(() => {
    if (!hasInitializedExecutionSoundRef.current) {
      hasInitializedExecutionSoundRef.current = true;
      lastExecutionFillIdRef.current = latestFillId;
      return;
    }

    if (!latestFillId || lastExecutionFillIdRef.current === latestFillId) {
      return;
    }

    lastExecutionFillIdRef.current = latestFillId;
    if (tradePreferences.executionSound) {
      playLocalExecutionSound(tradePreferences.executionSound.dataUrl);
    }
  }, [latestFillId, tradePreferences.executionSound]);

  useEffect(() => {
    function selectRelativeMarket(step: -1 | 1) {
      if (state.availableMarkets.length <= 1) {
        return;
      }

      const currentIndex = state.availableMarkets.findIndex(
        (market) => market.id === state.selectedMarketId,
      );
      const startIndex = currentIndex >= 0 ? currentIndex : 0;
      const nextIndex =
        (startIndex + step + state.availableMarkets.length) % state.availableMarkets.length;
      const nextMarket = state.availableMarkets[nextIndex];
      if (!nextMarket) {
        return;
      }

      actions.selectMarket(nextMarket.id);
    }

    function handleKeyDown(event: KeyboardEvent) {
      if (isSettingsPanelOpen) {
        return;
      }

      if (event.metaKey || event.ctrlKey || event.altKey) {
        return;
      }

      const targetIsEditable = isEditableTarget(event.target);
      const targetIsTicketInput = isTicketInputTarget(
        event.target,
        priceInputRef.current,
        sharesInputRef.current,
      );
      const allowTradeKeybindWhileEditing =
        targetIsTicketInput && !shouldReserveTicketInputKeyForEditing(event.key);

      if (
        isTradeKeybindMatch(event.key, tradePreferences.keybinds.submit) &&
        (!targetIsEditable || allowTradeKeybindWhileEditing || event.key === "Enter")
      ) {
        event.preventDefault();
        void actions.submitOrder();
        return;
      }

      if (targetIsEditable && !allowTradeKeybindWhileEditing) {
        return;
      }

      if (isTradeKeybindMatch(event.key, tradePreferences.keybinds.buy)) {
        event.preventDefault();
        actions.setSide("buy");
        return;
      }

      if (isTradeKeybindMatch(event.key, tradePreferences.keybinds.sell)) {
        event.preventDefault();
        actions.setSide("sell");
        return;
      }

      if (isTradeKeybindMatch(event.key, tradePreferences.keybinds.limit)) {
        event.preventDefault();
        actions.setOrderType("limit");
        setIsOrderTypeMenuOpen(false);
        return;
      }

      if (isTradeKeybindMatch(event.key, tradePreferences.keybinds.market)) {
        event.preventDefault();
        actions.setOrderType("market");
        setIsOrderTypeMenuOpen(false);
        return;
      }

      if (isTradeKeybindMatch(event.key, tradePreferences.keybinds.marketPrev)) {
        event.preventDefault();
        selectRelativeMarket(-1);
        return;
      }

      if (isTradeKeybindMatch(event.key, tradePreferences.keybinds.marketNext)) {
        event.preventDefault();
        selectRelativeMarket(1);
        return;
      }

      if (isTradeKeybindMatch(event.key, tradePreferences.keybinds.price)) {
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
        return;
      }

      if (isTradeKeybindMatch(event.key, tradePreferences.keybinds.shares)) {
        event.preventDefault();
        sharesInputRef.current?.focus();
        sharesInputRef.current?.select();
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [
    actions,
    isSettingsPanelOpen,
    state.availableMarkets,
    state.orderType,
    state.selectedMarketId,
    tradePreferences.keybinds,
  ]);

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

  function openSettingsPanel() {
    setDraftKeybinds(tradePreferences.keybinds);
    setDraftExecutionSound(tradePreferences.executionSound);
    setSettingsErrorMessage(null);
    setIsProfileMenuOpen(false);
    setIsSettingsPanelOpen(true);
  }

  function closeSettingsPanel() {
    setIsSettingsPanelOpen(false);
    setSettingsErrorMessage(null);
  }

  function handleDraftKeybindChange(action: TradeKeybindAction, binding: string) {
    setDraftKeybinds((current) => ({
      ...current,
      [action]: binding,
    }));
  }

  function handleResetKeybindDefaults() {
    setDraftKeybinds(DEFAULT_TRADE_KEYBINDS);
    setSettingsErrorMessage(null);
  }

  async function handleSoundFileSelected(file: File | null) {
    if (!file) {
      return;
    }

    setIsUploadingSound(true);
    setSettingsErrorMessage(null);
    try {
      const dataUrl = await readFileAsDataUrl(file);
      setDraftExecutionSound({
        name: file.name,
        dataUrl,
      });
    } catch (error) {
      setSettingsErrorMessage(
        error instanceof Error
          ? error.message
          : "Unable to read the selected audio file.",
      );
    } finally {
      setIsUploadingSound(false);
    }
  }

  function handleSaveSettings() {
    if (Object.values(draftKeybinds).some((binding) => binding.length === 0)) {
      setSettingsErrorMessage("Every trade action needs a keybind.");
      return;
    }

    if (keybindsHaveConflicts(draftKeybinds)) {
      setSettingsErrorMessage("Each trade action needs a unique keybind.");
      return;
    }

    const nextPreferences = {
      keybinds: draftKeybinds,
      executionSound: draftExecutionSound,
    };

    try {
      saveTradePreferences(nextPreferences);
      setTradePreferences(nextPreferences);
      setSettingsErrorMessage(null);
      setIsSettingsPanelOpen(false);
    } catch {
      setSettingsErrorMessage(
        "Unable to save settings locally. Try a smaller sound file.",
      );
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
          className="relative z-[60] h-[88px] bg-black px-[clamp(18px,2.6vw,40px)] pt-[10px]"
          data-testid="trade-console-header"
        >
          <div className="surface-panel-soft motion-fade-up rounded-[10px] grid h-[68px] grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-[14px] px-[14px]">
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
              <div className="flex max-w-full items-center justify-center gap-[10px] overflow-x-auto">
                {state.availableMarkets.length > 1 ? (
                  <span className="shrink-0 text-[11px] font-medium leading-none text-[#8f929b]">
                    Prev <ShortcutHint keys={tradePreferences.keybinds.marketPrev} />
                  </span>
                ) : null}
                <div className="flex items-center gap-[8px] rounded-[10px] border border-[var(--surface-stroke)] bg-[var(--surface-soft)] p-[4px]">
                  {state.availableMarkets.map((market) => {
                    const isSelected = state.selectedMarketId === market.id;
                    const marketStatus = resolveMarketStatus(market.status);

                    return (
                      <button
                        className={marketTabClass(market.status, isSelected)}
                        key={market.id}
                        onClick={() => actions.selectMarket(market.id)}
                        type="button"
                      >
                        <span>{market.name}</span>
                        {marketStatus !== "enabled" ? (
                          <span
                            className={`rounded-[999px] px-[6px] py-[2px] text-[10px] font-semibold uppercase tracking-[0.08em] ${marketStatusBadgeClass(marketStatus)}`}
                          >
                            {marketStatusLabel(marketStatus)}
                          </span>
                        ) : null}
                      </button>
                    );
                  })}
                </div>
                {state.availableMarkets.length > 1 ? (
                  <span className="shrink-0 text-[11px] font-medium leading-none text-[#8f929b]">
                    Next <ShortcutHint keys={tradePreferences.keybinds.marketNext} />
                  </span>
                ) : null}
              </div>
            </nav>

            <div className="flex items-center justify-end gap-[10px]">
              <a
                className="inline-flex items-center rounded-[8px] border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-[14px] py-[10px] text-[13px] font-semibold leading-none whitespace-nowrap text-[var(--muted-strong)] hover:border-[rgba(66,204,78,0.42)] hover:text-white motion-hover-soft"
                href="https://jamesxu.mintlify.app/"
                rel="noreferrer"
                target="_blank"
              >
                API Docs
              </a>
              <div className="inline-flex items-center gap-[8px] rounded-[8px] border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-[12px] py-[10px] text-[13px] font-medium leading-none whitespace-nowrap text-white">
                <span
                  className={`motion-status-pulse h-[8px] w-[8px] rounded-full shadow-[0_0_14px_rgba(255,255,255,0.18)] ${connection.dotClass}`}
                />
                <span>{connection.label}</span>
              </div>
              <div className="relative">
                <button
                  aria-expanded={isProfileMenuOpen}
                  aria-label="Open profile menu"
                  aria-haspopup="menu"
                  className="flex h-[44px] items-center gap-[8px] rounded-[8px] border border-[var(--surface-stroke)] bg-[var(--surface-soft)] py-[6px] pl-[6px] pr-[10px] hover:border-[rgba(66,204,78,0.42)] motion-hover-soft"
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
                  <div className="surface-panel motion-scale-in absolute right-0 top-[calc(100%+10px)] z-[80] w-[240px] rounded-[10px] p-[8px]">
                    <div className="surface-panel-soft rounded-[8px] px-[12px] py-[10px]">
                      <p className="text-[15px] font-semibold leading-none text-white">{profileName}</p>
                      <p className="mt-[8px] text-[13px] font-medium leading-none text-[var(--muted)]">
                        {profileTeam}
                      </p>
                    </div>
                    <button
                      className="mt-[8px] w-full rounded-[8px] border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-[12px] py-[10px] text-[13px] font-semibold leading-none text-[var(--muted-strong)] hover:border-[rgba(66,204,78,0.42)] hover:text-white motion-hover-soft"
                      onClick={openSettingsPanel}
                      type="button"
                    >
                      Settings
                    </button>
                    <form action="/api/auth/logout" className="mt-[8px]" method="post">
                      <button
                        className="w-full rounded-[8px] border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-[12px] py-[10px] text-[13px] font-semibold leading-none text-[var(--muted-strong)] hover:border-[rgba(66,204,78,0.42)] hover:text-white motion-hover-soft"
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
                className={`${panelBaseClass} motion-fade-up motion-delay-1 grid h-full min-h-0 grid-rows-[48px_1fr] overflow-hidden`}
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

              <section className={`${panelBaseClass} motion-fade-up motion-delay-2 grid min-h-0 grid-rows-[46px_1fr] overflow-hidden`}>
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
              className={`${panelBaseClass} motion-fade-up motion-delay-2 grid h-full min-h-0 grid-rows-[52px_1fr_63px] overflow-hidden`}
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
                className="motion-fade-up motion-delay-3 grid h-full min-h-0 grid-rows-[44px_1fr] overflow-hidden rounded-[6px] border-[0.595px] border-[#26272b] bg-[rgba(24,24,27,0.82)]"
                data-testid="ticket-panel"
              >
                <div className="flex items-center justify-between border-b border-[#2c2d31] px-[13px] py-[10px]">
                  <div className="flex min-w-0 items-center gap-[8px]">
                    <p className="max-w-[180px] truncate text-[15.477px] font-bold leading-none text-white">
                      {selectedMarket?.name ?? "--"}
                    </p>
                    {selectedMarketStatus !== "enabled" ? (
                      <span
                        className={`rounded-[999px] px-[6px] py-[2px] text-[10px] font-semibold uppercase tracking-[0.08em] ${marketStatusBadgeClass(selectedMarketStatus)}`}
                      >
                        {marketStatusLabel(selectedMarketStatus)}
                      </span>
                    ) : null}
                  </div>

                  <div className="relative">
                    <button
                      className="flex items-center gap-[5px] text-[15.477px] font-medium leading-none text-white motion-hover-soft"
                      onClick={() => setIsOrderTypeMenuOpen((current) => !current)}
                      type="button"
                    >
                      <span>{state.orderType === "limit" ? "Limit" : "Market"}</span>
                      <ShortcutHint
                        keys={
                          state.orderType === "limit"
                            ? tradePreferences.keybinds.limit
                            : tradePreferences.keybinds.market
                        }
                      />
                      <Image alt="" height={14} src="/chevron.svg" width={14} />
                    </button>

                    {isOrderTypeMenuOpen ? (
                      <div className="motion-scale-in absolute right-0 top-[calc(100%+8px)] z-10 w-[126px] rounded-[7px] border border-[#2c2d31] bg-[#18181b] p-[6px] shadow-[0_12px_32px_rgba(0,0,0,0.35)]">
                        {(["limit", "market"] as const).map((orderType) => (
                          <button
                            className={
                              state.orderType === orderType
                                ? "flex w-full items-center justify-between rounded-[5px] bg-[#26272b] px-[10px] py-[8px] text-left text-[15px] font-semibold text-white motion-hover-soft"
                                : "flex w-full items-center justify-between rounded-[5px] px-[10px] py-[8px] text-left text-[15px] font-medium text-[#b8b8bc] motion-hover-soft"
                            }
                            key={orderType}
                            onClick={() => {
                              actions.setOrderType(orderType);
                              setIsOrderTypeMenuOpen(false);
                            }}
                            type="button"
                          >
                            <span>{orderType === "limit" ? "Limit" : "Market"}</span>
                            <ShortcutHint
                              keys={
                                orderType === "limit"
                                  ? tradePreferences.keybinds.limit
                                  : tradePreferences.keybinds.market
                              }
                            />
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
                          ? "h-[42px] rounded-[3px] bg-[#42cc4e] text-[16px] font-bold leading-none text-white motion-hover-soft"
                          : "h-[42px] rounded-[3px] bg-[#26272b] text-[16px] font-bold leading-none text-white motion-hover-soft"
                      }
                      onClick={() => actions.setSide("buy")}
                      type="button"
                    >
                      <span className="inline-flex items-center gap-[4px] text-[#e2e2e2]">
                        <span>Buy</span>
                        <ShortcutHint keys={tradePreferences.keybinds.buy} tone="button" />
                      </span>{" "}
                      {formatMaybePrice(summary.buyQuote)}
                    </button>
                    <button
                      className={
                        state.ticketSide === "sell"
                          ? "h-[42px] rounded-[3px] bg-[#d85b5b] text-[16px] font-bold leading-none text-white motion-hover-soft"
                          : "h-[42px] rounded-[3px] bg-[#26272b] text-[16px] font-bold leading-none text-white motion-hover-soft"
                      }
                      onClick={() => actions.setSide("sell")}
                      type="button"
                    >
                      <span className="inline-flex items-center gap-[4px] text-[#e2e2e2]">
                        <span>Sell</span>
                        <ShortcutHint keys={tradePreferences.keybinds.sell} tone="button" />
                      </span>{" "}
                      {formatMaybePrice(summary.sellQuote)}
                    </button>
                  </div>

                  <div className="mt-[20px] grid grid-cols-[1fr_148px] items-center">
                    <span className="inline-flex items-center gap-[6px] text-[16px] font-medium leading-none text-white">
                      <span>{state.orderType === "market" ? "Market Price" : "Limit Price"}</span>
                      <ShortcutHint keys={tradePreferences.keybinds.price} />
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
                      <ShortcutHint keys={tradePreferences.keybinds.shares} />
                    </span>
                    <div className="flex h-[34px] items-center justify-between rounded-[6px] border border-[#666] bg-[#18181b] px-[5px]">
                      <button
                        className="flex h-[24px] w-[24px] items-center justify-center motion-hover-soft"
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
                        className="flex h-[24px] w-[24px] items-center justify-center motion-hover-soft"
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
                        className="flex h-[22px] min-w-[34px] items-center justify-center rounded-[4px] border border-[#d5d5d5] px-[6px] text-[11px] font-semibold leading-none text-[#d5d5d5] motion-hover-soft"
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

                  {!selectedMarketCanTrade ? (
                    <p className="mt-[12px] text-[12px] font-medium leading-[1.2] text-[#989ba6]">
                      {selectedMarketStatus === "settled"
                        ? "This market is settled. New orders are unavailable."
                        : "This market is disabled. New orders are unavailable."}
                    </p>
                  ) : null}

                  <button
                    className={
                      state.ticketSide === "buy"
                        ? "mt-auto h-[44px] w-full rounded-[6px] bg-[#42cc4e] text-[16px] font-bold leading-none text-white disabled:cursor-not-allowed disabled:opacity-60 motion-hover-soft"
                        : "mt-auto h-[44px] w-full rounded-[6px] bg-[#d85b5b] text-[16px] font-bold leading-none text-white disabled:cursor-not-allowed disabled:opacity-60 motion-hover-soft"
                    }
                    disabled={state.isSubmitting || !selectedMarketCanTrade}
                    onClick={() => {
                      void actions.submitOrder();
                    }}
                    type="button"
                  >
                    <span className="inline-flex items-center gap-[6px]">
                      <span>
                        {!selectedMarketCanTrade
                          ? `Market ${marketStatusLabel(selectedMarketStatus)}`
                          : state.isSubmitting
                          ? "Submitting..."
                          : `${state.orderType === "market" ? "Market" : "Limit"} ${
                              state.ticketSide === "buy" ? "Buy" : "Sell"
                            }`}
                      </span>
                      {!state.isSubmitting && selectedMarketCanTrade ? (
                        <ShortcutHint keys={tradePreferences.keybinds.submit} tone="button" />
                      ) : null}
                    </span>
                  </button>
                </div>
              </section>

              <section
                className={`${panelBaseClass} motion-fade-up motion-delay-4 grid h-full min-h-0 grid-rows-[48px_1fr] overflow-hidden`}
                data-testid="messages-panel"
              >
                <div className="border-b border-[#2c2d31] px-[20px] pt-[10px]">
                  <h2 className="text-[21px] font-bold leading-none text-white">
                    Messages
                  </h2>
                </div>

                <div className="min-h-0 space-y-[10px] overflow-y-auto px-[20px] py-[14px]">
                  {visibleMessages.length > 0 ? (
                    visibleMessages.map((message, index) => (
                      <div
                        className={`motion-fade-up motion-fade-up-fast rounded-[5px] border px-[12px] py-[10px] ${messageCardToneClass(message.tone)}`}
                        key={message.id}
                        style={{ animationDelay: `${Math.min(index, 4) * 35}ms` }}
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
      {isSettingsPanelOpen ? (
        <TradeSettingsPanel
          draftExecutionSound={draftExecutionSound}
          draftKeybinds={draftKeybinds}
          errorMessage={settingsErrorMessage}
          isUploadingSound={isUploadingSound}
          onClearSound={() => setDraftExecutionSound(null)}
          onClose={closeSettingsPanel}
          onKeybindChange={handleDraftKeybindChange}
          onPreviewSound={() => {
            if (draftExecutionSound) {
              playLocalExecutionSound(draftExecutionSound.dataUrl);
            }
          }}
          onResetDefaults={handleResetKeybindDefaults}
          onSave={handleSaveSettings}
          onSoundSelected={handleSoundFileSelected}
        />
      ) : null}
    </div>
  );
}

export function TradeConsole({ runtime }: { runtime?: TradeRuntimeConfig }) {
  const controller = useTradeController(runtime ? { runtime } : undefined);
  return <TradeConsoleView controller={controller} />;
}
