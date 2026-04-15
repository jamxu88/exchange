type RateLimitBucket = {
  count: number;
  windowStartedAtMs: number;
};

export class IpRateLimitError extends Error {
  retryAfterSeconds: number;

  constructor(message: string, retryAfterSeconds: number) {
    super(message);
    this.name = "IpRateLimitError";
    this.retryAfterSeconds = retryAfterSeconds;
  }
}

const buckets = new Map<string, RateLimitBucket>();

export function resolveRateLimitKey(value: string | null | undefined) {
  const normalized = value?.trim();
  return normalized ? normalized : "unknown";
}

export function enforceIpRateLimit(
  key: string | null | undefined,
  limit: number,
  windowMs: number,
  nowMs = Date.now(),
) {
  const resolvedKey = resolveRateLimitKey(key);
  const existing = buckets.get(resolvedKey);

  if (!existing || nowMs - existing.windowStartedAtMs >= windowMs) {
    buckets.set(resolvedKey, { count: 1, windowStartedAtMs: nowMs });
    return;
  }

  if (existing.count >= limit) {
    const retryAfterSeconds = Math.max(
      1,
      Math.ceil((existing.windowStartedAtMs + windowMs - nowMs) / 1000),
    );
    throw new IpRateLimitError(
      `Rate limit exceeded: max ${limit} requests per ${Math.round(windowMs / 60000)} minute.`,
      retryAfterSeconds,
    );
  }

  existing.count += 1;
}

export function resetIpRateLimitBucketsForTests() {
  buckets.clear();
}
