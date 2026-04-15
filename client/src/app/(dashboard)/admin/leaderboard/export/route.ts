import { cookies } from "next/headers";
import { NextResponse } from "next/server";
import { readSessionFromCookieValue, SESSION_COOKIE } from "@/lib/auth";
import {
  ExchangeServerError,
  getAdminLeaderboard,
} from "@/lib/exchange-server";

function escapeCsv(value: string | number) {
  const text = String(value);
  if (/[",\n]/.test(text)) {
    return `"${text.replace(/"/g, "\"\"")}"`;
  }
  return text;
}

function leaderboardToCsv(
  rows: Awaited<ReturnType<typeof getAdminLeaderboard>>,
) {
  const header = [
    "rank",
    "team_number",
    "trader_id",
    "net_pnl",
    "realized_pnl",
    "unrealized_pnl",
    "gross_exposure",
  ];
  const lines = rows.map((row) =>
    [
      row.rank,
      row.team_number,
      row.trader_id,
      row.net_pnl,
      row.realized_pnl,
      row.unrealized_pnl,
      row.gross_exposure,
    ].map(escapeCsv).join(","));
  return [header.join(","), ...lines].join("\n");
}

function exportFilename() {
  const now = new Date().toISOString().replace(/[:.]/g, "-");
  return `leaderboard-${now}.csv`;
}

export async function GET(request: Request) {
  const cookieStore = await cookies();
  const session = readSessionFromCookieValue(cookieStore.get(SESSION_COOKIE)?.value);

  if (!session) {
    return NextResponse.redirect(new URL("/login", request.url));
  }

  try {
    const rows = await getAdminLeaderboard(session.apiKey);
    const csv = leaderboardToCsv(rows);
    return new NextResponse(csv, {
      status: 200,
      headers: {
        "content-type": "text/csv; charset=utf-8",
        "content-disposition": `attachment; filename="${exportFilename()}"`,
        "cache-control": "no-store",
      },
    });
  } catch (error) {
    if (error instanceof ExchangeServerError && error.status === 401) {
      return NextResponse.redirect(new URL("/trade", request.url));
    }

    const message = error instanceof Error
      ? error.message
      : "Failed to export leaderboard.";
    return NextResponse.json({ error: message }, { status: 502 });
  }
}
