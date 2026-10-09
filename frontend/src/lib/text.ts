/** Text re-setting's words: the phases, the reasons a line is traced as shapes, the failures. */
import type { DeclineReason, FontRef, TextPhase } from "@/platform/types";
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
};

export function textErrorText(code: string): string {
  return OWN[code] ?? ERROR_WORDS[code] ?? "Reading the text failed.";
}
