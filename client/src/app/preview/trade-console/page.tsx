import { notFound } from "next/navigation";
import { TradeConsolePreview } from "@/components/trade/trade-console-preview";

export default function TradeConsolePreviewPage() {
  if (process.env.NODE_ENV === "production") {
    notFound();
  }

  return (
    <main className="h-screen overflow-hidden bg-black">
      <TradeConsolePreview />
    </main>
  );
}
