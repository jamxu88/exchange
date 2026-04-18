import { cookies } from "next/headers";
import { redirect } from "next/navigation";
import { defaultRouteForRole, readSessionFromCookieValue, SESSION_COOKIE } from "@/lib/auth";

type LoginPageProps = {
  searchParams?: Promise<{
    error?: string;
  }>;
};

function errorCopy(error?: string) {
  if (error === "missing-api-key") {
    return "Enter the assigned API key to start the competition session.";
  }
  if (error === "invalid-api-key") {
    return "The exchange rejected that API key. Check the assigned key and try again.";
  }
  if (error === "exchange-unavailable") {
    return "The exchange could not be reached for login validation. Try again shortly.";
  }
  if (error === "session-expired") {
    return "Your session is no longer valid. Sign in again.";
  }

  return null;
}

export default async function LoginPage({ searchParams }: LoginPageProps) {
  const cookieStore = await cookies();
  const session = readSessionFromCookieValue(cookieStore.get(SESSION_COOKIE)?.value);

  if (session) {
    redirect(defaultRouteForRole(session.role));
  }

  const resolvedSearchParams = searchParams ? await searchParams : undefined;
  const errorMessage = errorCopy(resolvedSearchParams?.error);

  return (
    <main className="mx-auto flex min-h-screen w-full max-w-xl flex-col justify-center px-6 py-8">
      <section className="surface-panel overflow-hidden px-6 py-6 lg:px-8 lg:py-8">
        <form
          className="mx-auto flex w-full max-w-md flex-col gap-5"
          action="/api/auth/login"
          method="post"
        >
          <div className="text-center">
            <p className="text-sm font-semibold uppercase tracking-[0.28em] text-[var(--muted)]">
              Exchange Access
            </p>
          </div>
          <label className="block">
            <input
              autoComplete="off"
              className="block w-full rounded-2xl border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-4 py-3 text-xl text-[var(--text-primary)] outline-none"
              name="apiKey"
              placeholder="API key"
              spellCheck={false}
              type="password"
            />
          </label>
          {errorMessage ? (
            <p className="rounded-2xl border border-[rgba(216,91,91,0.42)] bg-[rgba(216,91,91,0.1)] px-4 py-3 text-lg text-[color:var(--red-strong)]">
              {errorMessage}
            </p>
          ) : null}
          <button
            className="w-full rounded-2xl bg-[var(--green)] px-4 py-3 text-xl font-bold text-[var(--background)] shadow-[0_0_24px_rgba(66,204,78,0.25)] hover:translate-y-[-1px] hover:brightness-[1.04]"
            type="submit"
          >
            Start session
          </button>
        </form>
      </section>
    </main>
  );
}
