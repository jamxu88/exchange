import { cookies } from "next/headers";
import { NextResponse } from "next/server";
import { readSessionFromCookieValue, SESSION_COOKIE } from "@/lib/auth";
import {
  ExchangeServerError,
  getAdminState,
  sendAdminMutation,
} from "@/lib/exchange-server";

export async function withAdminSession<T>(handler: (apiKey: string) => Promise<T>) {
  const cookieStore = await cookies();
  const session = readSessionFromCookieValue(cookieStore.get(SESSION_COOKIE)?.value);

  if (!session) {
    return NextResponse.json({ error: "missing session" }, { status: 401 });
  }

  if (session.role !== "admin") {
    return NextResponse.json({ error: "admin access required" }, { status: 403 });
  }

  try {
    const payload = await handler(session.apiKey);
    return NextResponse.json(payload, {
      headers: {
        "cache-control": "no-store",
      },
    });
  } catch (error) {
    const status = error instanceof ExchangeServerError ? error.status : 502;
    const message = error instanceof Error ? error.message : "Admin request failed.";
    return NextResponse.json({ error: message }, { status });
  }
}

export async function readJson<T>(request: Request): Promise<T> {
  return await request.json() as T;
}

export async function runAdminMutation(
  apiKey: string,
  path: string,
  method: "POST" | "PATCH" | "DELETE",
  notice: string,
  body?: unknown,
) {
  await sendAdminMutation(apiKey, path, method, body);
  const adminState = await getAdminState(apiKey);
  return {
    adminState,
    notice,
  };
}
