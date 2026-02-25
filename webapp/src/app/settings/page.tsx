"use client";

import dynamic from "next/dynamic";

const SettingsClient = dynamic(() => import("./settings-client"), { ssr: false });

export default function SettingsPage() {
  return <SettingsClient />;
}
