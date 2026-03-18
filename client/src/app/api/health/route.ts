import { NextResponse } from "next/server";

export async function GET() {
  return NextResponse.json({
    status: "ok",
    service: "client",
    timestamp: new Date().toISOString(),
  });
}
