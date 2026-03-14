"use client";

import { useEffect, useMemo, useState } from "react";
import { Button, Cell, Input, List, Section, Slider } from "@telegram-apps/telegram-ui";
import { useRawInitData } from "@telegram-apps/sdk-react";

import { apiFetch, MoodPoint } from "@/lib/api";

function todayISO() {
  const d = new Date();
  const yyyy = d.getFullYear();
  const mm = String(d.getMonth() + 1).padStart(2, "0");
  const dd = String(d.getDate()).padStart(2, "0");
  return `${yyyy}-${mm}-${dd}`;
}

function MoodChart({ data }: { data: MoodPoint[] }) {
  if (data.length < 2) return null;

  const W = 320;
  const H = 140;
  const PAD = { top: 16, right: 12, bottom: 24, left: 28 };
  const cw = W - PAD.left - PAD.right;
  const ch = H - PAD.top - PAD.bottom;

  const series = [
    { key: "happiness" as const, color: "#22c55e", label: "Happy" },
    { key: "energy" as const, color: "#3b82f6", label: "Energy" },
    { key: "stress" as const, color: "#ef4444", label: "Stress" },
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

  // Show ~5 date labels
  const labelStep = Math.max(1, Math.floor(data.length / 5));
  const dateLabels = data
    .filter((_, i) => i % labelStep === 0 || i === data.length - 1)
    .map((p, _, arr) => {
      const idx = data.indexOf(p);
      const x = PAD.left + (idx / (data.length - 1)) * cw;
      const label = p.date.slice(5); // MM-DD
      return { x, label };
    });

  return (
    <div>
      <svg viewBox={`0 0 ${W} ${H}`} className="w-full" style={{ maxWidth: W }}>
        {/* Y-axis labels */}
        {[1, 5, 10].map((v) => {
          const y = PAD.top + ch - ((v - 1) / 9) * ch;
          return (
            <text
              key={v}
              x={PAD.left - 6}
              y={y + 4}
              textAnchor="end"
              fontSize="10"
              fill="var(--tg-theme-hint-color, #999)"
            >
              {v}
            </text>
          );
        })}
        {/* Grid lines */}
        {[1, 5, 10].map((v) => {
          const y = PAD.top + ch - ((v - 1) / 9) * ch;
          return (
            <line
              key={v}
              x1={PAD.left}
              x2={PAD.left + cw}
              y1={y}
              y2={y}
              stroke="var(--tg-theme-hint-color, #ddd)"
              strokeOpacity={0.2}
            />
          );
        })}
        {/* Lines */}
        {series.map((s) => (
          <path key={s.key} d={toPath(s.key)} fill="none" stroke={s.color} strokeWidth={2} />
        ))}
        {/* Date labels */}
        {dateLabels.map((dl, i) => (
          <text
            key={i}
            x={dl.x}
            y={H - 4}
            textAnchor="middle"
            fontSize="9"
            fill="var(--tg-theme-hint-color, #999)"
          >
            {dl.label}
          </text>
        ))}
      </svg>
      <div className="flex justify-center gap-4 mt-1">
        {series.map((s) => (
          <div key={s.key} className="flex items-center gap-1 text-xs" style={{ color: s.color }}>
            <span
              className="inline-block w-2.5 h-0.5 rounded"
              style={{ background: s.color }}
            />
            {s.label}
          </div>
        ))}
      </div>
    </div>
  );
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
  const [history, setHistory] = useState<MoodPoint[]>([]);

  useEffect(() => {
    apiFetch<MoodPoint[]>(initData, "/v1/mood/history?days=30").then(setHistory).catch(() => {});
  }, [initData, saved]);

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
      {history.length >= 2 && (
        <Section header="Mood Trends (30 days)">
          <div className="px-2 py-3">
            <MoodChart data={history} />
          </div>
        </Section>
      )}

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
        <div className="rounded-2xl bg-[color:var(--tg-theme-secondary-bg-color,#1e1e1e)] p-3 space-y-2">
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
