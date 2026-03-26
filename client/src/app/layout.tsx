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
        className={`${darkerGrotesque.variable} ${geistMono.variable} relative antialiased`}
      >
        <KeybindProvider>
          {children}
          <div className="pointer-events-none fixed inset-x-0 bottom-2 z-50 flex justify-center px-4">
            <a
              className="pointer-events-auto text-[10px] font-medium tracking-[0.08em] text-[rgba(183,183,189,0.7)] hover:text-white"
              href="https://github.com/jamxu88/exchange"
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
