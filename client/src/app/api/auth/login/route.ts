import { cookies } from "next/headers";
import { NextResponse } from "next/server";
import {
  defaultRouteForRole,
  encodeSessionCookie,
  SESSION_COOKIE,
} from "@/lib/auth";
import { authenticateExchangeSession, ExchangeServerError } from "@/lib/exchange-server";

export async function POST(request: Request) {
  const form = await request.formData();
  const apiKey = String(form.get("apiKey") ?? "").trim();

  if (!apiKey) {
    return NextResponse.redirect(new URL("/login?error=missing-api-key", request.url));
  }

  let session;
  try {
    session = await authenticateExchangeSession(apiKey);
  } catch (error) {
    if (error instanceof ExchangeServerError && error.status < 500) {
      return NextResponse.redirect(new URL("/login?error=invalid-api-key", request.url));
    }
    return NextResponse.redirect(new URL("/login?error=exchange-unavailable", request.url));
  }

  const cookieStore = await cookies();
  cookieStore.set(SESSION_COOKIE, encodeSessionCookie(session), {
    httpOnly: true,
    sameSite: "lax",
    secure: process.env.NODE_ENV === "production",
    path: "/",
  });

  return NextResponse.redirect(new URL(defaultRouteForRole(session.role), request.url));
}
