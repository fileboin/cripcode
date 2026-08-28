import type { Metadata } from "next";

export const metadata: Metadata = {
  title: "AI Agent",
  description: "An AI agent",
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
