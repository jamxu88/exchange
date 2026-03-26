import {
  DEFAULT_TRADE_KEYBINDS,
  TRADE_PREFERENCES_STORAGE_KEY,
  keybindsHaveConflicts,
  loadTradePreferences,
  normalizeTradeKeybindKey,
  saveTradePreferences,
} from "@/components/trade/trade-preferences";

describe("trade preferences", () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  it("normalizes printable keys and enter-style keys", () => {
    expect(normalizeTradeKeybindKey("b")).toBe("B");
    expect(normalizeTradeKeybindKey(" ")).toBe("Space");
    expect(normalizeTradeKeybindKey("Enter")).toBe("Enter");
    expect(normalizeTradeKeybindKey("Shift")).toBeNull();
  });

  it("round-trips saved preferences from local storage", () => {
    saveTradePreferences({
      keybinds: {
        ...DEFAULT_TRADE_KEYBINDS,
        buy: "X",
      },
      executionSound: {
        name: "fill.wav",
        dataUrl: "data:audio/wav;base64,AAAA",
      },
    });

    expect(window.localStorage.getItem(TRADE_PREFERENCES_STORAGE_KEY)).toContain("fill.wav");
    expect(loadTradePreferences()).toEqual({
      keybinds: {
        ...DEFAULT_TRADE_KEYBINDS,
        buy: "X",
      },
      executionSound: {
        name: "fill.wav",
        dataUrl: "data:audio/wav;base64,AAAA",
      },
    });
  });

  it("detects conflicting keybind assignments", () => {
    expect(
      keybindsHaveConflicts({
        ...DEFAULT_TRADE_KEYBINDS,
        sell: "B",
      }),
    ).toBe(true);
  });
});
