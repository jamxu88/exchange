export function formatCurrency(value: number) {
  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    minimumFractionDigits: 0,
    maximumFractionDigits: 0,
  }).format(value);
}

export function formatSignedCurrency(value: number) {
  return value > 0
    ? `+${formatCurrency(value)}`
    : value < 0
      ? `-${formatCurrency(Math.abs(value))}`
      : formatCurrency(0);
}

export function formatTimestamp(value: string) {
  return new Date(value).toLocaleString("en-US", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function toneClass(level: "info" | "warning" | "critical") {
  if (level === "critical") {
    return "text-[color:var(--red-strong)]";
  }
  if (level === "warning") {
    return "text-[color:var(--amber)]";
  }
  return "text-[var(--green)]";
}

export function botStatusClass(status: "paused" | "running") {
  return status === "running"
    ? "border-[#2f6b37] bg-[#102015] text-[#b8ffbd]"
    : "border-[var(--surface-stroke)] bg-[var(--surface-soft)] text-[var(--text-primary)]";
}

export function formatPositionLimit(value: number | null) {
  if (value === null) {
    return "Unlimited";
  }

  return `${value.toLocaleString("en-US")} shares`;
}

export const primaryButtonClass = "ops-button ops-button-primary";
export const neutralButtonClass = "ops-button ops-button-neutral";
export const warningButtonClass = "ops-button ops-button-warning";
export const dangerButtonClass = "ops-button ops-button-danger";
export const inputClass = "ops-input";
export const selectClass = "ops-select";
export const textareaClass = "ops-textarea";
export const cardClass = "ops-panel-soft px-4 py-4 text-[15px] text-[var(--muted-strong)]";
