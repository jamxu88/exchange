import { cookies } from "next/headers";
import { redirect } from "next/navigation";
import { TradeConsole } from "@/components/trade/trade-console";
import { createTradeRuntimeConfig } from "@/components/trade/trade-runtime";
import { readSessionFromCookieValue, SESSION_COOKIE } from "@/lib/auth";

export default async function TradePage() {
  const cookieStore = await cookies();
  const session = readSessionFromCookieValue(cookieStore.get(SESSION_COOKIE)?.value);

  if (!session) {
    redirect("/login");
  }

  const runtime = {
    ...createTradeRuntimeConfig(),
    apiKey: session.apiKey,
  };

  return (
    <main className="h-screen overflow-hidden bg-[var(--trade-shell-bg)]">
      <TradeConsole runtime={runtime} />
    </main>
  );
}
