"use client";

import { useMemo, useState } from "react";
import { sendCardDealsAction } from "@/app/(dashboard)/admin/actions";
import {
  inputClass,
  primaryButtonClass,
  selectClass,
} from "@/app/(dashboard)/admin/ui";

const CARD_VALUES = [
  "A",
  "2",
  "3",
  "4",
  "5",
  "6",
  "7",
  "8",
  "9",
  "10",
  "J",
  "Q",
  "K",
] as const;

const SUITS: Array<{ id: "S" | "H" | "D" | "C"; label: string; glyph: string }> = [
  { id: "S", label: "spades", glyph: "♠" },
  { id: "H", label: "hearts", glyph: "♥" },
  { id: "D", label: "diamonds", glyph: "♦" },
  { id: "C", label: "clubs", glyph: "♣" },
];

const POSITIONS = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10] as const;
const PUBLIC_POSITIONS = new Set<number>([3, 6, 9]);

type CardDraft = { value: string; suit: "" | "S" | "H" | "D" | "C" };

function emptyDraft(): CardDraft {
  return { value: "", suit: "" };
}

function initialCards(): CardDraft[] {
  return POSITIONS.map(() => emptyDraft());
}

function suitGlyph(suit: CardDraft["suit"]) {
  const hit = SUITS.find((entry) => entry.id === suit);
  return hit ? hit.glyph : "";
}

function suitColorClass(suit: CardDraft["suit"]) {
  if (suit === "H" || suit === "D") {
    return "text-[#ff6b6b]";
  }
  if (suit === "S" || suit === "C") {
    return "text-white";
  }
  return "text-[var(--muted)]";
}

export function CardDealBroadcaster() {
  const [roundLabel, setRoundLabel] = useState("");
  const [cards, setCards] = useState<CardDraft[]>(initialCards);

  const allCardsSet = useMemo(
    () => cards.every((card) => card.value.length > 0 && card.suit.length > 0),
    [cards],
  );

  const updateCard = (index: number, patch: Partial<CardDraft>) => {
    setCards((current) =>
      current.map((card, idx) => (idx === index ? { ...card, ...patch } : card)),
    );
  };

  return (
    <form action={sendCardDealsAction} className="grid gap-4">
      <div className="grid gap-2">
        <label className="text-[11px] font-semibold uppercase tracking-wide text-[var(--muted-strong)]">
          Round label (optional; shown as a prefix before the card line)
        </label>
        <input
          className={inputClass}
          name="roundLabel"
          value={roundLabel}
          onChange={(event) => setRoundLabel(event.target.value)}
          placeholder="e.g. Round 1"
        />
      </div>

      <div className="grid gap-2">
        <div className="flex items-center justify-between">
          <span className="text-[11px] font-semibold uppercase tracking-wide text-[var(--muted-strong)]">
            10 drawn cards (position 1 = leftmost)
          </span>
          <span className="text-[11px] text-[var(--muted)]">
            Positions 3, 6, 9 are public (never dealt privately)
          </span>
        </div>
        <div className="grid gap-2 sm:grid-cols-2">
          {POSITIONS.map((pos, idx) => {
            const card = cards[idx];
            const isPublic = PUBLIC_POSITIONS.has(pos);
            return (
              <div
                key={pos}
                className={`ops-panel-soft flex items-center gap-3 px-3 py-2 ${
                  isPublic ? "opacity-80" : ""
                }`}
              >
                <span className="w-14 shrink-0 text-[11px] font-semibold uppercase tracking-wide text-[var(--muted-strong)]">
                  Pos {pos}
                  {isPublic ? (
                    <span className="ml-1 text-[10px] font-normal normal-case text-[var(--muted)]">
                      (public)
                    </span>
                  ) : null}
                </span>
                <select
                  className={`${selectClass} !py-1 !px-2 text-[13px]`}
                  name={`value_${pos}`}
                  value={card.value}
                  onChange={(event) => updateCard(idx, { value: event.target.value })}
                  required
                >
                  <option value="">Value</option>
                  {CARD_VALUES.map((value) => (
                    <option key={value} value={value}>
                      {value}
                    </option>
                  ))}
                </select>
                <select
                  className={`${selectClass} !py-1 !px-2 text-[13px]`}
                  name={`suit_${pos}`}
                  value={card.suit}
                  onChange={(event) =>
                    updateCard(idx, { suit: event.target.value as CardDraft["suit"] })
                  }
                  required
                >
                  <option value="">Suit</option>
                  {SUITS.map((suit) => (
                    <option key={suit.id} value={suit.id}>
                      {suit.glyph} {suit.label}
                    </option>
                  ))}
                </select>
                <span
                  className={`ml-auto text-[18px] font-semibold ${suitColorClass(card.suit)}`}
                  aria-hidden
                >
                  {card.value}
                  {suitGlyph(card.suit)}
                </span>
              </div>
            );
          })}
        </div>
      </div>

      <div className="flex items-center justify-between gap-3">
        <p className="text-sm text-[var(--muted)]">
          Each team receives 3 random positions drawn from {"{"}1, 2, 4, 5, 7, 8, 10{"}"}. The random
          selection runs server-side per team when you click send.
        </p>
        <button
          type="submit"
          className={primaryButtonClass}
          disabled={!allCardsSet}
        >
          Deal cards to all teams
        </button>
      </div>
    </form>
  );
}
