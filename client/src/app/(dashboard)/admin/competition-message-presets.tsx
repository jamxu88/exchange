"use client";

import { useState } from "react";
import { sendMessageAction } from "@/app/(dashboard)/admin/actions";
import {
  inputClass,
  neutralButtonClass,
  primaryButtonClass,
  textareaClass,
} from "@/app/(dashboard)/admin/ui";

type MessageLevel = "info" | "warning" | "critical";

type MessagePreset = {
  id: string;
  label: string;
  title: string;
  body: string;
  level?: MessageLevel;
};

type RoundPresets = {
  id: string;
  name: string;
  blurb: string;
  presets: MessagePreset[];
};

const ROUNDS: RoundPresets[] = [
  {
    id: "round-1",
    name: "Round 1",
    blurb: "Warm-Up · 2 contracts · 8m trading + 2m debrief",
    presets: [
      {
        id: "r1-start",
        label: "Round start",
        title: "Round 1 — Warm-Up",
        body:
          "Round 1 is live (8 minutes of trading).\n\nMarkets:\n• Total Sum — sum of all 10 card values.\n• Range — (highest − lowest) × 10.\n\nInfo reveals every 2 minutes. Good luck.",
      },
      {
        id: "r1-info-1",
        label: "Info 1 — Distinct values",
        title: "Round 1 · Info 1",
        body:
          "Distinct card values among the 10 cards: [FILL].",
      },
      {
        id: "r1-info-2",
        label: "Info 2 — Pos 3/6/9",
        title: "Round 1 · Info 2",
        body:
          "Sum of card values in Positions 3, 6, 9: [FILL].\nSuits (P3, P6, P9): [FILL], [FILL], [FILL].",
      },
      {
        id: "r1-info-3",
        label: "Info 3 — Highest card",
        title: "Round 1 · Info 3",
        body:
          "Highest card among all 10: [FILL] of [FILL].",
      },
      {
        id: "r1-debrief",
        label: "Debrief",
        title: "Round 1 debrief",
        body: "Round 1 trading closed. 2-minute debrief, then Round 2 begins.",
      },
    ],
  },
  {
    id: "round-2",
    name: "Round 2",
    blurb: "3 contracts · 8m trading + 2m debrief",
    presets: [
      {
        id: "r2-start",
        label: "Round start",
        title: "Round 2",
        body:
          "Round 2 is live (8 minutes of trading).\n\nMarkets:\n• Total Sum — sum of all 10 card values.\n• Odd vs Even Positions — (sum odd − sum even) + 100.\n• Red vs Black — (# red − # black) × (sum red − sum black). Can settle negative.\n\nInfo reveals every 2 minutes.",
      },
      {
        id: "r2-info-1",
        label: "Info 1 — Distinct values",
        title: "Round 2 · Info 1",
        body: "Distinct card values among the 10 cards: [FILL].",
      },
      {
        id: "r2-info-2",
        label: "Info 2 — Pos 3/6/9",
        title: "Round 2 · Info 2",
        body:
          "Sum of card values in Positions 3, 6, 9: [FILL].\nSuits (P3, P6, P9): [FILL], [FILL], [FILL].",
      },
      {
        id: "r2-info-3",
        label: "Info 3 — Highest card",
        title: "Round 2 · Info 3",
        body: "Highest card among all 10: [FILL] of [FILL].",
      },
      {
        id: "r2-debrief",
        label: "Debrief",
        title: "Round 2 debrief",
        body: "Round 2 trading closed. 2-minute debrief, then Round 3 begins.",
      },
    ],
  },
  {
    id: "round-3",
    name: "Round 3",
    blurb: "3 contracts · 8m trading + 2m debrief",
    presets: [
      {
        id: "r3-start",
        label: "Round start",
        title: "Round 3",
        body:
          "Round 3 is live (8 minutes of trading).\n\nMarkets:\n• Total Sum — sum of all 10 card values.\n• High Card Weighted Sum — 10 × (# cards ≥ 10) + (sum of those cards).\n• Suit Dominance — (sum of most-common-suit values) × (count of second-most-common suit). Ties → higher total value.\n\nInfo reveals every 2 minutes.",
      },
      {
        id: "r3-info-1",
        label: "Info 1 — Distinct values",
        title: "Round 3 · Info 1",
        body: "Distinct card values among the 10 cards: [FILL].",
      },
      {
        id: "r3-info-2",
        label: "Info 2 — Pos 3/6/9",
        title: "Round 3 · Info 2",
        body:
          "Sum of card values in Positions 3, 6, 9: [FILL].\nSuits (P3, P6, P9): [FILL], [FILL], [FILL].",
      },
      {
        id: "r3-info-3",
        label: "Info 3 — Highest card",
        title: "Round 3 · Info 3",
        body: "Highest card among all 10: [FILL] of [FILL].",
      },
      {
        id: "r3-debrief",
        label: "Debrief",
        title: "Round 3 debrief",
        body: "Round 3 trading closed. 2-minute debrief, then Round 4 begins.",
      },
    ],
  },
  {
    id: "round-4",
    name: "Round 4",
    blurb: "3 contracts · 8m trading + 2m debrief",
    presets: [
      {
        id: "r4-start",
        label: "Round start",
        title: "Round 4",
        body:
          "Round 4 is live (8 minutes of trading).\n\nMarkets:\n• Total Sum — sum of all 10 card values.\n• Suit Frequency Product — top three suit counts multiplied together, × 10.\n• Median — Median × 10 (average of 5th and 6th largest cards).\n\nInfo reveals every 2 minutes.",
      },
      {
        id: "r4-info-1",
        label: "Info 1 — Distinct values",
        title: "Round 4 · Info 1",
        body: "Distinct card values among the 10 cards: [FILL].",
      },
      {
        id: "r4-info-2",
        label: "Info 2 — Pos 3/6/9",
        title: "Round 4 · Info 2",
        body:
          "Sum of card values in Positions 3, 6, 9: [FILL].\nSuits (P3, P6, P9): [FILL], [FILL], [FILL].",
      },
      {
        id: "r4-info-3",
        label: "Info 3 — Highest card",
        title: "Round 4 · Info 3",
        body: "Highest card among all 10: [FILL] of [FILL].",
      },
      {
        id: "r4-debrief",
        label: "Debrief",
        title: "Round 4 debrief",
        body: "Round 4 trading closed. 2-minute debrief, then Round 5 begins.",
      },
    ],
  },
  {
    id: "round-5",
    name: "Round 5",
    blurb: "Poker Hand · 12m trading · 5 info reveals",
    presets: [
      {
        id: "r5-start",
        label: "Round start",
        title: "Round 5 — Poker Hand",
        body:
          "Round 5 is live (12 minutes of trading). This round has five info reveals at 2-minute intervals.\n\nMarkets: (see admin panel).\n\nGood luck.",
      },
      {
        id: "r5-info-1",
        label: "Info 1 — Distinct values",
        title: "Round 5 · Info 1",
        body: "Distinct card values among the 10 cards: [FILL].",
      },
      {
        id: "r5-info-2",
        label: "Info 2 — Least-common suit",
        title: "Round 5 · Info 2",
        body:
          "Times the least-common suit appears among the 10 cards: [FILL].",
      },
      {
        id: "r5-info-3",
        label: "Info 3 — Pos 3/6/9",
        title: "Round 5 · Info 3",
        body:
          "Sum of card values in Positions 3, 6, 9: [FILL].\nSuits (P3, P6, P9): [FILL], [FILL], [FILL].",
      },
      {
        id: "r5-info-4",
        label: "Info 4 — Highest card",
        title: "Round 5 · Info 4",
        body: "Highest card among all 10: [FILL] of [FILL].",
      },
      {
        id: "r5-info-5",
        label: "Info 5 — Repeated value",
        title: "Round 5 · Info 5",
        body:
          "Highest card value that appears at least twice: [FILL].\n(If none: \"No repeated values.\")",
      },
    ],
  },
];

