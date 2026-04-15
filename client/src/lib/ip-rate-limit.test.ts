import { beforeEach, describe, expect, it } from "vitest";
import {
  enforceIpRateLimit,
  IpRateLimitError,
  resetIpRateLimitBucketsForTests,
} from "@/lib/ip-rate-limit";

describe("enforceIpRateLimit", () => {
  beforeEach(() => {
    resetIpRateLimitBucketsForTests();
  });

  it("allows requests up to the configured limit", () => {
    for (let attempt = 0; attempt < 10; attempt += 1) {
      expect(() => enforceIpRateLimit("1.2.3.4", 10, 60_000, 1_000)).not.toThrow();
    }
  });

  it("rejects requests over the configured limit until the window resets", () => {
    for (let attempt = 0; attempt < 10; attempt += 1) {
      enforceIpRateLimit("1.2.3.4", 10, 60_000, 1_000);
    }

    expect(() => enforceIpRateLimit("1.2.3.4", 10, 60_000, 20_000)).toThrow(IpRateLimitError);
    expect(() => enforceIpRateLimit("1.2.3.4", 10, 60_000, 61_001)).not.toThrow();
  });
});
