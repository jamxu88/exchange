import { afterEach, describe, expect, it, vi } from "vitest";
import {
  ExchangeServerError,
  authenticateExchangeSession,
} from "@/lib/exchange-server";

const originalFetch = global.fetch;

function jsonResponse(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

describe("authenticateExchangeSession", () => {
  afterEach(() => {
    global.fetch = originalFetch;
    vi.restoreAllMocks();
  });

  it("falls back to trader auth when admin probe returns plain-text 401", async () => {
    global.fetch = vi
      .fn()
      .mockResolvedValueOnce(new Response("invalid admin token", { status: 401 }))
      .mockResolvedValueOnce(
        jsonResponse({
          trader_id: "c7f8c572-8a50-4dbd-a1e1-9f452774b636",
          team_number: "TEAM-TRADER",
        }),
      ) as typeof fetch;

    const session = await authenticateExchangeSession("trader");

    expect(session.role).toBe("trader");
    expect(session.apiKey).toBe("trader");
    expect(global.fetch).toHaveBeenCalledTimes(2);
  });

  it("surfaces invalid api keys as 401 errors", async () => {
    global.fetch = vi
      .fn()
      .mockResolvedValueOnce(new Response("invalid admin token", { status: 401 }))
      .mockResolvedValueOnce(new Response("invalid api key", { status: 401 })) as typeof fetch;

    await expect(authenticateExchangeSession("nope")).rejects.toMatchObject({
      status: 401,
      message: "invalid api key",
    });
  });
});
