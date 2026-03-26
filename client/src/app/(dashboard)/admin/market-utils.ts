export const COMPETITION_QUOTE_ASSET = "USD";
export const COMPETITION_MARKET_SUFFIX = "MARKET";

function normalizeCompetitionStem(value: string) {
  return value
    .trim()
    .toUpperCase()
    .replace(/[^A-Z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .replace(/-+/g, "-");
}

function stripCompetitionMarketSuffix(value: string) {
  const marketSuffix = `-${COMPETITION_MARKET_SUFFIX}`;
  return value.endsWith(marketSuffix)
    ? value.slice(0, -marketSuffix.length)
    : value;
}

export function deriveCompetitionBaseAsset(label: string) {
  const normalizedLabel = normalizeCompetitionStem(label);
  if (!normalizedLabel) {
    return "";
  }

  return stripCompetitionMarketSuffix(normalizedLabel);
}

export function deriveCompetitionMarketId(label: string) {
  const stem = deriveCompetitionBaseAsset(label);
  return stem ? `${stem}-${COMPETITION_MARKET_SUFFIX}` : "";
}
