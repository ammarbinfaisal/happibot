"use client";

import type { GoalAlignmentEntry } from "@/lib/api";

type Props = {
  goals: GoalAlignmentEntry[];
  size?: number;
  onGoalClick?: (goalId: string) => void;
};

// Classic 4-circle ikigai Venn diagram
//   Love (top-left)      Good At (top-right)
//   World Needs (bot-left)  Paid For (bot-right)

const QUADRANTS = {
  passion: { label: "Passion", cx: -0.3, cy: -0.3, color: "#f472b6" },  // love + good at
  mission: { label: "Mission", cx: -0.3, cy: 0.3, color: "#60a5fa" },   // love + world needs
  profession: { label: "Profession", cx: 0.3, cy: -0.3, color: "#a78bfa" }, // good at + paid for
  vocation: { label: "Vocation", cx: 0.3, cy: 0.3, color: "#34d399" },  // world needs + paid for
} as const;

const CIRCLES = [
  { label: "Love", cx: -0.22, cy: -0.22, color: "#f472b6" },
  { label: "Good at", cx: 0.22, cy: -0.22, color: "#a78bfa" },
  { label: "World needs", cx: -0.22, cy: 0.22, color: "#60a5fa" },
  { label: "Paid for", cx: 0.22, cy: 0.22, color: "#34d399" },
];

export function IkigaiDiagram({ goals, size = 260, onGoalClick }: Props) {
  const half = size / 2;
  const r = size * 0.32;

  function goalPosition(entry: GoalAlignmentEntry): { x: number; y: number } {
    if (entry.quadrants.length === 0) return { x: half, y: half };

    let sumX = 0;
    let sumY = 0;
    for (const q of entry.quadrants) {
      const quad = QUADRANTS[q as keyof typeof QUADRANTS];
      if (quad) {
        sumX += quad.cx;
        sumY += quad.cy;
      }
    }
    const count = entry.quadrants.length;
    return {
      x: half + (sumX / count) * size * 0.35,
      y: half + (sumY / count) * size * 0.35,
    };
  }

  return (
    <div className="flex flex-col items-center">
      <svg viewBox={`0 0 ${size} ${size}`} style={{ width: size, height: size }}>
        {/* Background circles */}
        {CIRCLES.map((c) => (
          <circle
            key={c.label}
            cx={half + c.cx * size * 0.28}
            cy={half + c.cy * size * 0.28}
            r={r}
            fill={c.color}
            fillOpacity={0.12}
            stroke={c.color}
            strokeOpacity={0.3}
            strokeWidth={1}
          />
        ))}

        {/* Circle labels */}
        {CIRCLES.map((c) => (
          <text
            key={`label-${c.label}`}
            x={half + c.cx * size * 0.52}
            y={half + c.cy * size * 0.52}
            textAnchor="middle"
            dominantBaseline="central"
            fontSize="10"
            fill="var(--tg-theme-hint-color, #888)"
            fontWeight="500"
          >
            {c.label}
          </text>
        ))}

        {/* Center label */}
        <text
          x={half}
          y={half}
          textAnchor="middle"
          dominantBaseline="central"
          fontSize="11"
          fontWeight="600"
          fill="var(--tg-theme-text-color, #333)"
        >
          ikigai
        </text>

        {/* Goal dots */}
        {goals.map((g) => {
          const pos = goalPosition(g);
          const dotR = 4 + (g.alignment_score / 100) * 4;
          const opacity = 0.4 + (g.alignment_score / 100) * 0.6;
          return (
            <g
              key={g.goal_id}
              onClick={() => onGoalClick?.(g.goal_id)}
              style={{ cursor: onGoalClick ? "pointer" : undefined }}
            >
              <circle
                cx={pos.x}
                cy={pos.y}
                r={dotR}
                fill="var(--tg-theme-button-color, #3b82f6)"
                fillOpacity={opacity}
                stroke="var(--tg-theme-button-color, #3b82f6)"
                strokeWidth={1}
              />
              <title>{g.goal_title} ({g.alignment_score}%)</title>
            </g>
          );
        })}
      </svg>

      {/* Legend */}
      {goals.length > 0 && (
        <div className="flex flex-wrap justify-center gap-2 mt-2 px-2">
          {goals.map((g) => (
            <div
              key={g.goal_id}
              className="flex items-center gap-1 text-xs"
              style={{ color: "var(--tg-theme-hint-color, #888)" }}
            >
              <span
                className="inline-block w-2 h-2 rounded-full"
                style={{
                  background: "var(--tg-theme-button-color, #3b82f6)",
                  opacity: 0.4 + (g.alignment_score / 100) * 0.6,
                }}
              />
              {g.goal_title}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
