"use client";

import dynamic from "next/dynamic";

const TodayClient = dynamic(() => import("./today-client"), { ssr: false });

export default function TodayPage() {
  return <TodayClient />;
}
