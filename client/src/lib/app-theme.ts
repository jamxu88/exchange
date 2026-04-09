export const APP_THEME_STORAGE_KEY = "exchange.app.theme.v1";

export type AppTheme = "dark" | "light";

export const DEFAULT_APP_THEME: AppTheme = "dark";

export function normalizeAppTheme(value: unknown): AppTheme {
  return value === "light" ? "light" : "dark";
}

export function loadAppTheme(): AppTheme {
  if (typeof window === "undefined") {
    return DEFAULT_APP_THEME;
  }

  try {
    return normalizeAppTheme(window.localStorage.getItem(APP_THEME_STORAGE_KEY));
  } catch {
    return DEFAULT_APP_THEME;
  }
}

export function saveAppTheme(theme: AppTheme) {
  if (typeof window === "undefined") {
    return;
  }

  window.localStorage.setItem(APP_THEME_STORAGE_KEY, theme);
}

export function applyAppTheme(theme: AppTheme) {
  if (typeof document === "undefined") {
    return;
  }

  document.documentElement.dataset.theme = theme;
  document.documentElement.style.colorScheme = theme;
}

export const APP_THEME_INIT_SCRIPT = `
(() => {
  const defaultTheme = "${DEFAULT_APP_THEME}";
  try {
    const storedTheme = window.localStorage.getItem("${APP_THEME_STORAGE_KEY}");
    const theme = storedTheme === "light" ? "light" : defaultTheme;
    document.documentElement.dataset.theme = theme;
    document.documentElement.style.colorScheme = theme;
  } catch {
    document.documentElement.dataset.theme = defaultTheme;
    document.documentElement.style.colorScheme = defaultTheme;
  }
})();
`.trim();
