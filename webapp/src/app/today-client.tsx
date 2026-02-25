"use client";

import { useEffect, useMemo, useState } from "react";
import {
  Button,
  Cell,
  List,
  Placeholder,
  Section,
  Spinner,
} from "@telegram-apps/telegram-ui";
import { useRawInitData } from "@telegram-apps/sdk-react";

import type { Goal, UserProfile, WeeklyReviewStats } from "@/lib/api";
import { apiFetch } from "@/lib/api";

export default function TodayClient() {
  const initData = useRawInitData();
  const [loading, setLoading] = useState(true);
  const [profile, setProfile] = useState<UserProfile | null>(null);
  const [goals, setGoals] = useState<Goal[]>([]);
  const [week, setWeek] = useState<WeeklyReviewStats | null>(null);
  const [error, setError] = useState<string | null>(null);

  const primaryGoal = useMemo(() => goals[0] || null, [goals]);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);

    Promise.all([
      apiFetch<UserProfile>(initData, "/v1/profile"),
      apiFetch<Goal[]>(initData, "/v1/goals?status=active"),
      apiFetch<WeeklyReviewStats>(initData, "/v1/reviews/weekly"),
    ])
      .then(([p, g, w]) => {
        if (cancelled) return;
        setProfile(p);
        setGoals(g);
        setWeek(w);
      })
      .catch((e) => {
        if (cancelled) return;
        setError(e instanceof Error ? e.message : String(e));
      })
      .finally(() => {
        if (cancelled) return;
        setLoading(false);
      });

    return () => {
      cancelled = true;
    };
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
    <div className="space-y-4">
      <Section header="Today">
        <List>
          <Cell subtitle={profile ? `Timezone: ${profile.timezone}` : undefined}>
            You have {goals.length} active goals
          </Cell>
          {week ? (
            <Cell
              subtitle={`This week: ${week.mood_days} mood logs, ${week.progress_logs} progress logs`}
            >
              Weekly pulse
            </Cell>
          ) : null}
        </List>
      </Section>

      <Section header="One thing">
        {primaryGoal ? (
          <List>
            <Cell subtitle={primaryGoal.why ?? undefined}>
              {primaryGoal.title}
            </Cell>
          </List>
        ) : (
          <Placeholder
            header="No goals yet"
            description="Create one goal and keep it tiny."
          >
            <div />
          </Placeholder>
        )}
      </Section>

      <div className="sticky bottom-[calc(var(--app-tabbar-height)+var(--app-bottom-safe))] z-10">
        <div className="rounded-2xl bg-[color:var(--tg-theme-secondary-bg-color,#f4f4f5)] p-3">
          <Button
            size="l"
            stretched
            onClick={() => (window.location.href = "/checkin")}
          >
            Log mood + quick check-in
          </Button>
        </div>
      </div>
    </div>
  );
}

