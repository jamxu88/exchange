import { describe, expect, it } from "vitest";
import {
  MAX_BULK_BOT_COUNT,
  resolveBulkBotDefinitions,
} from "@/app/(dashboard)/admin/bulk-bot-config";

describe("resolveBulkBotDefinitions", () => {
  it("builds a sequential batch of bot ids and display names", () => {
    expect(
      resolveBulkBotDefinitions({
        botIdPrefix: "depth-maker",
        displayNamePrefix: "Depth maker",
        count: 3,
        startIndex: 2,
      }),
    ).toEqual([
      { botId: "depth-maker-2", displayName: "Depth maker 2" },
      { botId: "depth-maker-3", displayName: "Depth maker 3" },
      { botId: "depth-maker-4", displayName: "Depth maker 4" },
    ]);
  });

  it("normalizes casing and repeated hyphens in the bot id prefix", () => {
    expect(
      resolveBulkBotDefinitions({
        botIdPrefix: "  Depth--Maker- ",
        displayNamePrefix: null,
        count: 1,
        startIndex: 1,
      }),
    ).toEqual([{ botId: "depth-maker-1", displayName: null }]);
  });

  it("rejects invalid prefixes", () => {
    expect(() =>
      resolveBulkBotDefinitions({
        botIdPrefix: "depth maker",
        displayNamePrefix: null,
        count: 2,
        startIndex: 1,
      }),
    ).toThrow("Bot ID prefix may only contain letters, numbers, and hyphens.");
  });

  it("caps the maximum number of bots per batch", () => {
    expect(() =>
      resolveBulkBotDefinitions({
        botIdPrefix: "depth-maker",
        displayNamePrefix: null,
        count: MAX_BULK_BOT_COUNT + 1,
        startIndex: 1,
      }),
    ).toThrow(`Bot count cannot exceed ${MAX_BULK_BOT_COUNT} per batch.`);
  });
});
