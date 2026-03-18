import Link from "next/link";

export default function Home() {
  return (
    <main className="mx-auto flex min-h-screen w-full max-w-7xl flex-col gap-8 px-6 py-8 lg:px-10">
      <section className="surface-panel overflow-hidden px-6 py-7 lg:px-8 lg:py-8">
        <div className="flex flex-col gap-8 lg:flex-row lg:items-end lg:justify-between">
          <div className="max-w-3xl">
            <p className="mb-3 text-sm font-semibold uppercase tracking-[0.28em] text-[var(--muted)]">
              Quant Exchange
            </p>
            <h1 className="max-w-2xl text-5xl font-extrabold leading-none text-white sm:text-6xl">
              Figma-driven trading client, not a starter shell.
            </h1>
            <p className="mt-4 max-w-2xl text-2xl leading-tight text-[var(--muted-strong)]">
              The client now centers on the trading console layout from the
              design source of truth, while keeping the ECS-ready Next.js app
              structure and route scaffolding intact.
            </p>
          </div>
          <div className="surface-panel-soft grid gap-4 p-5 text-lg text-[var(--muted-strong)] sm:grid-cols-3 lg:w-[30rem]">
            <div>
              <p className="text-3xl font-bold text-white">3</p>
              <p>Primary routes</p>
            </div>
            <div>
              <p className="text-3xl font-bold text-white">2</p>
              <p>Global keybinds</p>
            </div>
            <div>
              <p className="text-3xl font-bold text-white">1</p>
              <p>Trading shell target</p>
            </div>
          </div>
        </div>
      </section>

      <section className="grid gap-4 lg:grid-cols-3">
        <Link className="card-link flex flex-col gap-3" href="/trade">
          <span className="text-sm uppercase tracking-[0.2em] text-[var(--muted)]">
            Trade
          </span>
          <span className="text-3xl">Open the Figma-based market console</span>
          <span className="text-xl font-medium text-[var(--muted-strong)]">
            Positions, depth, ticket, PnL, and live messages.
          </span>
        </Link>
        <Link className="card-link flex flex-col gap-3" href="/admin">
          <span className="text-sm uppercase tracking-[0.2em] text-[var(--muted)]">
            Admin
          </span>
          <span className="text-3xl">Run event operations from one surface</span>
          <span className="text-xl font-medium text-[var(--muted-strong)]">
            Market state, risk controls, and operator workflows.
          </span>
        </Link>
        <Link className="card-link flex flex-col gap-3" href="/login">
          <span className="text-sm uppercase tracking-[0.2em] text-[var(--muted)]">
            Access
          </span>
          <span className="text-3xl">Sign in with your assigned API key</span>
          <span className="text-xl font-medium text-[var(--muted-strong)]">
            Competition users and admins authenticate with event-issued keys.
          </span>
        </Link>
      </section>

      <section className="surface-panel grid gap-6 px-6 py-6 lg:grid-cols-[1.15fr_0.85fr] lg:px-8">
        <div>
          <h2 className="surface-title">Current build focus</h2>
          <p className="mt-3 text-2xl text-[var(--muted-strong)]">
            The implemented work centers on the UX/UI workstream from
            `CLIENT_TODO.md`: replacing the template trading route with the
            Figma layout and moving the rest of the client onto the same visual
            system.
          </p>
        </div>
        <div>
          <h2 className="surface-title">Keybinds</h2>
          <ul className="mt-3 space-y-2 text-xl text-[var(--muted-strong)]">
            <li>
              <span className="font-semibold text-white">Ctrl/Cmd + K</span>:
              jump to trader console
            </li>
            <li>
              <span className="font-semibold text-white">Ctrl/Cmd + G</span>:
              jump to admin panel
            </li>
          </ul>
        </div>
      </section>
    </main>
  );
}
