"use client";

import dynamic from "next/dynamic";

const CheckinClient = dynamic(() => import("./checkin-client"), { ssr: false });

export default function CheckinPage() {
  return <CheckinClient />;
}
