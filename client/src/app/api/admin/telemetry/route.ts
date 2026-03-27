import { cookies } from "next/headers";
import { NextResponse } from "next/server";
import { readSessionFromCookieValue, SESSION_COOKIE } from "@/lib/auth";
import { ExchangeServerError, getAdminTelemetry } from "@/lib/exchange-server";

export async function GET() {
  const cookieStore = await cookies();
  const session = readSessionFromCookieValue(cookieStore.get(SESSION_COOKIE)?.value);

  if (!session) {
    return NextResponse.json({ error: "missing session" }, { status: 401 });
  }

  if (session.role !== "admin") {
    return NextResponse.json({ error: "admin access required" }, { status: 403 });
  }

  try {
    const telemetry = await getAdminTelemetry(session.apiKey);
    return NextResponse.json(telemetry, {
      headers: {
        "cache-control": "no-store",
      },
    });
  } catch (error) {
    const status = error instanceof ExchangeServerError ? error.status : 502;
    const message = error instanceof Error
      ? error.message
      : "Failed to load exchange telemetry.";
    return NextResponse.json({ error: message }, { status });
  }
}
