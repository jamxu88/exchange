"use client";

import { useEffect, useState } from "react";
import {
  applyAppTheme,
  DEFAULT_APP_THEME,
  loadAppTheme,
  saveAppTheme,
  type AppTheme,
} from "@/lib/app-theme";

function nextTheme(theme: AppTheme): AppTheme {
  return theme === "dark" ? "light" : "dark";
}

export function ThemeToggle({ className }: { className?: string }) {
  const [theme, setTheme] = useState<AppTheme>(() => loadAppTheme() ?? DEFAULT_APP_THEME);

  useEffect(() => {
    applyAppTheme(theme);
  }, [theme]);

  function handleToggle() {
    const updatedTheme = nextTheme(theme);
    setTheme(updatedTheme);
    saveAppTheme(updatedTheme);
    applyAppTheme(updatedTheme);
  }

  return (
    <button
      aria-label={`Switch to ${nextTheme(theme)} mode`}
      className={`pointer-events-auto inline-flex items-center gap-2 rounded-full border border-[var(--surface-stroke)] bg-[var(--surface-raised)] px-3 py-2 text-[11px] font-semibold uppercase tracking-[0.14em] text-[var(--muted-strong)] shadow-[0_10px_24px_rgba(0,0,0,0.14)] backdrop-blur-xl hover:border-[var(--green)] hover:text-[var(--foreground)] motion-hover-soft ${className ?? ""}`}
      onClick={handleToggle}
      type="button"
    >
      <span className="inline-flex h-2.5 w-2.5 rounded-full bg-[var(--green)]" />
      <span>{theme === "dark" ? "Light mode" : "Dark mode"}</span>
    </button>
  );
}
