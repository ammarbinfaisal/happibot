"use client";

type Props = {
  data: { date: string; value: number | null }[];
  width?: number;
  height?: number;
  color?: string;
};

export function ProgressSparkline({
  data,
  width = 120,
  height = 32,
  color = "#22c55e",
}: Props) {
  const points = data.filter((d) => d.value != null) as {
    date: string;
    value: number;
  }[];

  if (points.length < 2) return null;

  const PAD = 4;
  const cw = width - PAD * 2;
  const ch = height - PAD * 2;

  const min = Math.min(...points.map((p) => p.value));
  const max = Math.max(...points.map((p) => p.value));
  const range = max - min || 1;

  const pathD = points
    .map((p, i) => {
      const x = PAD + (i / (points.length - 1)) * cw;
      const y = PAD + ch - ((p.value - min) / range) * ch;
      return `${i === 0 ? "M" : "L"}${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join("");

  const last = points[points.length - 1];
  const lastX = PAD + cw;
  const lastY = PAD + ch - ((last.value - min) / range) * ch;

  return (
    <svg viewBox={`0 0 ${width} ${height}`} style={{ width, height }}>
      <path d={pathD} fill="none" stroke={color} strokeWidth={1.5} />
      <circle cx={lastX} cy={lastY} r={2.5} fill={color} />
    </svg>
  );
}
