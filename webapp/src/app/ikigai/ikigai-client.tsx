"use client";

import { useEffect, useState } from "react";
import { Cell, List, Placeholder, Section, Spinner } from "@telegram-apps/telegram-ui";
import { useRawInitData } from "@telegram-apps/sdk-react";

import type { DashboardData } from "@/lib/api";
import { apiFetch } from "@/lib/api";

export default function IkigaiClient() {
  const initData = useRawInitData();
  const [loading, setLoading] = useState(true);
  const [dashboard, setDashboard] = useState<DashboardData | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);

    apiFetch<DashboardData>(initData, "/v1/dashboard")
      .then((nextDashboard) => {
        if (!cancelled) setDashboard(nextDashboard);
      })
      .catch((nextError) => {
        if (!cancelled) {
          setError(nextError instanceof Error ? nextError.message : String(nextError));
        }
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
      <Placeholder header="Loading ikigai" description={<Spinner size="l" />}>
        <div />
      </Placeholder>
    );
  }

  if (error || !dashboard) {
    return (
      <Placeholder header="API Error" description={error ?? "Failed to load ikigai"}>
        <div />
      </Placeholder>
    );
  }

  const { ikigai, ikigai_svg: ikigaiSvg, goal_alignments: goalAlignments } = dashboard;

  return (
    <div className="space-y-4">
      <Section header="Purpose map">
        {ikigaiSvg ? (
          <div
            className="ikigai-frame"
            dangerouslySetInnerHTML={{ __html: ikigaiSvg }}
          />
        ) : (
          <Placeholder
            header="Ikigai is still cooking"
            description="Happi will cache a purpose map in the background once it has enough signal."
          >
            <div />
          </Placeholder>
        )}
      </Section>

      {ikigai?.mission ? (
        <Section header="Mission">
          <List>
            <Cell subtitle="Cached from your latest ikigai snapshot">{ikigai.mission}</Cell>
          </List>
        </Section>
      ) : null}

      {goalAlignments.length > 0 ? (
        <Section header="Aligned goals">
          <List>
            {goalAlignments.map((goal) => (
              <Cell
                key={goal.goal_id}
                subtitle={goal.quadrants.join(" · ")}
              >
                {goal.goal_title} · {goal.alignment_score}%
              </Cell>
            ))}
          </List>
        </Section>
      ) : null}
    </div>
  );
}
