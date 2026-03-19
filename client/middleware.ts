import { NextRequest, NextResponse } from "next/server";
import { readSessionFromCookieValue, SESSION_COOKIE } from "./src/lib/auth";
import { buildExternalUrl } from "./src/lib/request-origin";

const PUBLIC_PATHS = ["/", "/login", "/api/health", "/api/auth/mock-login"];
const ADMIN_PATHS = ["/admin"];

export function middleware(request: NextRequest) {
  const { pathname } = request.nextUrl;
  const isPublic = PUBLIC_PATHS.some((path) =>
    path === "/" ? pathname === "/" : pathname.startsWith(path),
  );

  if (isPublic) {
    return NextResponse.next();
  }

  const session = readSessionFromCookieValue(request.cookies.get(SESSION_COOKIE)?.value);
  if (!session) {
    return NextResponse.redirect(buildExternalUrl("/login", request));
  }

  const isAdminRoute = ADMIN_PATHS.some((path) => pathname.startsWith(path));
  if (isAdminRoute && session.role !== "admin") {
    return NextResponse.redirect(buildExternalUrl("/trade", request));
  }

  return NextResponse.next();
}

export const config = {
  matcher: ["/trade/:path*", "/admin/:path*"],
};
