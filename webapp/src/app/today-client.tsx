"use client";

import { startTransition, useEffect, useState } from "react";
import { Button, Cell, List, Placeholder, Section, Spinner } from "@telegram-apps/telegram-ui";
import { useRawInitData } from "@telegram-apps/sdk-react";

import type { DashboardData, GoalWithProgress } from "@/lib/api";
import { apiFetch } from "@/lib/api";
import { ProgressSparkline } from "@/components/ProgressSparkline";

function todayISO() {
  const now = new Date();
  const year = now.getFullYear();
  const month = String(now.getMonth() + 1).padStart(2, "0");
  const day = String(now.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

type DraftMap = Record<string, string>;

function defaultValue(goal: GoalWithProgress) {
  return goal.latest_value == null ? "" : String(goal.latest_value);
}

export default function TodayClient() {
  const initData = useRawInitData();
  const [loading, setLoading] = useState(true);
  const [dashboard, setDashboard] = useState<DashboardData | null>(null);
  const [pageError, setPageError] = useState<string | null>(null);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [noteDrafts, setNoteDrafts] = useState<DraftMap>({});
  const [valueDrafts, setValueDrafts] = useState<DraftMap>({});
  const [savingGoalId, setSavingGoalId] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    async function loadDashboard() {
      setLoading(true);
      setPageError(null);

      try {
        const nextDashboard = await apiFetch<DashboardData>(initData, "/v1/dashboard");
        if (cancelled) return;

        setDashboard(nextDashboard);
        setNoteDrafts((current) => {
          const next = { ...current };
          for (const goal of nextDashboard.goals) {
            next[goal.id] = next[goal.id] ?? "";
          }
          return next;
        });
        setValueDrafts((current) => {
          const next = { ...current };
          for (const goal of nextDashboard.goals) {
            next[goal.id] = next[goal.id] ?? defaultValue(goal);
          }
          return next;
        });
      } catch (nextError) {
        if (!cancelled) {
          setPageError(nextError instanceof Error ? nextError.message : String(nextError));
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    }

    loadDashboard();

    return () => {
      cancelled = true;
    };
  }, [initData]);

  async function refreshDashboard() {
    const nextDashboard = await apiFetch<DashboardData>(initData, "/v1/dashboard");
    startTransition(() => {
      setDashboard(nextDashboard);
      setValueDrafts((current) => {
        const next = { ...current };
        for (const goal of nextDashboard.goals) {
          next[goal.id] = defaultValue(goal);
        }
        return next;
      });
    });
  }

  async function submitProgress(goal: GoalWithProgress) {
    const rawNote = noteDrafts[goal.id]?.trim() ?? "";
    const rawValue = valueDrafts[goal.id]?.trim() ?? "";
    const parsedValue = rawValue ? Number(rawValue) : null;

    if (!rawNote && !rawValue) return;
    if (rawValue && Number.isNaN(parsedValue)) {
      setSubmitError(`Progress value for "${goal.title}" must be a number.`);
      return;
    }

    setSubmitError(null);
    setSavingGoalId(goal.id);

    try {
      await apiFetch<string>(initData, "/v1/progress", {
        method: "POST",
        body: JSON.stringify({
          goal_id: goal.id,
          date: todayISO(),
          value: parsedValue,
          note: rawNote || null,
          confidence: 4,
          idempotency_key: `${goal.id}:${todayISO()}:${rawValue}:${rawNote}`,
        }),
      });

      setNoteDrafts((current) => ({ ...current, [goal.id]: "" }));
      await refreshDashboard();
    } catch (nextError) {
      setSubmitError(nextError instanceof Error ? nextError.message : String(nextError));
    } finally {
      setSavingGoalId(null);
    }
  }

  if (loading) {
    return (
      <Placeholder header="Loading progress" description={<Spinner size="l" />}>
        <div />
      </Placeholder>
    );
  }

  if (pageError || !dashboard) {
    return (
      <Placeholder header="API Error" description={pageError ?? "Failed to load progress"}>
        <div />
      </Placeholder>
    );
  }

  return (
    <div className="space-y-4">
      <Section header="This week">
        <List>
          <Cell subtitle={`${dashboard.weekly_stats.active_goals} active goals`}>
            {dashboard.weekly_stats.progress_logs} progress logs this week
          </Cell>
          <Cell subtitle="Current streak">
            {dashboard.streak.current_progress_streak} consecutive days with progress
          </Cell>
        </List>
      </Section>

      <Section header="Log progress">
        {submitError ? <p className="settings-error">{submitError}</p> : null}
        {dashboard.goals.length > 0 ? (
          <div className="space-y-3">
            {dashboard.goals.map((goal) => (
              <article key={goal.id} className="progress-card">
                <div className="flex items-start justify-between gap-3">
                  <div>
                    <h3 className="progress-card-title">{goal.title}</h3>
                    {goal.why ? <p className="progress-card-copy">{goal.why}</p> : null}
                  </div>
                  {goal.completion_pct != null ? (
                    <div className="progress-chip">{Math.round(goal.completion_pct)}%</div>
                  ) : null}
                </div>

                <div className="grid grid-cols-1 gap-3 sm:grid-cols-[1fr_104px]">
                  <label className="progress-field">
                    <span className="progress-field-label">Update</span>
                    <textarea
                      className="progress-textarea"
                      rows={3}
                      placeholder="What moved forward?"
                      value={noteDrafts[goal.id] ?? ""}
                      onChange={(event) =>
                        setNoteDrafts((current) => ({
                          ...current,
                          [goal.id]: event.target.value,
                        }))
                      }
                    />
                  </label>

                  <label className="progress-field">
                    <span className="progress-field-label">
                      {goal.metric || goal.target_text || "Value"}
                    </span>
                    <input
                      className="progress-input"
                      inputMode="decimal"
                      placeholder="Optional"
                      value={valueDrafts[goal.id] ?? ""}
                      onChange={(event) =>
                        setValueDrafts((current) => ({
                          ...current,
                          [goal.id]: event.target.value,
                        }))
                      }
                    />
                  </label>
                </div>

                <div className="flex items-center justify-between gap-3">
                  <div className="flex items-center gap-3">
                    {goal.progress_last_7d.length >= 2 ? (
                      <ProgressSparkline data={goal.progress_last_7d} width={118} height={26} />
                    ) : null}
                    <span className="progress-card-meta">{goal.total_logs} total logs</span>
                  </div>

                  <Button
                    size="m"
                    onClick={() => submitProgress(goal)}
                    disabled={savingGoalId === goal.id}
                  >
                    {savingGoalId === goal.id ? "Saving..." : "Save"}
                  </Button>
                </div>
              </article>
            ))}
          </div>
        ) : (
          <Placeholder
            header="No active goals yet"
            description="Create goals in chat with Happi, then use this screen to log progress."
          >
            <div />
          </Placeholder>
        )}
      </Section>
    </div>
  );
}
