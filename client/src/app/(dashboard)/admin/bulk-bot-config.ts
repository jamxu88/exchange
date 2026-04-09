export const MAX_BULK_BOT_COUNT = 50;

const BOT_ID_PREFIX_PATTERN = /^[a-z0-9-]+$/;

export type BulkBotDefinition = {
  botId: string;
  displayName: string | null;
};

type ResolveBulkBotDefinitionsInput = {
  botIdPrefix: string;
  displayNamePrefix: string | null;
  count: number;
  startIndex: number;
};

function normalizeBotIdPrefix(raw: string) {
  const normalized = raw
    .trim()
    .toLowerCase()
    .replace(/-+/g, "-")
    .replace(/^-+|-+$/g, "");

  if (!normalized) {
    throw new Error("Bot ID prefix is required for batch creation.");
  }

  if (!BOT_ID_PREFIX_PATTERN.test(normalized)) {
    throw new Error("Bot ID prefix may only contain letters, numbers, and hyphens.");
  }

  return normalized;
}

function requireWholeNumber(value: number, fieldLabel: string) {
  if (!Number.isInteger(value) || value < 1) {
    throw new Error(`${fieldLabel} must be a whole number greater than zero.`);
  }
}

export function resolveBulkBotDefinitions({
  botIdPrefix,
  displayNamePrefix,
  count,
  startIndex,
}: ResolveBulkBotDefinitionsInput): BulkBotDefinition[] {
  const normalizedPrefix = normalizeBotIdPrefix(botIdPrefix);
  requireWholeNumber(count, "Bot count");
  requireWholeNumber(startIndex, "Start index");

  if (count > MAX_BULK_BOT_COUNT) {
    throw new Error(`Bot count cannot exceed ${MAX_BULK_BOT_COUNT} per batch.`);
  }

  const trimmedDisplayNamePrefix = displayNamePrefix?.trim() || null;

  return Array.from({ length: count }, (_, offset) => {
    const index = startIndex + offset;

    return {
      botId: `${normalizedPrefix}-${index}`,
      displayName: trimmedDisplayNamePrefix
        ? `${trimmedDisplayNamePrefix} ${index}`
        : null,
    };
  });
}
