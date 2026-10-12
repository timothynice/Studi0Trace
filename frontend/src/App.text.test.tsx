import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { TextPlan } from "@/platform/types";
import { VEXEL, VEXEL_PRESETS } from "@/test/server";

vi.mock("sonner", () => ({ toast: { error: vi.fn(), success: vi.fn() }, Toaster: () => null }));

const hooks = vi.hoisted(() => ({ menu: null as null | ((c: string) => void), opens: null as null | ((p: string[]) => void) }));

const PLAN: TextPlan = {
  image: "/pics/small.png",
  reader: "vision",
  aiError: null,
  fontsOffline: false,
  lines: [
    {
      id: "/pics/small.png-t0",
      quad: [[4, 10], [40, 10], [40, 20], [4, 20]],
      string: "EST. 2026",
      class: "type",
      colour: [0, 0, 0],
      status: { kind: "matched" },
      candidates: [{ font: { family: "Montserrat", weight: 400, italic: false, source: "google" }, iou: 0.9, stroke: 1, fit: { size: 10, baseline: 20, x: 4, tracking: 0, nudges: [], iou: 0.9, stroke: 1 }, path: "M4 10h36v10h-36z", text: "<text/>", refined: true }],
      chosen: 0,
      enabled: true,
      fit: { size: 10, baseline: 20, x: 4, tracking: 0, nudges: [], iou: 0.9, stroke: 1 },
      occluders: [],
    },
  ],
};

/** A plan whose one line is declined (traced as shapes), `height` px tall. */
const declinedPlan = (image: string, height: number): TextPlan => ({
  ...PLAN,
  image,
  lines: [{ ...PLAN.lines[0], id: `${image}-t0`, quad: [[4, 10], [60, 10], [60, 10 + height], [4, 10 + height]], status: { kind: "declined", reason: "script" }, candidates: [], chosen: null, enabled: false, fit: null }],
});

vi.mock("@/platform", async () => {
  const types = await vi.importActual<typeof import("@/platform/types")>("@/platform/types");
  const platform = {
    kind: "native",
    canExportPdf: true,
    engines: async () => [VEXEL],
    presets: async () => VEXEL_PRESETS,
    pickImages: async () => [],
    openFiles: async () => [],
    openPaths: vi.fn(async (paths: string[]) => paths.map((p) => ({ ok: { id: p, name: p.split("/").pop(), path: p, width: 64, height: 64, format: "PNG", previewUrl: `blob:${p}` } }))),
    closeImage: vi.fn(),
    vectorize: vi.fn(async () => ({
      success: true,
      image_id: "x",
      width: 64,
      height: 64,
      results: { vexel: { svg: '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" fill="#ffffff"/></svg>', elapsed_ms: 500, stats: {} } },
      parameters_used: { vexel: {} },
      auto: null,
    })),
    exportFile: vi.fn(async () => null),
    exportAll: vi.fn(async () => null),
    copyText: vi.fn(async () => {}),
    reveal: vi.fn(async () => {}),
    loadSettings: async () => ({ ...types.DEFAULT_SETTINGS, suggestRedraw: false }),
    saveSettings: async (s: unknown) => s,
    onSettings: () => () => {},
    onMenu: (cb: (c: string) => void) => {
      hooks.menu = cb;
      return () => {};
    },
    onOpenPaths: (cb: (p: string[]) => void) => {
      hooks.opens = cb;
      return () => {};
    },
    onOpenFailures: () => () => {},
    onDragState: () => () => {},
    setMenuState: () => {},
    confirmClear: vi.fn(async () => true),
    openSettingsWindow: () => true,
    windowRole: () => "main",
    redrawKeyStatus: vi.fn(async () => false),
    setRedrawKey: vi.fn(async () => {}),
    deleteRedrawKey: vi.fn(async () => {}),
    onRedrawKey: () => () => {},
    imageRoughness: vi.fn(async () => ({ rough: false, reason: null })),
    textPlan: vi.fn(async () => PLAN),
    textSetLine: vi.fn(async () => ({ plan: PLAN })),
    textSearchFonts: vi.fn(async () => []),
  };
  return { ...types, platform };
});

const { default: App } = await import("./App");
const { platform } = await import("@/platform");

