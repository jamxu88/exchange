import type { Metadata } from "next";
import { Darker_Grotesque, Geist_Mono } from "next/font/google";
import Script from "next/script";
import "./globals.css";
import { KeybindProvider } from "@/components/providers/keybind-provider";
import { APP_THEME_INIT_SCRIPT } from "@/lib/app-theme";

const darkerGrotesque = Darker_Grotesque({
  variable: "--font-darker-grotesque",
  subsets: ["latin"],
  weight: ["400", "500", "600", "700", "800"],
});

const geistMono = Geist_Mono({
  variable: "--font-geist-mono",
  subsets: ["latin"],
});

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
      <body
        className={`${darkerGrotesque.variable} ${geistMono.variable} relative antialiased`}
      >
        <Script
          id="exchange-app-theme"
          strategy="beforeInteractive"
        >
          {APP_THEME_INIT_SCRIPT}
        </Script>

        <KeybindProvider>
          {children}
          <div className="pointer-events-none fixed inset-x-0 bottom-2 z-50 flex justify-center px-4">
            <a
              className="pointer-events-auto text-[10px] font-medium tracking-[0.08em] text-[var(--muted)] hover:text-[var(--foreground)] motion-fade-up motion-fade-up-fast motion-delay-4"
              href="https://jamesxu.dev"
              rel="noreferrer"
              target="_blank"
            >
              Made with ❤️ by James
            </a>
          </div>
        </KeybindProvider>
      </body>
    </html>
  );
}
