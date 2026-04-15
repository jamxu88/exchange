"use client";

import Link from "next/link";
import type { FormEvent } from "react";
import { useState } from "react";

type LookupResult = {
  apiKey: string;
};

function isLookupResult(payload: unknown): payload is LookupResult {
  return Boolean(
    payload &&
      typeof payload === "object" &&
      "apiKey" in payload &&
      typeof payload.apiKey === "string",
  );
}

export function LookupKeyForm() {
  const [identifier, setIdentifier] = useState("");
  const [result, setResult] = useState<LookupResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();

    setIsSubmitting(true);
    setError(null);
    setResult(null);

    try {
      const response = await fetch("/api/auth/lookup-key", {
        method: "POST",
        cache: "no-store",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({ identifier }),
      });

      const payload = (await response.json().catch(() => null)) as
        | LookupResult
        | { error?: string }
        | null;

      if (!response.ok) {
        const message =
          payload && typeof payload === "object" && "error" in payload && payload.error
            ? payload.error
            : "Unable to find an API key for that identifier.";
        setError(message);
        return;
      }

      if (!isLookupResult(payload)) {
        setError("Lookup succeeded but returned an unexpected response.");
        return;
      }

      setResult({
        apiKey: payload.apiKey,
      });
    } catch {
      setError("The lookup service could not be reached. Try again shortly.");
    } finally {
      setIsSubmitting(false);
    }
  }

  return (
    <form className="mx-auto flex w-full max-w-md flex-col gap-5" onSubmit={handleSubmit}>
      <div className="text-center">
        <p className="text-sm font-semibold uppercase tracking-[0.28em] text-[var(--muted)]">
          Key Lookup
        </p>
      </div>

      <label className="block">
        <span className="mb-2 block text-sm font-semibold uppercase tracking-[0.22em] text-[var(--muted)]">
          Identifier
        </span>
        <input
          autoCapitalize="characters"
          autoComplete="off"
          className="block w-full rounded-2xl border border-[var(--surface-stroke)] bg-[var(--surface-soft)] px-4 py-3 text-xl uppercase text-[var(--text-primary)] outline-none"
          onChange={(event) => setIdentifier(event.target.value.toUpperCase())}
          placeholder="identifier"
          spellCheck={false}
          type="text"
          value={identifier}
        />
      </label>

      {error ? (
        <p className="rounded-2xl border border-[rgba(216,91,91,0.42)] bg-[rgba(216,91,91,0.1)] px-4 py-3 text-lg text-[color:var(--red-strong)]">
          {error}
        </p>
      ) : null}

      {result ? (
        <div className="rounded-2xl border border-[rgba(66,204,78,0.34)] bg-[rgba(66,204,78,0.1)] px-4 py-4 text-left">
          <p className="text-sm font-semibold uppercase tracking-[0.22em] text-[var(--green-strong)]">
            API key revealed
          </p>
          <p className="mt-3 break-all font-mono text-lg text-[var(--text-primary)]">
            {result.apiKey}
          </p>
        </div>
      ) : null}

      <button
        className="w-full rounded-2xl bg-[var(--green)] px-4 py-3 text-xl font-bold text-[var(--background)] shadow-[0_0_24px_rgba(66,204,78,0.25)] hover:translate-y-[-1px] hover:brightness-[1.04] disabled:cursor-not-allowed disabled:opacity-70"
        disabled={isSubmitting}
        type="submit"
      >
        {isSubmitting ? "Looking up..." : "Reveal API key"}
      </button>

      <p className="text-center text-base text-[var(--muted-strong)]">
        <Link className="underline decoration-[var(--surface-stroke)] underline-offset-4" href="/login">
          Back to login
        </Link>
      </p>
    </form>
  );
}