describe("Text in the Mac app", () => {
  it("the Inspect tab lists the plan's lines, and hovering a line marks its box on the canvas", async () => {
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <App />
      </QueryClientProvider>,
    );
    await screen.findByText("Drop images here");
    await waitFor(() => expect(hooks.opens).not.toBeNull());
    act(() => hooks.opens!(["/pics/small.png"]));
    await screen.findByRole("option", { name: /small\.png/ });
    act(() => hooks.menu!("generate"));
    await waitFor(() => expect(platform.vectorize).toHaveBeenCalled());
    expect(vi.mocked(platform.vectorize).mock.calls[0][0]).toMatchObject({ mask: ["/pics/small.png-t0"] });
    fireEvent.mouseDown(screen.getByRole("tab", { name: "Inspect" }));
    const row = await screen.findByRole("group", { name: "Line 1" });
    expect(within(row).getByDisplayValue("EST. 2026")).toBeInTheDocument();
    expect(within(row).getByRole("switch", { name: /Re-set line 1/ })).toBeChecked();
    const viewport = screen.getByTestId("viewport");
    await waitFor(() => expect(viewport.querySelector("svg")).not.toBeNull());
    expect(viewport.querySelector("polygon")).toBeNull();
    fireEvent.mouseEnter(row);
    await waitFor(() => expect(viewport.querySelector("polygon")).not.toBeNull());
    expect(viewport.querySelector("polygon")!.getAttribute("points")).toBe("4,10 40,10 40,20 4,20");
    fireEvent.mouseLeave(row);
    await waitFor(() => expect(viewport.querySelector("polygon")).toBeNull());
  });

  async function openAndTrace(path: string, plan: TextPlan) {
    vi.mocked(platform.vectorize).mockClear();
    vi.mocked(platform.textPlan).mockResolvedValueOnce(plan);
    render(
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <App />
      </QueryClientProvider>,
    );
    await screen.findByText("Drop images here");
    await waitFor(() => expect(hooks.opens).not.toBeNull());
    act(() => hooks.opens!([path]));
    await screen.findByRole("option", { name: new RegExp(path.split("/").pop()!.replace(".", "\\.")) });
    act(() => hooks.menu!("generate"));
    await waitFor(() => expect(platform.vectorize).toHaveBeenCalled());
    fireEvent.mouseDown(screen.getByRole("tab", { name: "Inspect" }));
    return screen.findByRole("switch", { name: "Trace at 2×" });
  }

  it("Trace at 2× is on for a declined 20 px line: Auto asks every candidate for 2×, and the hint says why", async () => {
    const toggle = await openAndTrace("/pics/tagline.png", declinedPlan("/pics/tagline.png", 20));
    expect(vi.mocked(platform.vectorize).mock.calls[0][0]).toMatchObject({ auto: true, parameters: { upsample: "always" } });
    expect(toggle).toBeChecked();
    expect(screen.getByText("Small text found: tracing at twice the size, about four times slower")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Auto" })).toBeNull();
  });

  it("Trace at 2× is off for a 60 px line; switched on, it traces again at 2×, and Auto goes back to the decision", async () => {
    const toggle = await openAndTrace("/pics/wordmark.png", declinedPlan("/pics/wordmark.png", 60));
    expect(vi.mocked(platform.vectorize).mock.calls[0][0]).toMatchObject({ auto: true, parameters: {} });
    expect(toggle).not.toBeChecked();
    expect(screen.getByText("No small text: normal size")).toBeInTheDocument();
    await waitFor(() => expect(screen.getByTestId("viewport").querySelector("svg")).not.toBeNull());
    fireEvent.click(toggle);
    await waitFor(() => expect(platform.vectorize).toHaveBeenCalledTimes(2));
    expect(vi.mocked(platform.vectorize).mock.calls[1][0]).toMatchObject({ auto: true, parameters: { upsample: "always" } });
    await waitFor(() => expect(screen.getByRole("switch", { name: "Trace at 2×" })).toBeChecked());
    fireEvent.click(screen.getByRole("button", { name: "Auto" }));
    await waitFor(() => expect(screen.getByRole("switch", { name: "Trace at 2×" })).not.toBeChecked());
    expect(screen.getByText("No small text: normal size")).toBeInTheDocument();
    expect(platform.vectorize).toHaveBeenCalledTimes(2);
  });
});
