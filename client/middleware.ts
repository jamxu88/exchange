import { NextRequest, NextResponse } from "next/server";
import { SESSION_COOKIE } from "./src/lib/auth";

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

  const role = request.cookies.get(SESSION_COOKIE)?.value;
  if (!role) {
    return NextResponse.redirect(new URL("/login", request.url));
  }

  const isAdminRoute = ADMIN_PATHS.some((path) => pathname.startsWith(path));
  if (isAdminRoute && role !== "admin") {
    return NextResponse.redirect(new URL("/trade", request.url));
  }

  return NextResponse.next();
}

export const config = {
  matcher: ["/trade/:path*", "/admin/:path*"],
};
