"use client";

import dynamic from "next/dynamic";

const IkigaiClient = dynamic(() => import("./ikigai-client"), { ssr: false });

export default function IkigaiPage() {
  return <IkigaiClient />;
}
