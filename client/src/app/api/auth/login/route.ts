import { cookies } from "next/headers";
import { NextResponse } from "next/server";
import {
  defaultRouteForRole,
  encodeSessionCookie,
  SESSION_COOKIE,
} from "@/lib/auth";
import { authenticateExchangeSession, ExchangeServerError } from "@/lib/exchange-server";
import { buildExternalUrl } from "@/lib/request-origin";

export async function POST(request: Request) {
  const form = await request.formData();
  const apiKey = String(form.get("apiKey") ?? "").trim();

  if (!apiKey) {
    return NextResponse.redirect(buildExternalUrl("/login?error=missing-api-key", request));
  }

  let session;
  try {
    session = await authenticateExchangeSession(apiKey);
  } catch (error) {
    if (error instanceof ExchangeServerError && error.status < 500) {
      return NextResponse.redirect(buildExternalUrl("/login?error=invalid-api-key", request));
    }
    return NextResponse.redirect(buildExternalUrl("/login?error=exchange-unavailable", request));
  }

  const cookieStore = await cookies();
  cookieStore.set(SESSION_COOKIE, encodeSessionCookie(session), {
    httpOnly: true,
    sameSite: "lax",
    secure: process.env.NODE_ENV === "production",
    path: "/",
  });

  return NextResponse.redirect(buildExternalUrl(defaultRouteForRole(session.role), request));
}
