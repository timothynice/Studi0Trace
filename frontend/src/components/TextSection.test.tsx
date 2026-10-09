import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { ImageItem } from "@/state/library";
import type { TextLine, TextPlan } from "@/platform/types";
import { TextSection } from "./TextSection";

const line = (over: Partial<TextLine> = {}): TextLine => ({ id: "a-t0", quad: [[0, 0], [10, 0], [10, 5], [0, 5]], string: "EST. 2026", class: "type", colour: [0, 0, 0], status: { kind: "matched" }, candidates: [{ font: { family: "Montserrat", weight: 400, italic: false, source: "google" }, iou: 0.946 }, { font: { family: "Lato", weight: 400, italic: false, source: "installed" }, iou: 0.8 }], chosen: 0, enabled: true, fit: { size: 10, baseline: 5, x: 0, tracking: 0, nudges: [], iou: 0.946 }, ...over });
const item = (plan: TextPlan | null, phase: "finding" | null = null, error: null | { code: string } = null): ImageItem =>
  ({ image: { id: "a", name: "a.png", path: null, width: 100, height: 50, format: "PNG", previewUrl: "" }, preset: "auto", params: {}, traces: {}, shown: null, exported: null, job: null, error: null, errorKey: null, auto: null, text: { plan, phase, error: error as never, asked: true } }) as ImageItem;
const plan = (lines: TextLine[], reader: "vision" | "ai" = "vision"): TextPlan => ({ image: "a", reader, lines, aiError: null, fontsOffline: false });
const props = () => ({ enabled: true, onChange: vi.fn(), onHover: vi.fn(), onCancel: vi.fn(), searchFonts: vi.fn(async () => []) });

describe("TextSection", () => {
  it("lists a matched line with its string, font, fit and switch", () => {
    const p = props();
    render(<TextSection item={item(plan([line()]))} {...p} />);
    expect(screen.getByRole("heading", { name: /Text/ })).toHaveTextContent("1 line");
    expect(screen.getByText("On-device")).toBeInTheDocument();
    expect(screen.getByDisplayValue("EST. 2026")).toBeInTheDocument();
    expect(screen.getByText("Fit 94 %")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: /Re-set/ })).toBeChecked();
    expect(screen.getByRole("combobox", { name: /Font/ })).toHaveValue("0");
  });

  it("edits call onChange with the string, the font or the switch", () => {
    const p = props();
    render(<TextSection item={item(plan([line()]))} {...p} />);
    const input = screen.getByDisplayValue("EST. 2026");
    fireEvent.change(input, { target: { value: "EST. 2027" } });
    fireEvent.blur(input);
    expect(p.onChange).toHaveBeenCalledWith("a-t0", { string: "EST. 2027" });
    fireEvent.change(screen.getByRole("combobox", { name: /Font/ }), { target: { value: "1" } });
    expect(p.onChange).toHaveBeenCalledWith("a-t0", { font: { family: "Lato", weight: 400, italic: false, source: "installed" } });
    fireEvent.click(screen.getByRole("switch", { name: /Re-set/ }));
    expect(p.onChange).toHaveBeenCalledWith("a-t0", { enabled: false });
  });

  it("says why a line is traced as shapes, with its switch off", () => {
    render(<TextSection item={item(plan([line({ status: { kind: "declined", reason: "script" }, enabled: false, chosen: null, fit: null })]))} {...props()} />);
    expect(screen.getByText("Traced as shapes — script lettering")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: /Re-set/ })).not.toBeChecked();
  });

  it("hovering a row reports the line, leaving reports null", () => {
    const p = props();
    render(<TextSection item={item(plan([line()]))} {...p} />);
    const row = screen.getByRole("group", { name: /Line 1/ });
    fireEvent.mouseEnter(row);
    expect(p.onHover).toHaveBeenCalledWith("a-t0");
    fireEvent.mouseLeave(row);
    expect(p.onHover).toHaveBeenCalledWith(null);
  });

  it("shows the phase while reading, the empty state, the AI reader and its error, and nothing when off", () => {
    const p = props();
    const { rerender } = render(<TextSection item={item(null, "finding")} {...p} />);
    expect(screen.getByText("Finding text")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(p.onCancel).toHaveBeenCalled();
    rerender(<TextSection item={item(plan([]))} {...p} />);
    expect(screen.getByText("No text found")).toBeInTheDocument();
    rerender(<TextSection item={item({ ...plan([line()], "ai"), aiError: "quota" })} {...p} />);
    expect(screen.getByText("AI")).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("out of credit");
    rerender(<TextSection item={item(plan([line()]))} {...p} enabled={false} />);
    expect(screen.queryByRole("heading", { name: /Text/ })).toBeNull();
  });

  it("searches fonts from the picker and picks a result", async () => {
    const p = props();
    p.searchFonts = vi.fn(async () => [{ family: "Josefin Sans", weight: 300, italic: false, source: "google" as const }]);
    render(<TextSection item={item(plan([line()]))} {...p} />);
    fireEvent.change(screen.getByRole("combobox", { name: /Font/ }), { target: { value: "search" } });
    const box = await screen.findByRole("textbox", { name: /Search fonts/ });
    fireEvent.change(box, { target: { value: "jos" } });
    const hit = await screen.findByRole("button", { name: "Josefin Sans 300" });
    fireEvent.click(hit);
    expect(p.onChange).toHaveBeenCalledWith("a-t0", { font: { family: "Josefin Sans", weight: 300, italic: false, source: "google" } });
  });

  it("after a revert, a trace that carries text with no plan says to trace again", () => {
    const it = item(null);
    it.traces = { auto: { svg: "<svg/>", base: "<svg/>", textGroup: "<g/>", elapsedMs: 1, stats: {} } } as never;
    it.shown = "auto" as never;
    render(<TextSection item={it} {...props()} />);
    expect(screen.getByText("Text was re-set — trace again to edit it")).toBeInTheDocument();
    expect(screen.queryByText("No text found")).toBeNull();
  });
});
