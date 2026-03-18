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
    <main className="mx-auto flex min-h-screen w-full max-w-5xl flex-col justify-center px-6 py-8 lg:px-10">
      <section className="surface-panel grid gap-8 overflow-hidden px-6 py-6 lg:grid-cols-[1.1fr_0.9fr] lg:px-8 lg:py-8">
        <div className="flex flex-col justify-between gap-8">
          <div>
            <p className="text-sm font-semibold uppercase tracking-[0.28em] text-[var(--muted)]">
              Access
            </p>
            <h1 className="mt-3 text-5xl font-extrabold leading-none text-white sm:text-6xl">
              Sign in to the exchange client.
            </h1>
            <p className="mt-4 max-w-xl text-2xl leading-tight text-[var(--muted-strong)]">
              Enter the API key assigned to you for the internal competition.
              The same key is used for the authenticated trade session and the
              live exchange connection.
            </p>
          </div>
          <div className="surface-panel-soft grid gap-4 p-5 text-xl text-[var(--muted-strong)]">
            <p className="font-semibold text-white">Session notes</p>
            <p>Each user signs in with an assigned API key.</p>
            <p>Admin access is derived from the configured admin key list.</p>
            <p>The trade client reuses the same session for REST and WS access.</p>
          </div>
        </div>

        <form
          className="surface-panel-soft space-y-5 p-6"
          action="/api/auth/login"
          method="post"
        >
          <div>
            <p className="text-sm font-semibold uppercase tracking-[0.24em] text-[var(--muted)]">
              Authentication
            </p>
            <p className="mt-2 text-3xl font-bold text-white">API key login</p>
          </div>
          <label className="block text-xl font-semibold text-white">
            Assigned API key
            <input
              autoComplete="off"
              className="mt-2 block w-full rounded-2xl border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-4 py-3 text-xl text-white outline-none"
              name="apiKey"
              placeholder="paste your competition key"
              spellCheck={false}
              type="password"
            />
          </label>
          {errorMessage ? (
            <p className="rounded-2xl border border-[rgba(216,91,91,0.42)] bg-[rgba(216,91,91,0.1)] px-4 py-3 text-lg text-[#ffb2b2]">
              {errorMessage}
            </p>
          ) : null}
          <button
            className="w-full rounded-2xl bg-[var(--green)] px-4 py-3 text-xl font-bold text-white shadow-[0_0_24px_rgba(66,204,78,0.25)] hover:translate-y-[-1px] hover:bg-[#4dd859]"
            type="submit"
          >
            Start session
          </button>
          <p className="text-lg text-[var(--muted)]">
            Trader or admin access is derived from the key you were assigned.
          </p>
        </form>
      </section>
    </main>
  );
}
