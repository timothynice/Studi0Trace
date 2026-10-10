import { describe, expect, it } from "vitest";

import { declineText, fitPercent, fontLabel, TEXT_PHASE_TEXT, textErrorText } from "./text";

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
