import { describe, expect, it } from "vitest";
import { buildExternalUrl, getExternalOrigin } from "./request-origin";

function makeRequest(
  url: string,
  headers: Record<string, string> = {},
): { headers: Headers; url: string } {
  return { url, headers: new Headers(headers) };
}

describe("request origin helpers", () => {
  it("prefers forwarded host and proto for proxied requests", () => {
    const request = makeRequest("http://ip-172-31-8-125.us-east-2.compute.internal:3000/api/auth/login", {
      "x-forwarded-host": "exchange.jamesxu.dev",
      "x-forwarded-proto": "https",
      host: "ip-172-31-8-125.us-east-2.compute.internal:3000",
    });

    expect(getExternalOrigin(request)).toBe("https://exchange.jamesxu.dev");
    expect(buildExternalUrl("/trade", request).toString()).toBe("https://exchange.jamesxu.dev/trade");
  });

  it("falls back to the host header when forwarded headers are absent", () => {
    const request = makeRequest("http://localhost:3000/login", {
      host: "localhost:3000",
    });

    expect(getExternalOrigin(request)).toBe("http://localhost:3000");
  });

  it("falls back to the request url origin when headers are unavailable", () => {
    const request = makeRequest("https://exchange.jamesxu.dev/login");

    expect(getExternalOrigin(request)).toBe("https://exchange.jamesxu.dev");
  });
});
