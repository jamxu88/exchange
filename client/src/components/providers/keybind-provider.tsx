"use client";

import { useEffect } from "react";
import { useRouter } from "next/navigation";

export function KeybindProvider({ children }: { children: React.ReactNode }) {
  const router = useRouter();

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey)) {
        return;
      }

      const key = event.key.toLowerCase();
      if (key === "k") {
        event.preventDefault();
        router.push("/trade");
      }

      if (key === "g") {
        event.preventDefault();
        router.push("/admin");
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [router]);

  return <>{children}</>;
}
