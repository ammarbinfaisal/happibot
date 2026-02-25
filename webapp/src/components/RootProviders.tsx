"use client";

import { ReactNode } from "react";
import { AppRoot } from "@telegram-apps/telegram-ui";

import { AppShell } from "@/components/AppShell";
import { TgInit } from "@/components/TgInit";

type Props = {
  children: ReactNode;
};

export function RootProviders({ children }: Props) {
  return (
    <AppRoot>
      <TgInit />
      <AppShell>{children}</AppShell>
    </AppRoot>
  );
}