type DraftState = Record<string, { title: string; body: string }>;

function buildInitialDrafts(): DraftState {
  const drafts: DraftState = {};
  for (const round of ROUNDS) {
    for (const preset of round.presets) {
      drafts[preset.id] = { title: preset.title, body: preset.body };
    }
  }
  return drafts;
}

export function CompetitionMessagePresets() {
  const [activeRoundId, setActiveRoundId] = useState<string>(ROUNDS[0].id);
  const [drafts, setDrafts] = useState<DraftState>(buildInitialDrafts);

  const activeRound = ROUNDS.find((round) => round.id === activeRoundId) ?? ROUNDS[0];

  const updateDraft = (
    presetId: string,
    patch: Partial<{ title: string; body: string }>,
  ) => {
    setDrafts((current) => ({
      ...current,
      [presetId]: { ...current[presetId], ...patch },
    }));
  };

  const resetDraft = (preset: MessagePreset) => {
    updateDraft(preset.id, { title: preset.title, body: preset.body });
  };

  return (
    <div className="grid gap-4">
      <div className="flex flex-wrap gap-2">
        {ROUNDS.map((round) => {
          const isActive = round.id === activeRoundId;
          return (
            <button
              key={round.id}
              type="button"
              onClick={() => setActiveRoundId(round.id)}
              className={
                isActive
                  ? `${primaryButtonClass} !py-2 !px-3 text-[13px]`
                  : `${neutralButtonClass} !py-2 !px-3 text-[13px]`
              }
            >
              {round.name}
            </button>
          );
        })}
      </div>

      <p className="text-sm text-[var(--muted)]">{activeRound.blurb}</p>

      <div className="grid gap-3">
        {activeRound.presets.map((preset) => {
          const draft = drafts[preset.id] ?? { title: preset.title, body: preset.body };
          const level: MessageLevel = preset.level ?? "info";
          return (
            <form
              key={preset.id}
              action={sendMessageAction}
              className="ops-panel-soft grid gap-2 px-4 py-3"
            >
              <input type="hidden" name="audience" value="all" />
              <input type="hidden" name="level" value={level} />

              <div className="flex flex-wrap items-center justify-between gap-2">
                <span className="text-[11px] font-semibold uppercase tracking-wide text-[var(--muted-strong)]">
                  {preset.label}
                </span>
                <button
                  type="button"
                  onClick={() => resetDraft(preset)}
                  className="text-[11px] font-semibold uppercase tracking-wide text-[var(--muted)] hover:text-white"
                >
                  Reset
                </button>
              </div>

              <input
                className={inputClass}
                name="title"
                value={draft.title}
                onChange={(event) =>
                  updateDraft(preset.id, { title: event.target.value })
                }
                placeholder="Title"
              />
              <textarea
                className={`${textareaClass} min-h-24`}
                name="body"
                value={draft.body}
                onChange={(event) =>
                  updateDraft(preset.id, { body: event.target.value })
                }
                required
              />
              <div className="flex justify-end">
                <button type="submit" className={`${primaryButtonClass} !py-2`}>
                  Send to all
                </button>
              </div>
            </form>
          );
        })}
      </div>
    </div>
  );
}
