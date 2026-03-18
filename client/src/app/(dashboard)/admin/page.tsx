import { cookies } from "next/headers";
import { redirect } from "next/navigation";
import { readSessionFromCookieValue, SESSION_COOKIE } from "@/lib/auth";

export default async function AdminPage() {
  const cookieStore = await cookies();
  const session = readSessionFromCookieValue(cookieStore.get(SESSION_COOKIE)?.value);

  if (!session) {
    redirect("/login");
  }

  if (session.role !== "admin") {
    redirect("/trade");
  }

  return (
    <main className="mx-auto flex min-h-screen w-full max-w-7xl flex-col gap-6 px-6 py-8 lg:px-10">
      <section className="surface-panel flex flex-col gap-6 px-6 py-6 lg:flex-row lg:items-end lg:justify-between lg:px-8">
        <div>
          <p className="text-sm font-semibold uppercase tracking-[0.28em] text-[var(--muted)]">
            Admin
          </p>
          <h1 className="mt-3 text-5xl font-extrabold leading-none text-white">
            Event operations panel
          </h1>
          <p className="mt-4 max-w-3xl text-2xl text-[var(--muted-strong)]">
            The admin route is still a scaffold, but it now lives inside the
            same client visual system as the trading console and surfaces the
            operator workflows called out in the TODO.
          </p>
        </div>
        <div className="surface-panel-soft flex gap-6 px-5 py-4 text-xl text-[var(--muted-strong)]">
          <div>
            <p className="text-3xl font-bold text-white">4</p>
            <p>Ops modules</p>
          </div>
          <div>
            <p className="text-3xl font-bold text-white">24/7</p>
            <p>Runbook surface</p>
          </div>
          <div>
            <p className="text-3xl font-bold text-white">{session.apiKeyPreview}</p>
            <p>Signed-in key</p>
          </div>
          <form action="/api/auth/logout" className="flex items-start" method="post">
            <button
              className="rounded-2xl border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-4 py-3 text-base font-semibold text-white hover:border-[rgba(66,204,78,0.45)]"
              type="submit"
            >
              Log out
            </button>
          </form>
        </div>
      </section>

      <div className="grid gap-4 xl:grid-cols-2">
        <section className="surface-panel px-6 py-6">
          <h2 className="surface-title">Exchange controls</h2>
          <div className="mt-5 grid gap-3 md:grid-cols-2">
            <button className="surface-panel-soft px-4 py-4 text-left text-xl font-semibold text-white hover:border-[rgba(66,204,78,0.45)]">
              Pause market
            </button>
            <button className="surface-panel-soft px-4 py-4 text-left text-xl font-semibold text-white hover:border-[rgba(66,204,78,0.45)]">
              Resume matching
            </button>
            <button className="surface-panel-soft px-4 py-4 text-left text-xl font-semibold text-white hover:border-[rgba(66,204,78,0.45)]">
              Tighten limits
            </button>
            <button className="surface-panel-soft px-4 py-4 text-left text-xl font-semibold text-white hover:border-[rgba(66,204,78,0.45)]">
              Send announcement
            </button>
          </div>
        </section>

        <section className="surface-panel px-6 py-6">
          <h2 className="surface-title">Live event state</h2>
          <div className="mt-5 grid gap-3 text-xl text-[var(--muted-strong)]">
            <div className="surface-panel-soft flex items-center justify-between px-4 py-4">
              <span>Matching engine</span>
              <span className="font-semibold text-[var(--green)]">Healthy</span>
            </div>
            <div className="surface-panel-soft flex items-center justify-between px-4 py-4">
              <span>Queue depth</span>
              <span className="font-semibold text-white">1,204 orders</span>
            </div>
            <div className="surface-panel-soft flex items-center justify-between px-4 py-4">
              <span>Active traders</span>
              <span className="font-semibold text-white">84 online</span>
            </div>
            <div className="surface-panel-soft flex items-center justify-between px-4 py-4">
              <span>Broadcast status</span>
              <span className="font-semibold text-white">Nominal</span>
            </div>
          </div>
        </section>
      </div>
    </main>
  );
}
