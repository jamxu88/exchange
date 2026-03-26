"use client";

export const TRADE_PREFERENCES_STORAGE_KEY = "exchange.trade.preferences.v1";

export type TradeKeybindAction =
  | "buy"
  | "sell"
  | "limit"
  | "market"
  | "price"
  | "shares"
  | "submit";

export type TradeKeybinds = Record<TradeKeybindAction, string>;

export type ExecutionSoundPreference = {
  name: string;
  dataUrl: string;
};

export type TradePreferences = {
  keybinds: TradeKeybinds;
  executionSound: ExecutionSoundPreference | null;
};

export const DEFAULT_TRADE_KEYBINDS: TradeKeybinds = {
  buy: "B",
  sell: "S",
  limit: "L",
  market: "M",
  price: "P",
  shares: "Q",
  submit: "Enter",
};

export const DEFAULT_TRADE_PREFERENCES: TradePreferences = {
  keybinds: DEFAULT_TRADE_KEYBINDS,
  executionSound: null,
};

const SPECIAL_KEYS = new Set([
  "Enter",
  "Tab",
  "Escape",
  "Space",
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
]);

function isTradeKeybinds(value: unknown): value is TradeKeybinds {
  if (!value || typeof value !== "object") {
    return false;
  }

  return Object.keys(DEFAULT_TRADE_KEYBINDS).every((key) => {
    const keybind = (value as Record<string, unknown>)[key];
    return typeof keybind === "string" && keybind.length > 0;
  });
}

function isExecutionSoundPreference(value: unknown): value is ExecutionSoundPreference {
  if (!value || typeof value !== "object") {
    return false;
  }

  return (
    typeof (value as Record<string, unknown>).name === "string" &&
    typeof (value as Record<string, unknown>).dataUrl === "string"
  );
}

export function normalizeTradeKeybindKey(key: string) {
  if (key === " ") {
    return "Space";
  }

  if (key.length === 1) {
    const trimmed = key.trim();
    return trimmed.length > 0 ? trimmed.toUpperCase() : null;
  }

  return SPECIAL_KEYS.has(key) ? key : null;
}

export function loadTradePreferences() {
  if (typeof window === "undefined") {
    return DEFAULT_TRADE_PREFERENCES;
  }

  const raw = window.localStorage.getItem(TRADE_PREFERENCES_STORAGE_KEY);
  if (!raw) {
    return DEFAULT_TRADE_PREFERENCES;
  }

  try {
    const parsed = JSON.parse(raw) as Partial<TradePreferences>;
    return {
      keybinds: isTradeKeybinds(parsed.keybinds)
        ? parsed.keybinds
        : DEFAULT_TRADE_KEYBINDS,
      executionSound: isExecutionSoundPreference(parsed.executionSound)
        ? parsed.executionSound
        : null,
    };
  } catch {
    return DEFAULT_TRADE_PREFERENCES;
  }
}

export function saveTradePreferences(preferences: TradePreferences) {
  if (typeof window === "undefined") {
    return;
  }

  window.localStorage.setItem(
    TRADE_PREFERENCES_STORAGE_KEY,
    JSON.stringify(preferences),
  );
}

export function keybindsHaveConflicts(keybinds: TradeKeybinds) {
  return new Set(Object.values(keybinds)).size !== Object.values(keybinds).length;
}

export function isTradeKeybindMatch(key: string, binding: string) {
  return normalizeTradeKeybindKey(key) === binding;
}
