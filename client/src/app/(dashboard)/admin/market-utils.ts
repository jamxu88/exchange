export const COMPETITION_QUOTE_ASSET = "USD";
export const COMPETITION_QUOTE_LABEL = "$";

export function normalizeBaseAsset(value: string) {
  return value.trim().toUpperCase();
}

export function deriveCompetitionMarketId(baseAsset: string) {
  const normalizedBaseAsset = normalizeBaseAsset(baseAsset);
  return normalizedBaseAsset
    ? `${normalizedBaseAsset}-${COMPETITION_QUOTE_ASSET}`
    : "";
}
