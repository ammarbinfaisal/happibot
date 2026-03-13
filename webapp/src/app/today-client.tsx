"use client";

import { useEffect, useState } from "react";
import {
  Button,
  Cell,
  List,
  Placeholder,
  Section,
  Spinner,
} from "@telegram-apps/telegram-ui";
import { useRawInitData } from "@telegram-apps/sdk-react";

import type { DashboardData } from "@/lib/api";
import { apiFetch } from "@/lib/api";
import { ProgressSparkline } from "@/components/ProgressSparkline";
import { IkigaiDiagram } from "@/components/IkigaiDiagram";

function MiniMoodChart({ data }: { data: { date: string; happiness: number; energy: number; stress: number }[] }) {
  if (data.length < 2) return null;

  const W = 280;
  const H = 80;
  const PAD = { top: 8, right: 8, bottom: 16, left: 24 };
  const cw = W - PAD.left - PAD.right;
  const ch = H - PAD.top - PAD.bottom;

  const series = [
    { key: "happiness" as const, color: "#22c55e" },
    { key: "energy" as const, color: "#3b82f6" },
    { key: "stress" as const, color: "#ef4444" },
  ];

  const toPath = (key: "happiness" | "energy" | "stress") => {
    return data
      .map((p, i) => {
        const x = PAD.left + (i / (data.length - 1)) * cw;
        const y = PAD.top + ch - ((p[key] - 1) / 9) * ch;
        return `${i === 0 ? "M" : "L"}${x.toFixed(1)},${y.toFixed(1)}`;
      })
      .join("");
  };

  return (
    <svg viewBox={`0 0 ${W} ${H}`} className="w-full" style={{ maxWidth: W }}>
      {series.map((s) => (
        <path key={s.key} d={toPath(s.key)} fill="none" stroke={s.color} strokeWidth={1.5} strokeOpacity={0.7} />
      ))}
    </svg>
  );
}

export default function TodayClient() {
  const initData = useRawInitData();
  const [loading, setLoading] = useState(true);
  const [dashboard, setDashboard] = useState<DashboardData | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);

    apiFetch<DashboardData>(initData, "/v1/dashboard")
      .then((d) => {
        if (!cancelled) setDashboard(d);
      })
      .catch((e) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
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

  if (error || !dashboard) {
    return (
      <Placeholder header="API Error" description={error ?? "Failed to load"}>
        <div />
      </Placeholder>
    );
  }

  const { goals, mood_trend, weekly_stats, ikigai, goal_alignments, streak } = dashboard;

  const hasStreak = streak.current_mood_streak > 0 || streak.current_progress_streak > 0;

  return (
    <div className="space-y-4">
      {/* Streaks */}
      {hasStreak && (
        <Section header="Streaks">
          <List>
            {streak.current_mood_streak > 0 && (
              <Cell
                before={<span className="text-lg">🔥</span>}
                subtitle="consecutive days"
              >
                {streak.current_mood_streak}-day mood streak
              </Cell>
            )}
            {streak.current_progress_streak > 0 && (
              <Cell
                before={<span className="text-lg">⚡</span>}
                subtitle="consecutive days"
              >
                {streak.current_progress_streak}-day progress streak
              </Cell>
            )}
          </List>
        </Section>
      )}

      {/* Weekly pulse */}
      <Section header="This week">
        <List>
          <Cell subtitle={`${weekly_stats.active_goals} active goals`}>
            {weekly_stats.mood_days} mood logs · {weekly_stats.progress_logs} progress logs
          </Cell>
        </List>
        {mood_trend.length >= 2 && (
          <div className="px-3 py-2">
            <MiniMoodChart data={mood_trend.slice(-7)} />
            <div className="flex justify-center gap-3 mt-1">
              {[
                { label: "Happy", color: "#22c55e" },
                { label: "Energy", color: "#3b82f6" },
                { label: "Stress", color: "#ef4444" },
              ].map((s) => (
                <div key={s.label} className="flex items-center gap-1 text-xs" style={{ color: s.color }}>
                  <span className="inline-block w-2 h-0.5 rounded" style={{ background: s.color }} />
                  {s.label}
                </div>
              ))}
            </div>
          </div>
        )}
      </Section>

      {/* Goal progress cards */}
      <Section header="Goals">
        {goals.length > 0 ? (
          <List>
            {goals.map((g) => (
              <Cell
                key={g.id}
                subtitle={
                  <div className="space-y-1">
                    {g.why && (
                      <div className="text-xs" style={{ color: "var(--tg-theme-hint-color, #888)" }}>
                        {g.why}
                      </div>
                    )}
                    <div className="flex items-center gap-2">
                      {g.progress_last_7d.length >= 2 && (
                        <ProgressSparkline
                          data={g.progress_last_7d}
                          width={100}
                          height={24}
                        />
                      )}
                      <span className="text-xs" style={{ color: "var(--tg-theme-hint-color, #888)" }}>
                        {g.total_logs} logs
                      </span>
                    </div>
                    {g.completion_pct != null && (
                      <div className="w-full rounded-full h-1.5" style={{ background: "var(--tg-theme-secondary-bg-color, #e5e7eb)" }}>
                        <div
                          className="h-1.5 rounded-full"
                          style={{
                            width: `${Math.min(g.completion_pct, 100)}%`,
                            background: g.completion_pct >= 100 ? "#22c55e" : "var(--tg-theme-button-color, #3b82f6)",
                          }}
                        />
                      </div>
                    )}
                  </div>
                }
              >
                {g.title}
              </Cell>
            ))}
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

      {/* Ikigai alignment */}
      {(goal_alignments.length > 0 || ikigai) && (
        <Section header="Ikigai alignment">
          {ikigai?.mission && (
            <List>
              <Cell subtitle="Your mission">
                {ikigai.mission}
              </Cell>
            </List>
          )}
          {goal_alignments.length > 0 && (
            <div className="px-3 py-2 flex justify-center">
              <IkigaiDiagram goals={goal_alignments} size={240} />
            </div>
          )}
          {ikigai?.themes && ikigai.themes.length > 0 && (
            <div className="px-3 pb-2 flex flex-wrap gap-1">
              {ikigai.themes.map((t) => (
                <span
                  key={t}
                  className="text-xs px-2 py-0.5 rounded-full"
                  style={{
                    background: "var(--tg-theme-secondary-bg-color, #f3f4f6)",
                    color: "var(--tg-theme-hint-color, #666)",
                  }}
                >
                  {t}
                </span>
              ))}
            </div>
          )}
        </Section>
      )}

      {/* CTA */}
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
