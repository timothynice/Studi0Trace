/** Text re-setting's words: the phases, the reasons a line is traced as shapes, the failures. */
import type { DeclineReason, FontRef, TextLine, TextPhase, TextPlan } from "@/platform/types";
import { ERROR_WORDS } from "./redraw";

export const TEXT_PHASE_TEXT: Record<TextPhase, string> = {
  finding: "Finding text",
  reading: "Reading with AI",
  matching: "Matching fonts",
  done: "",
  failed: "",
};

const REASONS: Record<DeclineReason, string> = {
  script: "script lettering",
  custom: "custom lettering",
  gradient: "gradient fill",
  mismatch: "the text and the pixels disagree",
  no_fit: "no font fits",
  rotated: "rotated text",
  fonts: "no font available",
};

export const declineText = (reason: DeclineReason) => `Traced as shapes — ${REASONS[reason]}`;

/** Floored, as the drift check floors: 0.946 reads 94 %, never 95. */
export const fitPercent = (iou: number) => `${Math.floor(iou * 100 + 1e-9)} %`;

export const fontLabel = (f: FontRef) => `${f.family} ${f.weight}${f.italic ? " Italic" : ""}`;

const OWN: Record<string, string> = {
  no_key: "Add your OpenAI API key in Settings ▸ Text to read text with AI.",
  text_unavailable: "Text recognition needs macOS 10.15 or later.",
  text_failed: "Text recognition failed.",
};

export function textErrorText(code: string): string {
  return OWN[code] ?? ERROR_WORDS[code] ?? "Reading the text failed.";
}

/** Text under this height (source px) that the trace draws as shapes is what tracing at 2× is for. */
export const SMALL_TEXT_PX = 40;

/**
 * The most pixels (width × height of the image) the app traces at 2× on its own: Auto at 2× traces its four candidates
 * at once at four times the pixels, and peaked at 14.9 GB resident on a 1536 × 1024 logo (README, performance), so
 * 1536 · 1024 · 8 / 14.9 ≈ 0.85 MP keeps it under about 8 GB. The switch still forces 2× at any size.
 */
export const UPSCALE_MAX_PX = 800_000;

/** A line's height in source px: max y − min y of its quad. */
export const lineHeight = (line: Pick<TextLine, "quad">): number => {
  const ys = line.quad.map((p) => p[1]);
  return ys.length ? Math.max(...ys) - Math.min(...ys) : 0;
};

/**
 * Whether `plan` holds a line under `SMALL_TEXT_PX` that the trace draws as shapes: any line not among `setInFont`
 * (the lines composed over the trace — a trace's mask), so a declined line, one switched off or one with no font.
 * Lines the re-setter composes are not the trace's problem.
 */
export function smallTextPresent(plan: TextPlan | null, setInFont: Iterable<string>): boolean {
  if (!plan) return false;
  const set = new Set(setInFont);
  return plan.lines.some((l) => !set.has(l.id) && l.quad.length > 0 && lineHeight(l) < SMALL_TEXT_PX);
}

/**
 * The Trace at 2× switch's hint: why, while it follows the decision ("auto"), else what the user chose. `tooLarge`, the
 * image's pixels when small text was found but the image is over `UPSCALE_MAX_PX`, says why the decision is 1×.
 */
export function upscaleHint(mode: "auto" | "on" | "off", on: boolean, tooLarge: number | null = null): string {
  if (mode === "auto" && !on && tooLarge !== null) return `Small text found, but the image is too large to trace at 2× automatically (about ${(tooLarge / 1e6).toFixed(1)} MP); switch it on to force it`;
  if (mode === "auto") return on ? "Small text found: tracing at twice the size (several times slower; a minute or more on a large logo)" : "No small text: normal size";
  return on ? "Tracing at twice the size (several times slower; a minute or more on a large logo)" : "Normal size";
}
