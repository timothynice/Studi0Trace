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
 * What a trace at 2× is predicted to hold resident per pixel of the 2× image (4 × the source's pixels): the heaviest
 * pick, Balanced, peaked at 18.2 GB resident on a 1536 × 1024 logo traced at 3072 × 2048 (README, performance), about
 * 2.9 KB a pixel, rounded up.
 */
export const UPSCALE_BYTES_PER_PX = 3000;

/** The share of the Mac's physical memory a trace at 2× the app decides on by itself may be predicted to need. */
export const UPSCALE_MEMORY_SHARE = 0.5;

/**
 * The fallback when the Mac's memory is not known (the browser harness, or the app could not read it): the most
 * pixels (width × height of the image) the app traces at 2× on its own, 1536 · 1024 · 8 / 18.2 ≈ 0.69 MP rounded down,
 * which keeps the heaviest pick under about 8 GB. The switch still forces 2× at any size.
 */
export const UPSCALE_FALLBACK_MAX_PX = 600_000;

/** A trace at 2×'s predicted peak resident memory, in bytes, for an image of `width` × `height`. */
export const upscalePeakBytes = (width: number, height: number): number => UPSCALE_BYTES_PER_PX * 4 * width * height;

/**
 * Whether the app may decide on 2× by itself for an image of `width` × `height` on a Mac with `memoryBytes` of physical
 * memory: the predicted peak (`upscalePeakBytes`) is at most `UPSCALE_MEMORY_SHARE` of it; with the memory unknown
 * (null), the image is at most `UPSCALE_FALLBACK_MAX_PX`.
 */
export function upscaleFits(width: number, height: number, memoryBytes: number | null): boolean {
  return memoryBytes ? upscalePeakBytes(width, height) <= UPSCALE_MEMORY_SHARE * memoryBytes : width * height <= UPSCALE_FALLBACK_MAX_PX;
}

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
 * The Trace at 2× switch's hint: why, while it follows the decision ("auto"), else what the user chose. `tooLarge`, set
 * when small text was found but the image does not fit (`upscaleFits`), says why the decision is 1×: with the Mac's
 * memory known, the trace's predicted peak (`upscalePeakBytes`); without it, the pixel fallback decided, and says so.
 */
export function upscaleHint(mode: "auto" | "on" | "off", on: boolean, tooLarge: { bytes: number; memoryKnown: boolean } | null = null): string {
  if (mode === "auto" && !on && tooLarge !== null)
    return tooLarge.memoryKnown
      ? `Small text found, but the image is too large to trace at 2× automatically on this Mac (it would need about ${Math.max(1, Math.round(tooLarge.bytes / 1e9))} GB); switch it on to force it`
      : `Small text found, but the image is too large to trace at 2× automatically (over ${UPSCALE_FALLBACK_MAX_PX / 1e6} MP); switch it on to force it`;
  if (mode === "auto") return on ? "Small text found: tracing at twice the size (several times slower; a minute or more on a large logo)" : "No small text: normal size";
  return on ? "Tracing at twice the size (several times slower; a minute or more on a large logo)" : "Normal size";
}
