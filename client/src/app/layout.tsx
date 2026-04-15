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
        </KeybindProvider>
      </body>
    </html>
  );
}
