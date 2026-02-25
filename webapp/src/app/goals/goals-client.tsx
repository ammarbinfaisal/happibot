"use client";

import { useEffect, useState } from "react";
import {
  Button,
  Cell,
  Input,
  List,
  Placeholder,
  Section,
  Spinner,
} from "@telegram-apps/telegram-ui";
import { useRawInitData } from "@telegram-apps/sdk-react";

import type { Goal } from "@/lib/api";
import { apiFetch } from "@/lib/api";

export default function GoalsClient() {
  const initData = useRawInitData();
  const [loading, setLoading] = useState(true);
  const [goals, setGoals] = useState<Goal[]>([]);
  const [error, setError] = useState<string | null>(null);

  const [title, setTitle] = useState("");
  const [why, setWhy] = useState("");

  const refresh = () => {
    setLoading(true);
    setError(null);
    apiFetch<Goal[]>(initData, "/v1/goals?status=active")
      .then(setGoals)
      .catch((e) => setError(e instanceof Error ? e.message : String(e)))
      .finally(() => setLoading(false));
  };

  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [initData]);

  const create = async () => {
    const t = title.trim();
    if (!t) return;
    await apiFetch<Goal>(initData, "/v1/goals", {
      method: "POST",
      body: JSON.stringify({
        title: t,
        why: why.trim() ? why.trim() : null,
        target_kind: "habit",
        cadence: "daily",
        tags: ["mvp"],
      }),
    });
    setTitle("");
    setWhy("");
    refresh();
  };

  if (loading) {
    return (
      <Placeholder header="Loading goals" description={<Spinner size="l" />}>
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
      <Section header="Create (tiny)">
        <List>
          <Cell>
            <Input
              header="Goal title"
              placeholder="e.g. Walk after iftar"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
            />
          </Cell>
          <Cell>
            <Input
              header="Why (optional)"
              placeholder="What does this unlock for you?"
              value={why}
              onChange={(e) => setWhy(e.target.value)}
            />
          </Cell>
        </List>
        <div className="mt-3">
          <Button size="l" stretched onClick={create} disabled={!title.trim()}>
            Add goal
          </Button>
        </div>
      </Section>

      <Section header="Active goals">
        {goals.length ? (
          <List>
            {goals.map((g) => (
              <Cell key={g.id} subtitle={g.why ?? undefined}>
                {g.title}
              </Cell>
            ))}
          </List>
        ) : (
          <Placeholder header="No active goals" description="Add one above.">
            <div />
          </Placeholder>
        )}
      </Section>
    </div>
  );
}

