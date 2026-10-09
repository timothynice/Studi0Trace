import type { TextPlan } from "@/platform/types";

/** The hovered line's box on the canvas, in the vector's coordinates: dashed in the accent, as the inspector marks a shape. */
export function TextOverlay({ plan, hovered, width, height, scale }: { plan: TextPlan; hovered: string | null; width: number; height: number; scale: number }) {
  const line = hovered ? plan.lines.find((l) => l.id === hovered) : undefined;
  if (!line) return null;
  const w = 1 / Math.max(scale, 0.01);
  const points = line.quad.map(([x, y]) => `${x},${y}`).join(" ");
  return (
    <svg viewBox={`0 0 ${width} ${height}`} width={width} height={height} className="pointer-events-none absolute left-0 top-0 overflow-visible" aria-hidden="true">
      <polygon points={points} fill="hsl(var(--brand-accent) / 0.08)" stroke="hsl(var(--brand-accent))" strokeWidth={w} strokeDasharray={`${4 * w} ${3 * w}`} />
    </svg>
  );
}
