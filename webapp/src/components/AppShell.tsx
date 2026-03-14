"use client";

import { ReactNode } from "react";
import { usePathname, useRouter } from "next/navigation";

import { Tabbar } from "@telegram-apps/telegram-ui";
import { Icon28Stats } from "@telegram-apps/telegram-ui/dist/icons/28/stats";
import { Icon28Heart } from "@telegram-apps/telegram-ui/dist/icons/28/heart";
import { Icon28Devices } from "@telegram-apps/telegram-ui/dist/icons/28/devices";

type Props = {
  children: ReactNode;
};

export function AppShell({ children }: Props) {
  const pathname = usePathname();
  const router = useRouter();

  const is = (href: string) => pathname === href;

  return (
    <div className="app-shell">
      <main className="app-main">{children}</main>
      <Tabbar className="app-tabbar">
        <Tabbar.Item selected={is("/")} text="Progress" onClick={() => router.push("/")}>
          <Icon28Stats />
        </Tabbar.Item>
        <Tabbar.Item
          selected={is("/ikigai")}
          text="Ikigai"
          onClick={() => router.push("/ikigai")}
        >
          <Icon28Heart />
        </Tabbar.Item>
        <Tabbar.Item
          selected={is("/settings")}
          text="Settings"
          onClick={() => router.push("/settings")}
        >
          <Icon28Devices />
        </Tabbar.Item>
      </Tabbar>
    </div>
  );
}
