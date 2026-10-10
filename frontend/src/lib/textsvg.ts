import { deltaE, parseHex, toHex } from "./colour";
import { parseSvg, svgDom, type SvgDoc } from "./svgdoc";
import type { TextLine, TextPlan } from "@/platform/types";

const SVG_NS = "http://www.w3.org/2000/svg";

/** How far (ΔE) a traced shape's fill may be from an occluder colour and still be the shape that lay over the line. */
export const OCCLUDER_DE = 12;

export interface ComposeInput {
  plan: TextPlan | null;
  /** The ids of the lines that are set. */
  enabled: ReadonlySet<string>;
  /** Settings ▸ Text ▸ hidden text: each line's invisible `<text>` is written beside its glyphs. */
  hiddenText: boolean;
}

function quadBox(quad: [number, number][]): [number, number, number, number] {
  const xs = quad.map((p) => p[0]);
  const ys = quad.map((p) => p[1]);
  return [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)];
}

function overlaps(a: [number, number, number, number], b: [number, number, number, number]): boolean {
  return a[0] < b[2] && a[2] > b[0] && a[1] < b[3] && a[3] > b[1];
}

/** The index in `doc.shapes` before which `line` goes — the first traced shape that overlaps it and is painted in one of
 *  its occluders' colours, which is what lay over the line in the source — or null to append. */
export function insertionIndex(doc: SvgDoc, line: TextLine): number | null {
  if (!line.occluders.length || !line.quad.length) return null;
  const [x0, y0, x1, y1] = quadBox(line.quad);
  for (const s of doc.shapes) {
    if (s.line !== undefined) continue;
    const [x, y, w, h] = s.bounds;
    if (x >= x1 || x + w <= x0 || y >= y1 || y + h <= y0) continue;
    const fill = parseHex(s.fill);
    if (fill && line.occluders.some((o) => deltaE(fill, o) <= OCCLUDER_DE)) return s.index;
  }
  return null;
}

/** The base trace with every enabled, matched line's chosen candidate inserted where the source stacks it. Returns `base` unchanged when nothing composes. */
export function composeSvg(base: string, { plan, enabled, hiddenText }: ComposeInput): string {
  if (!plan) return base;
  const lines = plan.lines.filter((l) => enabled.has(l.id) && l.status.kind === "matched" && l.chosen !== null && l.candidates[l.chosen]);
  if (!lines.length) return base;
  const doc = parseSvg(base);
  const dom = doc && svgDom(doc);
  if (!doc || !dom) return base;
  // an upsampled trace draws inside a scaled group: the line's coordinates are the image's, so it goes on top of it
  const scaled = doc.shapes.some((s) => dom.elements[s.index]?.parentElement !== dom.root);
  const document = dom.root.ownerDocument;
  // the lines inserted so far: a line may lie under one set before it, as the source stacks them
  const placed: { el: Element; box: [number, number, number, number]; rgb: [number, number, number] }[] = [];
  for (const line of lines) {
    const candidate = line.candidates[line.chosen as number];
    const path = document.createElementNS(SVG_NS, "path");
    path.setAttribute("d", candidate.path);
    path.setAttribute("fill", toHex(line.colour));
    path.setAttribute("data-line", line.id);
    path.setAttribute("data-label", line.string);
    const made: Element[] = [path];
    if (hiddenText) {
      const holder = new DOMParser().parseFromString(`<svg xmlns="${SVG_NS}">${candidate.text}</svg>`, "image/svg+xml");
      if (!holder.getElementsByTagName("parsererror").length) {
        for (const el of Array.from(holder.documentElement.children)) made.push(document.importNode(el, true));
      }
    }
    const at = scaled ? null : insertionIndex(doc, line);
    let before: Element | null = at === null ? null : dom.elements[at];
    const box = quadBox(line.quad);
    if (!scaled) {
      const over = placed.find((p) => overlaps(p.box, box) && line.occluders.some((o) => deltaE(p.rgb, o) <= OCCLUDER_DE));
      // whichever of the two comes first in the document is the one the line goes beneath
      if (over && (!before || (before.compareDocumentPosition(over.el) & Node.DOCUMENT_POSITION_PRECEDING))) before = over.el;
    }
    for (const el of made) dom.root.insertBefore(el, before);
    placed.push({ el: path, box, rgb: line.colour });
  }
  return new XMLSerializer().serializeToString(dom.root.ownerDocument);
}
