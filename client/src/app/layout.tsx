import type { Metadata } from "next";
import Script from "next/script";
import "./globals.css";
import { KeybindProvider } from "@/components/providers/keybind-provider";
import { APP_THEME_INIT_SCRIPT } from "@/lib/app-theme";

export const metadata: Metadata = {
  title: "Quant Exchange Client",
  description: "Trading and event operations client for the exchange",
  icons: {
    icon: "/favicon.ico",
    shortcut: "/favicon.ico",
  },
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en" suppressHydrationWarning>
      <head>
        <link rel="preconnect" href="https://fonts.googleapis.com" />
        <link rel="preconnect" href="https://fonts.gstatic.com" crossOrigin="" />
        <link
          href="https://fonts.googleapis.com/css2?family=Darker+Grotesque:wght@400;500;600;700;800&display=swap"
          rel="stylesheet"
        />
        <link
          href="https://fonts.googleapis.com/css2?family=Geist+Mono:wght@100..900&display=swap"
          rel="stylesheet"
        />
      </head>
      <body className="relative antialiased">
        <Script
          id="exchange-app-theme"
          strategy="beforeInteractive"
        >
          {APP_THEME_INIT_SCRIPT}
        </Script>

        <KeybindProvider>
          {children}
        </KeybindProvider>
      </body>
    </html>
  );
}
