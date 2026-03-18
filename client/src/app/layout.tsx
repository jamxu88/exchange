import type { Metadata } from "next";
import { Darker_Grotesque, Geist_Mono } from "next/font/google";
import "./globals.css";
import { KeybindProvider } from "@/components/providers/keybind-provider";

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
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en">
      <body
        className={`${darkerGrotesque.variable} ${geistMono.variable} antialiased`}
      >
        <KeybindProvider>{children}</KeybindProvider>
      </body>
    </html>
  );
}
