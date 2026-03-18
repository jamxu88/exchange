import { cookies } from "next/headers";
import { NextResponse } from "next/server";
import {
  createSessionForApiKey,
  defaultRouteForRole,
  encodeSessionCookie,
  SESSION_COOKIE,
} from "@/lib/auth";

export async function POST(request: Request) {
  const form = await request.formData();
  const apiKey = String(form.get("apiKey") ?? "").trim();

  if (!apiKey) {
    return NextResponse.redirect(new URL("/login?error=missing-api-key", request.url));
  }

  const session = createSessionForApiKey(apiKey);
  const cookieStore = await cookies();
  cookieStore.set(SESSION_COOKIE, encodeSessionCookie(session), {
    httpOnly: true,
    sameSite: "lax",
    secure: process.env.NODE_ENV === "production",
    path: "/",
  });

  return NextResponse.redirect(new URL(defaultRouteForRole(session.role), request.url));
}
