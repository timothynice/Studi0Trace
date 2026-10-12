import { describe, expect, it } from "vitest";

import type { TextLine, TextPlan } from "@/platform/types";
import { declineText, fitPercent, fontLabel, SMALL_TEXT_PX, smallTextPresent, TEXT_PHASE_TEXT, textErrorText } from "./text";

describe("text copy", () => {
  it("names every phase and every reason", () => {
    for (const p of ["finding", "reading", "matching"] as const) expect(TEXT_PHASE_TEXT[p]).toBeTruthy();
    expect(declineText("script")).toBe("Traced as shapes — script lettering");
    expect(declineText("no_fit")).toBe("Traced as shapes — no font fits");
    expect(declineText("gradient")).toBe("Traced as shapes — gradient fill");
    expect(declineText("mismatch")).toBe("Traced as shapes — the text and the pixels disagree");
    expect(declineText("fonts")).toBe("Traced as shapes — no font available");
    expect(declineText("rotated")).toBe("Traced as shapes — rotated text");
    expect(declineText("custom")).toBe("Traced as shapes — custom lettering");
  });
  it("floors the fit percentage and labels fonts", () => {
    expect(fitPercent(0.946)).toBe("94 %");
    expect(fontLabel({ family: "Josefin Sans", weight: 300, italic: false, source: "google" })).toBe("Josefin Sans 300");
    expect(fontLabel({ family: "Lora", weight: 400, italic: true, source: "installed" })).toBe("Lora 400 Italic");
  });
  it("uses redraw's words for the AI codes and its own for the rest", () => {
    expect(textErrorText("no_key")).toBe("Add your OpenAI API key in Settings ▸ Text to read text with AI.");
    expect(textErrorText("quota")).toBe("Your OpenAI account is out of credit or rate limited.");
    expect(textErrorText("text_unavailable")).toBe("Text recognition needs macOS 10.15 or later.");
    expect(textErrorText("text_failed")).toBe("Text recognition failed.");
    expect(textErrorText("whatever")).toBe("Reading the text failed.");
  });
});

const line = (id: string, height: number, matched = true): TextLine => ({
  id,
  quad: [[2, 10], [80, 12], [80, 12 + height], [2, 10 + height]],
  string: id,
  class: "type",
  colour: [0, 0, 0],
  status: matched ? { kind: "matched" } : { kind: "declined", reason: "script" },
  candidates: [],
  chosen: matched ? 0 : null,
  enabled: matched,
  fit: null,
  occluders: [],
});
const plan = (lines: TextLine[]): TextPlan => ({ image: "a", reader: "vision", lines, aiError: null, fontsOffline: false });

describe("smallTextPresent", () => {
  it("is a line under 40 px that is not set in a font: declined, switched off, or no plan at all is none", () => {
    expect(SMALL_TEXT_PX).toBe(40);
    expect(smallTextPresent(null, [])).toBe(false);
    expect(smallTextPresent(plan([]), [])).toBe(false);
    // a declined 20 px line is traced as shapes
    expect(smallTextPresent(plan([line("d", 20, false)]), [])).toBe(true);
    // the quad's height is max y - min y of its four points: 10..52 is 42 px
    expect(smallTextPresent(plan([line("d", 40, false)]), [])).toBe(false);
    expect(smallTextPresent(plan([line("d", 60, false)]), [])).toBe(false);
    // a small line the re-setter composes is not the trace's problem; switched off, it is
    expect(smallTextPresent(plan([line("m", 20)]), ["m"])).toBe(false);
    expect(smallTextPresent(plan([line("m", 20)]), [])).toBe(true);
    expect(smallTextPresent(plan([line("m", 20), line("big", 60, false)]), new Set(["m"]))).toBe(false);
  });
});
