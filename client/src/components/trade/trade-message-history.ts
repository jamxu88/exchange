"use client";

import type { MessageEntry } from "@/components/trade/trade-types";

export const TRADE_MESSAGE_HISTORY_STORAGE_KEY = "exchange.trade.message_history.v1";
const MAX_STORED_TRADE_MESSAGES = 18;

function isMessageEntry(value: unknown): value is MessageEntry {
  if (!value || typeof value !== "object") {
    return false;
  }

  const entry = value as Record<string, unknown>;
  return (
    typeof entry.id === "number" &&
    typeof entry.time === "string" &&
    typeof entry.tone === "string" &&
    typeof entry.text === "string"
  );
}

function normalizeMessageHistory(messages: MessageEntry[]) {
  return messages.slice(-MAX_STORED_TRADE_MESSAGES).map((message, index) => ({
    ...message,
    id: index + 1,
  }));
}

export function loadTradeMessageHistory() {
  if (typeof window === "undefined") {
    return [] as MessageEntry[];
  }

  const raw = window.localStorage.getItem(TRADE_MESSAGE_HISTORY_STORAGE_KEY);
  if (!raw) {
    return [] as MessageEntry[];
  }

  try {
    const parsed = JSON.parse(raw) as { messages?: unknown };
    if (!Array.isArray(parsed.messages)) {
      return [] as MessageEntry[];
    }

    return normalizeMessageHistory(parsed.messages.filter(isMessageEntry));
  } catch {
    return [] as MessageEntry[];
  }
}

export function saveTradeMessageHistory(messages: MessageEntry[]) {
  if (typeof window === "undefined") {
    return;
  }

  window.localStorage.setItem(
    TRADE_MESSAGE_HISTORY_STORAGE_KEY,
    JSON.stringify({
      messages: normalizeMessageHistory(messages),
    }),
  );
}
