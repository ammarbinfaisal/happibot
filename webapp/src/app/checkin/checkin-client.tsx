"use client";

import { useMemo, useState } from "react";
import { Button, Cell, Input, List, Section, Slider } from "@telegram-apps/telegram-ui";
import { useRawInitData } from "@telegram-apps/sdk-react";

import { apiFetch } from "@/lib/api";

function todayISO() {
  const d = new Date();
  const yyyy = d.getFullYear();
  const mm = String(d.getMonth() + 1).padStart(2, "0");
  const dd = String(d.getDate()).padStart(2, "0");
  return `${yyyy}-${mm}-${dd}`;
}

export default function CheckinClient() {
  const initData = useRawInitData();
  const date = useMemo(() => todayISO(), []);

  const [happiness, setHappiness] = useState(6);
  const [energy, setEnergy] = useState(6);
  const [stress, setStress] = useState(4);
  const [note, setNote] = useState("");
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState<string | null>(null);

  const save = async () => {
    setSaving(true);
    setSaved(null);
    try {
      await apiFetch(initData, "/v1/mood", {
        method: "POST",
        body: JSON.stringify({
          date,
          happiness,
          energy,
          stress,
          note: note.trim() ? note.trim() : null,
          idempotency_key: `mood:${date}`,
        }),
      });
      setSaved("Saved");
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="space-y-4">
      <Section header="Mood (1-10)">
        <List>
          <Cell subtitle={`Happiness: ${happiness}`}>
            <Slider min={1} max={10} step={1} value={happiness} onChange={setHappiness} />
          </Cell>
          <Cell subtitle={`Energy: ${energy}`}>
            <Slider min={1} max={10} step={1} value={energy} onChange={setEnergy} />
          </Cell>
          <Cell subtitle={`Stress: ${stress}`}>
            <Slider min={1} max={10} step={1} value={stress} onChange={setStress} />
          </Cell>
          <Cell>
            <Input
              header="One sentence (optional)"
              placeholder="What happened today?"
              value={note}
              onChange={(e) => setNote(e.target.value)}
            />
          </Cell>
        </List>
      </Section>

      <div className="sticky bottom-[calc(var(--app-tabbar-height)+var(--app-bottom-safe))] z-10">
        <div className="rounded-2xl bg-[color:var(--tg-theme-secondary-bg-color,#f4f4f5)] p-3 space-y-2">
          <Button size="l" stretched onClick={save} loading={saving}>
            Save check-in
          </Button>
          {saved ? (
            <div className="text-sm text-[color:var(--tg-theme-hint-color,#6b7280)]">{saved}</div>
          ) : null}
        </div>
      </div>
    </div>
  );
}

