"use client";

import dynamic from "next/dynamic";

const GoalsClient = dynamic(() => import("./goals-client"), { ssr: false });

export default function GoalsPage() {
  return <GoalsClient />;
}
