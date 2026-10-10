import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { useLayerInspector } from "./useLayerInspector";

const open = (body: string) => `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 4 4">${body}</svg>`;
const A = open('<path d="M0 0h4v4z" fill="#f00"/><path d="M1 1h.5v.5z" fill="#0f0"/>');
const A2 = A.replace("#0f0", "#0f1");
const B2 = open('<path d="M0 0h2v2z" fill="#00f"/><path d="M2 2h2v2z" fill="#ff1"/>');
const B = open('<path d="M0 0h2v2z" fill="#00f"/><path d="M2 2h2v2z" fill="#ff0"/>');

describe("useLayerInspector", () => {
  it("keeps the speck threshold through a re-trace of the same image, so an export never regains the specks", () => {
    const { result, rerender } = renderHook(({ svg, id }) => useLayerInspector(svg, id), { initialProps: { svg: A, id: "a" } });
    act(() => result.current.patch({ minArea: 1 }));
    expect(result.current.exportSvg).not.toContain("#0f0");
    rerender({ svg: A2, id: "a" });
    expect(result.current.state.minArea).toBe(1);
    expect(result.current.exportSvg).not.toContain("#0f1");
  });

  it("resets the hidden shapes and the highlight on a new SVG, but not the threshold", () => {
    const { result, rerender } = renderHook(({ svg, id }) => useLayerInspector(svg, id), { initialProps: { svg: A, id: "a" } });
    act(() => result.current.patch({ minArea: 1, hidden: new Set([0]), highlight: 1 }));
    rerender({ svg: A2, id: "a" });
    expect(result.current.state.hidden.size).toBe(0);
    expect(result.current.state.highlight).toBeNull();
    expect(result.current.state.minArea).toBe(1);
  });

  it("keeps the hidden shapes through a change of the composed SVG over the same base (a line toggled), not through another base", () => {
    const { result, rerender } = renderHook(({ svg, base }) => useLayerInspector(svg, "a", undefined, base), { initialProps: { svg: A, base: A } });
    act(() => result.current.patch({ hidden: new Set([1]), highlight: 0 }));
    const withText = A.replace("</svg>", '<path d="M0 0h1" fill="#000"/></svg>');
    rerender({ svg: withText, base: A });
    expect([...result.current.state.hidden]).toEqual([1]);
    expect(result.current.state.highlight).toBe(0);
    expect(result.current.exportSvg).not.toContain("#0f0");
    act(() => result.current.toggle(0));
    expect([...result.current.state.hidden].sort()).toEqual([0, 1]);
    rerender({ svg: A, base: A });
    expect([...result.current.state.hidden].sort()).toEqual([0, 1]);
    rerender({ svg: A2, base: A2 });
    expect(result.current.state.hidden.size).toBe(0);
  });

  it("exportSvgFor honours the hidden shapes for the same base under a different composed SVG", () => {
    const { result } = renderHook(() => useLayerInspector(A, "a"));
    act(() => result.current.toggle(1));
    const withText = A.replace("</svg>", '<path d="M0 0h1" fill="#000"/></svg>');
    expect(result.current.exportSvgFor("a", withText, A)).not.toContain("#0f0");
    expect(result.current.exportSvgFor("a", withText, A2)).toContain("#0f0");
  });

  it("holds the sections' open state in the window's shared state, all open to begin with", () => {
    const { result, rerender } = renderHook(({ svg, id }) => useLayerInspector(svg, id), { initialProps: { svg: A, id: "a" } });
    expect(result.current.state.sections).toEqual({ text: true, layers: true, cleanup: true });
    act(() => result.current.patch({ sections: { text: false, layers: true, cleanup: true } }));
    rerender({ svg: B, id: "b" });
    expect(result.current.state.sections.text).toBe(false);
    expect(result.current.liveState.sections.text).toBe(false);
  });

  it("holds the threshold per image: another image starts at off and the first keeps its own", () => {
    const { result, rerender } = renderHook(({ svg, id }) => useLayerInspector(svg, id), { initialProps: { svg: A, id: "a" } });
    act(() => result.current.patch({ minArea: 1 }));
    rerender({ svg: B, id: "b" });
    expect(result.current.state.minArea).toBe(0);
    expect(result.current.exportSvg).toBe(B);
    act(() => result.current.patch({ minArea: 3 }));
    rerender({ svg: A, id: "a" });
    expect(result.current.state.minArea).toBe(1);
    rerender({ svg: B, id: "b" });
    expect(result.current.state.minArea).toBe(3);
  });

  it("never applies another SVG's hidden shapes, not even in the first render after the SVG changes", () => {
    const seen: (string | undefined)[] = [];
    const { result, rerender } = renderHook(
      ({ svg }) => {
        const r = useLayerInspector(svg, "a");
        seen.push(r.exportSvg);
        return r;
      },
      { initialProps: { svg: A } },
    );
    act(() => result.current.patch({ hidden: new Set([0]) }));
    seen.length = 0;
    rerender({ svg: B });
    expect(seen.length).toBeGreaterThan(0);
    for (const s of seen) expect(s).toBe(B);
    expect(result.current.liveState.hidden.size).toBe(0);
  });

  it("keeps the panel and display toggles across images", () => {
    const { result, rerender } = renderHook(({ svg, id }) => useLayerInspector(svg, id), { initialProps: { svg: A, id: "a" } });
    act(() => result.current.patch({ open: true, points: true }));
    rerender({ svg: B, id: "b" });
    expect(result.current.state.open).toBe(true);
    expect(result.current.state.points).toBe(true);
  });
  it("hiding a shape by hand does not pin what the threshold drops: lowering the threshold brings the specks back", () => {
    const { result } = renderHook(() => useLayerInspector(A, "a"));
    act(() => result.current.patch({ minArea: 1 }));
    act(() => result.current.toggle(0));
    expect([...result.current.state.hidden]).toEqual([0]);
    expect([...result.current.dropped].sort()).toEqual([0, 1]);
    act(() => result.current.patch({ minArea: 0 }));
    expect([...result.current.dropped]).toEqual([0]);
    act(() => result.current.toggle(0));
    expect(result.current.dropped.size).toBe(0);
    expect(result.current.exportSvg).toBe(A);
  });

  it("resolves any image's SVG by its own state: its threshold, and its hidden shapes only for the SVG they were made on", () => {
    const { result, rerender } = renderHook(({ svg, id }) => useLayerInspector(svg, id), { initialProps: { svg: A, id: "a" } });
    act(() => result.current.patch({ minArea: 1 }));
    rerender({ svg: B, id: "b" });
    act(() => result.current.patch({ minArea: 3 }));
    act(() => result.current.toggle(0));
    expect(result.current.exportSvgFor("a", A)).not.toContain("#0f0");
    expect(result.current.exportSvgFor("a", A)).toContain("#f00");
    expect(result.current.exportSvgFor("b", B)).not.toContain("#00f");
    expect(result.current.exportSvgFor("b", B2)).toContain("#00f"); // its hidden shape was B's, not B2's
    expect(result.current.exportSvgFor("z", A)).toBe(A); // no entry: untouched
  });

  it("drops the entry of an image that has left the library", () => {
    const { result, rerender } = renderHook(({ ids }) => useLayerInspector(A, "a", ids), { initialProps: { ids: ["a", "b"] } });
    act(() => result.current.patch({ minArea: 1 }));
    expect(result.current.exportSvgFor("a", A)).not.toContain("#0f0");
    rerender({ ids: ["b"] });
    expect(result.current.exportSvgFor("a", A)).toBe(A);
    expect(result.current.state.minArea).toBe(0);
  });
});
