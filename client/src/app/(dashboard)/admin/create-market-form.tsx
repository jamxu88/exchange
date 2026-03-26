"use client";

import { useState } from "react";
import { createMarketAction } from "@/app/(dashboard)/admin/actions";
import { deriveCompetitionMarketId } from "@/app/(dashboard)/admin/market-utils";

type AdminFieldProps = {
  label: string;
  children: React.ReactNode;
};

function AdminField({ label, children }: AdminFieldProps) {
  return (
    <label className="grid gap-2">
      <p className="ops-kicker text-white">{label}</p>
      {children}
    </label>
  );
}

export function CreateMarketForm() {
  const [displayName, setDisplayName] = useState("");
  const marketId = deriveCompetitionMarketId(displayName);

  return (
    <form action={createMarketAction} className="mt-4 grid gap-3">
      <div className="grid gap-3">
        <AdminField label="Display Name">
          <input
            className="ops-input"
            name="displayName"
            onChange={(event) => setDisplayName(event.target.value)}
            placeholder="Bitcoin"
            required
            value={displayName}
          />
        </AdminField>
      </div>
      <div className="grid gap-3">
        <AdminField label="Market ID">
          <input
            className="ops-input bg-black/30"
            placeholder="BITCOIN-MARKET"
            readOnly
            tabIndex={-1}
            value={marketId}
          />
        </AdminField>
      </div>
      <div className="grid gap-3 md:grid-cols-3">
        <AdminField label="Tick Size">
          <input
            className="ops-input"
            defaultValue="1"
            min="1"
            name="tickSize"
            required
            type="number"
          />
        </AdminField>
        <AdminField label="Minimum Order Quantity">
          <input
            className="ops-input"
            defaultValue="1"
            min="1"
            name="minOrderQuantity"
            required
            type="number"
          />
        </AdminField>
        <AdminField label="Reference Price">
          <input
            className="ops-input"
            min="0"
            name="referencePrice"
            placeholder="100"
            type="number"
          />
        </AdminField>
      </div>
      <div className="grid gap-2">
        <label className="flex items-center gap-3 text-sm text-[var(--muted-strong)]">
          <input className="ops-check" defaultChecked name="enabled" type="checkbox" />
          Enable immediately
        </label>
        <p className="text-sm text-[var(--muted-strong)]">
          Enabled markets can accept orders right away.
        </p>
      </div>
      <button
        className="ops-button ops-button-primary"
        type="submit"
      >
        Save market
      </button>
    </form>
  );
}
