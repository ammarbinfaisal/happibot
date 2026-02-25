"use client";

import { useEffect, useState } from "react";
import { Cell, List, Placeholder, Section, Spinner } from "@telegram-apps/telegram-ui";
import { useRawInitData } from "@telegram-apps/sdk-react";

import type { UserProfile } from "@/lib/api";
import { apiFetch } from "@/lib/api";

export default function SettingsClient() {
  const initData = useRawInitData();
  const [loading, setLoading] = useState(true);
  const [profile, setProfile] = useState<UserProfile | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setLoading(true);
    setError(null);
    apiFetch<UserProfile>(initData, "/v1/profile")
      .then(setProfile)
      .catch((e) => setError(e instanceof Error ? e.message : String(e)))
      .finally(() => setLoading(false));
  }, [initData]);

  if (loading) {
    return (
      <Placeholder header="Loading" description={<Spinner size="l" />}>
        <div />
      </Placeholder>
    );
  }

  if (error) {
    return (
      <Placeholder header="API Error" description={error}>
        <div />
      </Placeholder>
    );
  }

  return (
    <Section header="Profile">
      <List>
        <Cell subtitle="Used for reminders and check-ins">{profile?.timezone ?? "UTC"}</Cell>
        <Cell subtitle="Onboarding state">{profile?.onboarding_state ?? "new"}</Cell>
      </List>
    </Section>
  );
}

