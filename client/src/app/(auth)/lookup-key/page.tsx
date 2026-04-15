import { LookupKeyForm } from "./lookup-key-form";

export default function LookupKeyPage() {
  return (
    <main className="mx-auto flex min-h-screen w-full max-w-xl flex-col justify-center px-6 py-8">
      <section className="surface-panel overflow-hidden px-6 py-6 lg:px-8 lg:py-8">
        <LookupKeyForm />
      </section>
    </main>
  );
}
