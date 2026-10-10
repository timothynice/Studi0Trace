import { describe, expect, it } from "vitest";
import type { TextLine, TextPlan } from "@/platform/types";
import { composeSvg } from "./textsvg";

const base = (shapes: string) => `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 60">${shapes}</svg>`;
const line = (
  id: string,
  quad: [number, number][],
  occluders: [number, number, number][] = [],
  colour: [number, number, number] = [0, 0, 0],
): TextLine => ({
  id,
  quad,
  string: "AB",
  class: "type",
  colour,
  status: { kind: "matched" },
  chosen: 0,
  enabled: true,
  fit: null,
  occluders,
  candidates: [
    {
      font: { family: "F", weight: 400, italic: false, source: "google" },
      iou: 0.9,
      stroke: 1,
      fit: { size: 10, baseline: 30, x: 10, tracking: 0, nudges: [], iou: 0.9, stroke: 1 },
      path: "M10 20h20v10h-20z",
      text: `<text x="10" y="30" fill="none" data-line="${id}">AB</text>`,
    },
  ],
});
const plan = (...lines: TextLine[]): TextPlan => ({ image: "i", reader: "vision", lines, aiError: null, fontsOffline: false });
const all = (p: TextPlan) => new Set(p.lines.map((l) => l.id));
const compose = (b: string, p: TextPlan | null, hiddenText = false, enabled?: Set<string>) =>
  composeSvg(b, { plan: p, enabled: enabled ?? (p ? all(p) : new Set()), hiddenText });
const children = (svg: string) => Array.from(new DOMParser().parseFromString(svg, "image/svg+xml").documentElement.children);
const box: [number, number][] = [[10, 20], [30, 20], [30, 30], [10, 30]];
const BACKDROP = '<rect width="100" height="60" fill="#ffffff"/>';
const GOLD = '<rect x="5" y="15" width="40" height="20" fill="#d49c17"/>';
const GOLD_RGB: [number, number, number] = [212, 156, 23];

describe("composeSvg", () => {
  it("appends a line with no occluder, before </svg>", () => {
    const out = compose(base(BACKDROP), plan(line("a", box)));
    const kids = children(out);
    expect(kids.map((k) => k.tagName)).toEqual(["rect", "path"]);
    expect(kids[1].getAttribute("data-line")).toBe("a");
    expect(kids[1].getAttribute("d")).toBe("M10 20h20v10h-20z");
    expect(kids[1].getAttribute("fill")).toBe("#000000");
    expect(kids[1].getAttribute("data-label")).toBe("AB");
  });

  it("inserts before the first overlapping shape whose fill matches an occluder", () => {
    const blue = '<rect x="60" y="40" width="20" height="10" fill="#0000ff"/>';
    const out = compose(base(BACKDROP + GOLD + blue), plan(line("a", box, [GOLD_RGB])));
    expect(children(out).map((k) => k.getAttribute("data-line") ?? k.getAttribute("fill"))).toEqual(["#ffffff", "a", "#d49c17", "#0000ff"]);
  });

  it("ignores a matching colour that does not overlap the line", () => {
    const far = '<rect x="60" y="40" width="20" height="10" fill="#d49c17"/>';
    const out = compose(base(BACKDROP + far), plan(line("a", box, [GOLD_RGB])));
    expect(children(out).map((k) => k.getAttribute("data-line") ?? k.getAttribute("fill"))).toEqual(["#ffffff", "#d49c17", "a"]);
  });

  it("keeps plan order between two overlapping lines", () => {
    const lower: [number, number][] = [[10, 40], [30, 40], [30, 50], [10, 50]];
    const out = compose(base(BACKDROP + GOLD), plan(line("A", box, [GOLD_RGB]), line("B", lower)));
    const ids = children(out).map((k) => k.getAttribute("data-line"));
    expect(ids.indexOf("A")).toBeGreaterThan(-1);
    expect(ids.indexOf("A")).toBeLessThan(ids.indexOf("B"));
    expect(ids.indexOf("A")).toBeLessThan(2); // before the gold rect
  });

  it("appends on an upsampled trace", () => {
    const up = base(`<g transform="scale(0.5)">${BACKDROP}${GOLD}</g>`);
    const out = compose(up, plan(line("a", box, [GOLD_RGB])));
    const kids = children(out);
    expect(kids.map((k) => k.tagName)).toEqual(["g", "path"]);
  });

  it("leaves the hidden text out when the setting is off, in when on", () => {
    const p = plan(line("a", box));
    expect(compose(base(BACKDROP), p, false)).not.toContain("<text");
    const on = children(compose(base(BACKDROP), p, true));
    expect(on.map((k) => k.tagName)).toEqual(["rect", "path", "text"]);
    expect(on[2].getAttribute("data-line")).toBe("a");
    expect(on[2].textContent).toBe("AB");
  });

  it("returns the base unchanged with no plan, no enabled line, or a line with no chosen font", () => {
    const b = base(BACKDROP);
    const p = plan(line("a", box));
    expect(compose(b, null)).toBe(b);
    expect(compose(b, p, false, new Set())).toBe(b);
    const declined = { ...line("a", box), status: { kind: "declined", reason: "script" } as const, chosen: null };
    expect(compose(b, plan(declined))).toBe(b);
    expect(compose("<svg>", p)).toBe("<svg>");
  });

  it("composes an enabled line that still has a chosen font whatever its status (a string edit that declined it)", () => {
    const b = base(BACKDROP);
    const declined = { ...line("a", box), status: { kind: "declined", reason: "no_fit" } as const };
    const out = children(compose(b, plan(declined)));
    expect(out.map((e) => e.getAttribute("data-line"))).toEqual([null, "a"]);
  });

  it("puts a lower line beneath an earlier one that lies over it in the source", () => {
    const upper = line("A", box, [], [10, 20, 30]);
    const lower = line("B", [[10, 25], [30, 25], [30, 35], [10, 35]], [[10, 20, 30]]);
    const ids = children(compose(base(BACKDROP), plan(upper, lower))).map((k) => k.getAttribute("data-line") ?? "rect");
    expect(ids).toEqual(["rect", "B", "A"]);
  });

  it("orders two lines under the same shape S with A over B as B, A, S", () => {
    const a = line("A", box, [GOLD_RGB], [10, 20, 30]);
    const b = line("B", [[10, 25], [30, 25], [30, 35], [10, 35]], [GOLD_RGB, [10, 20, 30]]);
    const ids = children(compose(base(BACKDROP + GOLD), plan(a, b))).map((k) => k.getAttribute("data-line") ?? k.getAttribute("fill"));
    expect(ids).toEqual(["#ffffff", "B", "A", "#d49c17"]);
  });
});
