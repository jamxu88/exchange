"use client";

import { useState } from "react";
import { createMarketAction } from "@/app/(dashboard)/admin/actions";
import {
  COMPETITION_QUOTE_LABEL,
  deriveCompetitionMarketId,
  normalizeBaseAsset,
} from "@/app/(dashboard)/admin/market-utils";

type AdminFieldProps = {
  label: string;
  hint: string;
  children: React.ReactNode;
};

function AdminField({ label, hint, children }: AdminFieldProps) {
  return (
    <label className="grid gap-2">
      <div>
        <p className="text-sm font-semibold uppercase tracking-[0.2em] text-white">
          {label}
        </p>
        <p className="mt-1 text-sm text-[var(--muted-strong)]">{hint}</p>
      </div>
      {children}
    </label>
  );
}

export function CreateMarketForm() {
  const [baseAsset, setBaseAsset] = useState("");
  const marketId = deriveCompetitionMarketId(baseAsset);

  return (
    <form action={createMarketAction} className="mt-5 grid gap-3">
      <div className="rounded-2xl border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-4 py-4 text-sm text-[var(--muted-strong)]">
        Competition markets only need a base asset. Quote units are fixed to{" "}
        <span className="font-semibold text-white">{COMPETITION_QUOTE_LABEL}</span>,
        and the system generates the market ID automatically.
      </div>
      <div className="grid gap-3 md:grid-cols-2">
        <AdminField
          hint="The asset traders are taking exposure to. Example: BTC."
          label="Base Asset"
        >
          <input
            autoComplete="off"
            className="rounded-2xl border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-4 py-3 text-lg text-white outline-none"
            name="baseAsset"
            onChange={(event) => setBaseAsset(normalizeBaseAsset(event.target.value))}
            placeholder="BTC"
            required
            value={baseAsset}
          />
        </AdminField>
        <AdminField
          hint="Optional trader-facing label. Leave blank to default to the generated market ID."
          label="Display Name"
        >
          <input
            className="rounded-2xl border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-4 py-3 text-lg text-white outline-none"
            name="displayName"
            placeholder="Bitcoin"
          />
        </AdminField>
      </div>
      <div className="grid gap-3 md:grid-cols-2">
        <AdminField
          hint="Generated automatically from the base asset using the competition quote format."
          label="Market ID"
        >
          <input
            className="rounded-2xl border border-[var(--surface-stroke)] bg-black/20 px-4 py-3 text-lg text-white outline-none"
            placeholder="BTC-USD"
            readOnly
            tabIndex={-1}
            value={marketId}
          />
        </AdminField>
        <div className="rounded-2xl border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-4 py-4">
          <p className="text-sm font-semibold uppercase tracking-[0.2em] text-white">
            Quote Unit
          </p>
          <p className="mt-3 text-3xl font-bold text-white">{COMPETITION_QUOTE_LABEL}</p>
          <p className="mt-2 text-sm text-[var(--muted-strong)]">
            Fixed for the competition. No separate quote asset input is required.
          </p>
        </div>
      </div>
      <div className="grid gap-3 md:grid-cols-3">
        <AdminField
          hint="Smallest allowed price step. A tick size of 1 means prices move 100, 101, 102..."
          label="Tick Size"
        >
          <input
            className="rounded-2xl border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-4 py-3 text-lg text-white outline-none"
            defaultValue="1"
            min="1"
            name="tickSize"
            required
            type="number"
          />
        </AdminField>
        <AdminField
          hint="Smallest order size a trader can submit."
          label="Minimum Order Quantity"
        >
          <input
            className="rounded-2xl border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-4 py-3 text-lg text-white outline-none"
            defaultValue="1"
            min="1"
            name="minOrderQuantity"
            required
            type="number"
          />
        </AdminField>
        <AdminField
          hint="Optional fallback price used when the book is empty. It also marks PnL until live bids and asks exist."
          label="Reference Price"
        >
          <input
            className="rounded-2xl border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-4 py-3 text-lg text-white outline-none"
            min="0"
            name="referencePrice"
            placeholder="100"
            type="number"
          />
        </AdminField>
      </div>
      <div className="grid gap-2">
        <label className="flex items-center gap-3 text-lg text-[var(--muted-strong)]">
          <input defaultChecked name="enabled" type="checkbox" />
          Enable immediately
        </label>
        <p className="text-sm text-[var(--muted-strong)]">
          Enabled markets can accept orders right away.
        </p>
      </div>
      <button
        className="rounded-2xl bg-[var(--green)] px-4 py-3 text-base font-semibold text-white"
        type="submit"
      >
        Save market
      </button>
    </form>
  );
}
