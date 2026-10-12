import { deltaE, parseHex, toHex } from "./colour";
import { IDENTITY, multiply, parseSvg, parseTransform, svgDom, type Matrix, type SvgDoc } from "./svgdoc";
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

/** The matrix from `el`'s user space to the document's: every transform from the root down to it, its own included. */
function toDocument(el: Element, root: Element): Matrix {
  let m = IDENTITY;
  for (let e: Element | null = el; e && e !== root; e = e.parentElement) m = multiply(parseTransform(e.getAttribute("transform")), m);
  return m;
}

/** The transform that undoes `m`, as SVG writes it: `scale(2)` for Vexel's `scale(0.5)` group, else a matrix. */
function inverse(m: Matrix): string | null {
  const [a, b, c, d, e, f] = m;
  if (a === 1 && b === 0 && c === 0 && d === 1 && e === 0 && f === 0) return null;
  const det = a * d - b * c;
  if (!det) return null;
  const n: Matrix = [d / det, -b / det, -c / det, a / det, (c * f - d * e) / det, (b * e - a * f) / det];
  if (n[1] === 0 && n[2] === 0 && n[4] === 0 && n[5] === 0) return n[0] === n[3] ? `scale(${n[0]})` : `scale(${n[0]} ${n[3]})`;
  return `matrix(${n.join(" ")})`;
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

/**
 * The base trace with every enabled line's chosen candidate inserted where the source stacks it, whatever the line's
 * status: `enabled` is the trace's own mask, and a line it painted out is set in its font for as long as it has one (a
 * string edit that just declined it leaves the cached trace as it was until the new trace lands). Returns `base`
 * unchanged when nothing composes.
 */
export function composeSvg(base: string, { plan, enabled, hiddenText }: ComposeInput): string {
  if (!plan) return base;
  const lines = plan.lines.filter((l) => enabled.has(l.id) && l.chosen !== null && l.candidates[l.chosen]);
  if (!lines.length) return base;
  const doc = parseSvg(base);
  const dom = doc && svgDom(doc);
  if (!doc || !dom) return base;
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
    // the shapes' bounds are the document's (an upsampled trace's scaled group applied), as the line's quad is
    const at = insertionIndex(doc, line);
    let before: Element | null = at === null ? null : dom.elements[at];
    const box = quadBox(line.quad);
    const over = placed.find((p) => overlaps(p.box, box) && line.occluders.some((o) => deltaE(p.rgb, o) <= OCCLUDER_DE));
    // whichever of the two comes first in the document is the one the line goes beneath
    if (over && (!before || (before.compareDocumentPosition(over.el) & Node.DOCUMENT_POSITION_PRECEDING))) before = over.el;
    // beside a shape of an upsampled trace the line goes inside its scaled group, drawn back at the image's coordinates
    const parent = before?.parentElement ?? dom.root;
    const back = inverse(toDocument(parent, dom.root));
    for (const el of made) {
      if (back) el.setAttribute("transform", el.hasAttribute("transform") ? `${back} ${el.getAttribute("transform")}` : back);
      parent.insertBefore(el, before);
    }
    placed.push({ el: path, box, rgb: line.colour });
  }
  return new XMLSerializer().serializeToString(dom.root.ownerDocument);
}
