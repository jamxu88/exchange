import { cookies } from "next/headers";
import { NextResponse } from "next/server";
import { SESSION_COOKIE } from "@/lib/auth";
import { buildExternalUrl } from "@/lib/request-origin";

export async function POST(request: Request) {
  const cookieStore = await cookies();
  cookieStore.delete(SESSION_COOKIE);

  return NextResponse.redirect(buildExternalUrl("/login", request));
}
