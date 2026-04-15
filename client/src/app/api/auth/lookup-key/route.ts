import { NextResponse } from "next/server";
import { ApiKeyLookupError, lookupAllocatedApiKey } from "@/lib/api-key-lookup";
import { enforceIpRateLimit, IpRateLimitError } from "@/lib/ip-rate-limit";

const LOOKUP_RATE_LIMIT = 10;
const LOOKUP_RATE_WINDOW_MS = 60_000;
export const runtime = "nodejs";

function requestIp(request: Request) {
  const forwardedFor = request.headers.get("x-forwarded-for");
  if (forwardedFor) {
    return forwardedFor.split(",")[0]?.trim() ?? null;
  }

  return request.headers.get("x-real-ip");
}

export async function POST(request: Request) {
  const responseHeaders = new Headers({
    "cache-control": "no-store",
  });

  try {
    enforceIpRateLimit(requestIp(request), LOOKUP_RATE_LIMIT, LOOKUP_RATE_WINDOW_MS);
  } catch (error) {
    if (error instanceof IpRateLimitError) {
      responseHeaders.set("retry-after", String(error.retryAfterSeconds));
      return NextResponse.json({ error: error.message }, { status: 429, headers: responseHeaders });
    }

    return NextResponse.json(
      { error: "Unable to evaluate the current rate limit." },
      { status: 500, headers: responseHeaders },
    );
  }

  let payload: { identifier?: string };
  try {
    payload = (await request.json()) as { identifier?: string };
  } catch {
    return NextResponse.json(
      { error: "Request body must be valid JSON." },
      { status: 400, headers: responseHeaders },
    );
  }

  try {
    const identifier = typeof payload.identifier === "string" ? payload.identifier : "";
    const result = await lookupAllocatedApiKey(identifier);
    return NextResponse.json(result, { headers: responseHeaders });
  } catch (error) {
    if (error instanceof ApiKeyLookupError) {
      return NextResponse.json({ error: error.message }, { status: error.status, headers: responseHeaders });
    }

    return NextResponse.json(
      { error: "Unable to look up the API key right now." },
      { status: 500, headers: responseHeaders },
    );
  }
}
