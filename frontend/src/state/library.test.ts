import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ApiError, type EngineDescription, type Preset, type VectorizeResponse } from "@/lib/api";
import { paramsKey } from "@/lib/schema";
import type { Drift, OpenOutcome, Phase, Platform, RedrawPhase, RedrawResult, TextLine, TextLineChange, TextPlan, TraceRequest } from "@/platform/types";
import { DEFAULT_SETTINGS } from "@/platform/types";
import { composeSvg } from "@/lib/textsvg";
import { createLibrary, currentJob, DEBOUNCE_MS, errorOf, fullMask, maskOf, MASKED_TRACES_MAX, PREFETCH_MAX_LINES, PREFETCH_SETTLE_MS, needsUpdate, NO_REDRAW, shownAnswer, textOf, traceKey, traceKeyFor, unexportedCount, userJob, type Catalog, type LibraryPlatform } from "./library";

const ENGINE: EngineDescription = {
  id: "vexel",
  label: "Vexel",
  description: "",
  primary: true,
  params: {
    properties: {
      detail: { type: "number", default: 6, minimum: 1, maximum: 20 },
      min_region: { type: "integer", default: 8, minimum: 1, maximum: 64 },
    },
  },
  defaults: { detail: 6, min_region: 8 },
};
const PRESETS: Preset[] = [
  { id: "auto", label: "Auto", engine: "vexel", description: "", detail: "", sample: "auto.png", params: {}, kind: "auto" },
  { id: "balanced", label: "Balanced", engine: "vexel", description: "", detail: "", sample: "b.png", params: {}, auto_candidate: true },
  { id: "logo", label: "Logo & icon", engine: "vexel", description: "", detail: "", sample: "l.png", params: { detail: 10, min_region: 16 }, auto_candidate: true },
];
const CATALOG: Catalog = { engine: ENGINE, presets: PRESETS };

const image = (id: string, path: string | null = `/pics/${id}.png`) => ({ id, name: `${id}.png`, path, width: 64, height: 32, format: "PNG", previewUrl: `blob:${id}` });
const ok = (id: string, path?: string | null): OpenOutcome => ({ ok: image(id, path) });

function response(svg: string, elapsed: number, extra: Partial<VectorizeResponse> = {}): VectorizeResponse {
  return {
    success: true,
    image_id: "x",
    width: 64,
    height: 32,
    results: { vexel: { svg, elapsed_ms: elapsed, stats: { paths: 3 } } },
    parameters_used: { vexel: {} },
    auto: null,
    ...extra,
  };
}

interface Call { req: TraceRequest; signal: AbortSignal; onPhase?: (p: Phase) => void; resolve: (r: VectorizeResponse) => void; reject: (e: unknown) => void }

function fakePlatform() {
  const calls: Call[] = [];
  const platform = {
    vectorize: vi.fn((req: TraceRequest, opts: { signal: AbortSignal; onPhase?: (p: Phase) => void }) =>
      new Promise<VectorizeResponse>((resolve, reject) => calls.push({ req, signal: opts.signal, onPhase: opts.onPhase, resolve, reject })),
    ),
    closeImage: vi.fn(),
    openPaths: vi.fn(async (paths: string[]) => paths.map((p) => ok("small", p))),
  } satisfies Pick<Platform, "vectorize" | "closeImage" | "openPaths">;
  return { platform, calls };
}

// the timers are fake; a few turns of the microtask queue let a settled promise's handlers run
const flush = async () => {
  for (let i = 0; i < 5; i++) await Promise.resolve();
};

describe("library", () => {
  beforeEach(() => vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] }));
  afterEach(() => vi.useRealTimers());

  it("opens images on Auto with the engine's defaults, selects the last, and does not trace", () => {
    const { platform } = fakePlatform();
    const lib = createLibrary(platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([ok("a"), ok("b")]);
    const s = lib.getState();
    expect(s.items.map((i) => i.image.id)).toEqual(["a", "b"]);
    expect(s.selected).toBe("b");
    expect(s.items[0]).toMatchObject({ preset: "auto", params: { detail: 6, min_region: 8 }, shown: null, job: null });
    expect(platform.vectorize).not.toHaveBeenCalled();
  });

  it("keeps one card per file and keeps the files it could not open", () => {
    const { platform } = fakePlatform();
    const lib = createLibrary(platform, CATALOG, DEFAULT_SETTINGS);
    const failure = { failed: { name: "huge.png", path: "/pics/huge.png", error: new ApiError("too_many_pixels", "Image exceeds the 2048x2048 pixel limit", 400) } };
    lib.add([ok("a"), ok("a"), failure]);
    expect(lib.getState().items).toHaveLength(1);
    expect(lib.getState().failed.map((f) => f.name)).toEqual(["huge.png"]);
  });

  it("traces Auto, keeps every candidate as its preset's trace, and follows the pick", async () => {
    const { platform, calls } = fakePlatform();
    const lib = createLibrary(platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([ok("a")]);
    lib.generate("a");
    expect(calls[0].req).toMatchObject({ imageId: "a", auto: true, parameters: {} });
    expect(lib.getState().items[0].job).toMatchObject({ key: "auto|", phase: "queued" });
    calls[0].onPhase?.("tracing");
    expect(lib.getState().items[0].job?.phase).toBe("tracing");
    calls[0].resolve(
      response("<svg>logo</svg>", 900, {
        auto: {
          vexel: {
            engine: "vexel",
            pick: "logo",
            reason: "the cleanest at the same fidelity",
            candidates: [
              { preset: "balanced", label: "Balanced", svg: "<svg>balanced</svg>", elapsed_ms: 800, stats: { paths: 9 } },
              { preset: "logo", label: "Logo & icon", svg: "<svg>logo</svg>", elapsed_ms: 900, stats: { paths: 4 } },
            ],
          },
        },
      }),
    );
    await flush();
    const item = lib.getState().items[0];
    expect(item.shown).toBe("auto|");
    expect(item.job).toBeNull();
    expect(item.auto?.pick).toBe("logo");
    expect(item.params).toEqual({ detail: 10, min_region: 16 });
    expect(item.traces[`${paramsKey({ detail: 6, min_region: 8 })}|`].svg).toBe("<svg>balanced</svg>");
    // a candidate is shown at once, nothing traced again
    lib.pickPreset("a", PRESETS[1]);
    expect(lib.getState().items[0].shown).toBe(`${paramsKey({ detail: 6, min_region: 8 })}|`);
    expect(platform.vectorize).toHaveBeenCalledTimes(1);
  });

  it("re-traces by itself after a quick trace, and asks for Update after a slow one", async () => {
    const { platform, calls } = fakePlatform();
    const lib = createLibrary(platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([ok("a"), ok("b")]);
    lib.pickPreset("a", PRESETS[1]);
    lib.generate("a");
    calls[0].resolve(response("<svg>a</svg>", 500));
    await flush();
    lib.setParam("a", "detail", 7);
    vi.advanceTimersByTime(DEBOUNCE_MS - 1);
    expect(platform.vectorize).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(1);
    expect(platform.vectorize).toHaveBeenCalledTimes(2);
    expect(calls[1].req.parameters).toEqual({ detail: 7, min_region: 8 });

    lib.pickPreset("b", PRESETS[1]);
    lib.generate("b");
    calls[2].resolve(response("<svg>b</svg>", 5000));
    await flush();
    lib.setParam("b", "detail", 9);
    vi.advanceTimersByTime(DEBOUNCE_MS * 4);
    expect(platform.vectorize).toHaveBeenCalledTimes(3);
    expect(needsUpdate(lib.getState().items[1])).toBe(true);
  });

  it("does not re-trace by itself when live updates are off", async () => {
    const { platform, calls } = fakePlatform();
    const lib = createLibrary(platform, CATALOG, { ...DEFAULT_SETTINGS, liveUpdate: false });
    lib.add([ok("a")]);
    lib.pickPreset("a", PRESETS[1]);
    lib.generate("a");
    calls[0].resolve(response("<svg>a</svg>", 100));
    await flush();
    lib.setParam("a", "detail", 7);
    vi.advanceTimersByTime(DEBOUNCE_MS * 4);
    expect(platform.vectorize).toHaveBeenCalledTimes(1);
  });

  it("a newer request supersedes the running one, whose late answer is dropped", async () => {
    const { platform, calls } = fakePlatform();
    const lib = createLibrary(platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([ok("a")]);
    lib.pickPreset("a", PRESETS[1]);
    lib.generate("a");
    lib.setParam("a", "detail", 12);
    lib.generate("a");
    expect(calls[0].signal.aborted).toBe(true);
    calls[1].resolve(response("<svg>new</svg>", 100));
    calls[0].resolve(response("<svg>old</svg>", 100));
    await flush();
    const item = lib.getState().items[0];
    expect(item.traces[traceKey(item)].svg).toBe("<svg>new</svg>");
    expect(Object.values(item.traces).map((t) => t.svg)).not.toContain("<svg>old</svg>");
  });

  it("cancel drops the job at once and is not an error", async () => {
    const { platform, calls } = fakePlatform();
    const lib = createLibrary(platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([ok("a")]);
    lib.generate("a");
    lib.cancel("a");
    expect(lib.getState().items[0].job).toBeNull();
    expect(calls[0].signal.aborted).toBe(true);
    calls[0].reject(new ApiError("cancelled", "Cancelled", 499));
    await flush();
    expect(lib.getState().items[0].error).toBeNull();
  });

  it("an engine's error and a failed request become the item's error", async () => {
    const { platform, calls } = fakePlatform();
    const lib = createLibrary(platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([ok("a"), ok("b")]);
    lib.generate("a");
    calls[0].resolve({ ...response("", 0), results: { vexel: { error: { code: "engine_crashed", message: "The trace crashed" } } } });
    lib.generate("b");
    calls[1].reject(new ApiError("engine_crashed", "The trace crashed", 500));
    await flush();
    expect(lib.getState().items.map((i) => i.error?.code)).toEqual(["engine_crashed", "engine_crashed"]);
    expect(lib.getState().items.map((i) => i.job)).toEqual([null, null]);
  });

  it("remove closes the image, cancels its job and moves the selection", () => {
    const { platform, calls } = fakePlatform();
    const lib = createLibrary(platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([ok("a"), ok("b"), ok("c")]);
    lib.select("b");
    lib.generate("b");
    lib.remove("b");
    expect(platform.closeImage).toHaveBeenCalledWith("b");
    expect(calls[0].signal.aborted).toBe(true);
    expect(lib.getState().selected).toBe("c");
    lib.remove("c");
    expect(lib.getState().selected).toBe("a");
    lib.clear();
    expect(lib.getState()).toEqual({ items: [], failed: [], selected: null });
  });

  it("selectNext walks the list and stops at the ends", () => {
    const { platform } = fakePlatform();
    const lib = createLibrary(platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([ok("a"), ok("b")]);
    lib.selectNext(1);
    expect(lib.getState().selected).toBe("b");
    lib.selectNext(-1);
    lib.selectNext(-1);
    expect(lib.getState().selected).toBe("a");
  });

  it("downscale reopens a too-large file at 2048 px in its place", async () => {
    const { platform } = fakePlatform();
    const lib = createLibrary(platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([{ failed: { name: "huge.png", path: "/pics/huge.png", error: new ApiError("too_many_pixels", "too big", 400) } }]);
    await lib.downscale(0);
    expect(platform.openPaths).toHaveBeenCalledWith(["/pics/huge.png"], { downscale: true });
    expect(lib.getState().failed).toEqual([]);
    expect(lib.getState().items.map((i) => i.image.id)).toEqual(["small"]);
  });

  it("traces new images straight away when the setting says so", () => {
    const { platform } = fakePlatform();
    const lib = createLibrary(platform, CATALOG, { ...DEFAULT_SETTINGS, traceOnOpen: true });
    lib.add([ok("a"), ok("b")]);
    expect(platform.vectorize).toHaveBeenCalledTimes(2);
  });

  it("a trace landing after the settings moved on does not take the screen or the controls", async () => {
    const { platform, calls } = fakePlatform();
    const lib = createLibrary(platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([ok("a"), ok("b")]);
    // A: the job for detail 7 is running when the user goes back to the cached detail 6
    lib.pickPreset("a", PRESETS[1]);
    lib.generate("a");
    calls[0].resolve(response("<svg>six</svg>", 500));
    await flush();
    lib.setParam("a", "detail", 7);
    vi.advanceTimersByTime(DEBOUNCE_MS);
    expect(lib.getState().items[0].job).not.toBeNull();
    lib.setParam("a", "detail", 6);
    // the running job is let finish: its answer is cached for the day those settings come back
    expect(lib.getState().items[0].job).not.toBeNull();
    expect(calls[1].signal.aborted).toBe(false);
    calls[1].resolve(response("<svg>seven</svg>", 100));
    await flush();
    const a = lib.getState().items[0];
    expect(a.job).toBeNull();
    expect(a.shown).toBe(traceKey(a));
    expect(a.traces[a.shown as string].svg).toBe("<svg>six</svg>");
    expect(Object.values(a.traces).map((t) => t.svg)).toContain("<svg>seven</svg>");
    expect(needsUpdate(a)).toBe(false);
    // B: Balanced is picked while Auto runs; Auto lands with Logo as its pick
    lib.generate("b");
    lib.pickPreset("b", PRESETS[1]);
    calls[2].resolve(
      response("<svg>logo</svg>", 900, {
        auto: { vexel: { engine: "vexel", pick: "logo", reason: "", candidates: [
          { preset: "balanced", label: "Balanced", svg: "<svg>balanced</svg>", elapsed_ms: 800, stats: {} },
          { preset: "logo", label: "Logo & icon", svg: "<svg>logo</svg>", elapsed_ms: 900, stats: {} },
        ] } },
      }),
    );
    await flush();
    const b = lib.getState().items[1];
    expect(b.preset).toBe("balanced");
    expect(b.params).toEqual({ detail: 6, min_region: 8 });
    expect(b.traces["auto|"].svg).toBe("<svg>logo</svg>");
    expect(b.shown).toBe(traceKey(b));
    expect(b.traces[b.shown as string].svg).toBe("<svg>balanced</svg>");
    expect(b.auto?.pick).toBe("logo");
  });

  it("Cancel, a changed setting and removal all end a pending live update", async () => {
    const { platform, calls } = fakePlatform();
    const lib = createLibrary(platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([ok("a"), ok("b"), ok("c")]);
    for (const id of ["a", "b", "c"]) {
      lib.pickPreset(id, PRESETS[1]);
      lib.generate(id);
    }
    calls.forEach((c) => c.resolve(response("<svg/>", 100)));
    await flush();
    expect(platform.vectorize).toHaveBeenCalledTimes(3);
    lib.setParam("a", "detail", 7);
    lib.setParam("b", "detail", 7);
    lib.setParam("c", "detail", 7);
    vi.advanceTimersByTime(DEBOUNCE_MS / 2);
    lib.cancel("a");
    lib.setSettings({ ...DEFAULT_SETTINGS, liveUpdate: false });
    vi.advanceTimersByTime(DEBOUNCE_MS * 4);
    expect(platform.vectorize).toHaveBeenCalledTimes(3);
    lib.setSettings(DEFAULT_SETTINGS);
    lib.setParam("c", "detail", 8);
    lib.remove("c");
    vi.advanceTimersByTime(DEBOUNCE_MS * 4);
    expect(platform.vectorize).toHaveBeenCalledTimes(3);
  });

  it("picking Auto again restores its pick's values", async () => {
    const { platform, calls } = fakePlatform();
    const lib = createLibrary(platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([ok("a")]);
    lib.generate("a");
    calls[0].resolve(
      response("<svg>logo</svg>", 900, {
        auto: { vexel: { engine: "vexel", pick: "logo", reason: "", candidates: [{ preset: "logo", label: "Logo & icon", svg: "<svg>logo</svg>", elapsed_ms: 900, stats: {} }] } },
      }),
    );
    await flush();
    lib.pickPreset("a", PRESETS[1]);
    expect(lib.getState().items[0].params).toEqual({ detail: 6, min_region: 8 });
    lib.pickPreset("a", PRESETS[0]);
    const item = lib.getState().items[0];
    expect(item.params).toEqual({ detail: 10, min_region: 16 });
    expect(item.shown).toBe("auto|");
  });

  it("counts the images whose vector on screen has not been exported or copied since it was traced", async () => {
    const { platform, calls } = fakePlatform();
    const lib = createLibrary(platform, CATALOG, { ...DEFAULT_SETTINGS, liveUpdate: false });
    lib.add([ok("a"), ok("b")]);
    expect(unexportedCount(lib.getState())).toBe(0);
    lib.pickPreset("a", PRESETS[1]);
    lib.generate("a");
    // a trace still running has nothing to export yet
    expect(unexportedCount(lib.getState())).toBe(0);
    calls[0].resolve(response("<svg>a</svg>", 100));
    await flush();
    const first = traceKey(lib.getState().items[0]);
    expect(lib.getState().items[0].exported).toBeNull();
    expect(unexportedCount(lib.getState())).toBe(1);
    lib.markExported([{ id: "a", key: first }, { id: "gone", key: "x" }]);
    expect(lib.getState().items[0].exported).toBe(first);
    expect(unexportedCount(lib.getState())).toBe(0);
    // traced again with other settings: the new vector is not exported
    lib.setParam("a", "detail", 9);
    lib.generate("a");
    calls[1].resolve(response("<svg>a9</svg>", 100));
    await flush();
    expect(unexportedCount(lib.getState())).toBe(1);
    // back to the settings that were exported: that vector was
    lib.pickPreset("a", PRESETS[1]);
    expect(lib.getState().items[0].shown).toBe(first);
    expect(unexportedCount(lib.getState())).toBe(0);
    // removing the image takes its count with it
    lib.setParam("a", "detail", 9);
    expect(unexportedCount(lib.getState())).toBe(1);
    lib.remove("a");
    expect(unexportedCount(lib.getState())).toBe(0);
  });

  it("a control left as it was keeps Auto and traces nothing", async () => {
    const { platform, calls } = fakePlatform();
    const lib = createLibrary(platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([ok("a")]);
    lib.generate("a");
    calls[0].resolve(response("<svg>auto</svg>", 100));
    await flush();
    const before = lib.getState();
    lib.setParam("a", "detail", 6); // the default it already has
    lib.setParam("a", "detail", "6");
    expect(lib.getState()).toBe(before);
    expect(lib.getState().items[0]).toMatchObject({ preset: "auto", shown: "auto|" });
    vi.advanceTimersByTime(DEBOUNCE_MS * 4);
    expect(platform.vectorize).toHaveBeenCalledTimes(1);
    lib.setParam("a", "detail", 7);
    expect(lib.getState().items[0].preset).toBeNull();
  });

  it("an error belongs to the settings that failed: other settings, cached or not, do not show it", async () => {
    const { platform, calls } = fakePlatform();
    const lib = createLibrary(platform, CATALOG, { ...DEFAULT_SETTINGS, liveUpdate: false });
    lib.add([ok("a")]);
    lib.pickPreset("a", PRESETS[1]);
    lib.generate("a");
    calls[0].resolve(response("<svg>balanced</svg>", 100));
    await flush();
    lib.setParam("a", "detail", 9);
    lib.generate("a");
    calls[1].reject(new ApiError("engine_crashed", "The worker crashed", 500));
    await flush();
    expect(errorOf(lib.getState().items[0])?.message).toBe("The worker crashed");
    // back to the cached settings: the good trace shows and the error is not on it
    lib.pickPreset("a", PRESETS[1]);
    const item = lib.getState().items[0];
    expect(item.shown).toBe(traceKey(item));
    expect(errorOf(item)).toBeNull();
    // and to settings nothing is known of
    lib.setParam("a", "detail", 11);
    expect(errorOf(lib.getState().items[0])).toBeNull();
    // the failed settings again: still the failure that is known of them
    lib.setParam("a", "detail", 9);
    expect(errorOf(lib.getState().items[0])?.code).toBe("engine_crashed");
  });

  it("a stale job's failure is not shown on the newer settings", async () => {
    const { platform, calls } = fakePlatform();
    const lib = createLibrary(platform, CATALOG, { ...DEFAULT_SETTINGS, liveUpdate: false });
    lib.add([ok("a")]);
    lib.generate("a"); // Auto, running
    lib.pickPreset("a", PRESETS[2]); // settings moved on, nothing cached for them
    calls[0].reject(new ApiError("engine_crashed", "The worker crashed", 500));
    await flush();
    const item = lib.getState().items[0];
    expect(item.job).toBeNull();
    expect(errorOf(item)).toBeNull();
    lib.pickPreset("a", PRESETS[0]);
    expect(errorOf(lib.getState().items[0])?.code).toBe("engine_crashed");
  });

  it("choosing a preset with a cached trace lets the running trace finish and keeps its answer", async () => {
    const { platform, calls } = fakePlatform();
    const lib = createLibrary(platform, CATALOG, { ...DEFAULT_SETTINGS, liveUpdate: false });
    lib.add([ok("a")]);
    lib.pickPreset("a", PRESETS[1]);
    lib.generate("a");
    calls[0].resolve(response("<svg>balanced</svg>", 100));
    await flush();
    lib.pickPreset("a", PRESETS[0]);
    lib.generate("a"); // Auto, running
    lib.pickPreset("a", PRESETS[1]); // the cached one
    const mid = lib.getState().items[0];
    expect(mid.shown).toBe(traceKey(mid));
    expect(calls[1].signal.aborted).toBe(false);
    expect(mid.job).toMatchObject({ key: "auto|" });
    calls[1].resolve(response("<svg>auto</svg>", 5000));
    await flush();
    const done = lib.getState().items[0];
    expect(done.job).toBeNull();
    expect(done.traces["auto|"].svg).toBe("<svg>auto</svg>");
    expect(done.preset).toBe("balanced");
    expect(done.shown).toBe(traceKey(done));
  });

  it("notifies subscribers and hands out a new state object on every change", () => {
    const { platform } = fakePlatform();
    const lib = createLibrary(platform, CATALOG, DEFAULT_SETTINGS);
    const seen: unknown[] = [];
    const off = lib.subscribe(() => seen.push(lib.getState()));
    lib.add([ok("a")]);
    lib.select("a");
    off();
    lib.add([ok("b")]);
    expect(seen).toHaveLength(2);
    expect(seen[0]).not.toBe(seen[1]);
  });
});

const DRIFT: Drift = { edgeF1: 0.59, deltaE: 3.8, verdict: "large" };
const RESULT: RedrawResult = { redraw: { ...image("r1"), width: 128, height: 64, previewUrl: "blob:r1" }, drift: DRIFT };

function redrawPlatform() {
  const base = fakePlatform();
  let finish: (r: RedrawResult) => void = () => {};
  let failWith: (e: unknown) => void = () => {};
  const signals: AbortSignal[] = [];
  const platform = {
    ...base.platform,
    imageRoughness: vi.fn(async () => ({ rough: true, reason: "small" as const })),
    redrawImage: vi.fn((_id: string, opts: { signal: AbortSignal; onPhase?: (p: RedrawPhase) => void }) => {
      signals.push(opts.signal);
      opts.onPhase?.("drawing");
      return new Promise<RedrawResult>((resolve, reject) => {
        finish = resolve;
        failWith = reject;
      });
    }),
    acceptRedraw: vi.fn(async (id: string) => ({ image: { ...image(id), previewUrl: "blob:redraw", width: 128, height: 64 }, original: { ...image(id), id: `${id}-original` } })),
    discardRedraw: vi.fn(async () => {}),
    revertRedraw: vi.fn(async (id: string) => image(id)),
  };
  return { calls: base.calls, platform, signals, finish: (r: RedrawResult) => finish(r), fail: (e: unknown) => failWith(e) };
}

describe("library: AI redraw", () => {
  it("asks once whether an image looks rough", async () => {
    const r = redrawPlatform();
    const lib = createLibrary(r.platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([ok("a")]);
    lib.checkRough("a");
    lib.checkRough("a");
    await flush();
    expect(r.platform.imageRoughness).toHaveBeenCalledTimes(1);
    expect(lib.getState().items[0].rough).toEqual({ rough: true, reason: "small" });
  });

  it("reports the redraw's phase, then keeps it waiting for a decision without touching the image", async () => {
    const r = redrawPlatform();
    const lib = createLibrary(r.platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([ok("a")]);
    const done = lib.redraw("a");
    expect(lib.getState().items[0].redraw).toMatchObject({ phase: "drawing", pending: null });
    r.finish(RESULT);
    await expect(done).resolves.toBeNull();
    expect(lib.getState().items[0].redraw).toMatchObject({ phase: null, pending: RESULT, active: false });
    expect(lib.getState().items[0].image.previewUrl).toBe("blob:a");
  });

  it("hands back no_key for the consent sheet and keeps no error for it", async () => {
    const r = redrawPlatform();
    const lib = createLibrary(r.platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([ok("a")]);
    const done = lib.redraw("a");
    r.fail(new ApiError("no_key", "Add your OpenAI API key to use AI redraw.", 401));
    expect((await done)?.code).toBe("no_key");
    expect(lib.getState().items[0].redraw).toMatchObject({ phase: null, error: null });
  });

  it("keeps a keychain refusal as an error and does not hand it back for the consent sheet", async () => {
    const r = redrawPlatform();
    const lib = createLibrary(r.platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([ok("a")]);
    const done = lib.redraw("a");
    r.fail(new ApiError("keychain", "macOS did not let Studi0Trace read the key.", 500));
    expect(await done).toBeNull();
    expect(lib.getState().items[0].redraw?.error?.code).toBe("keychain");
  });

  it("keeps other failures by code; a cancel is silent", async () => {
    const r = redrawPlatform();
    const lib = createLibrary(r.platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([ok("a")]);
    let done = lib.redraw("a");
    r.fail(new ApiError("quota", "Your OpenAI account is out of credit or rate limited.", 429));
    expect(await done).toBeNull();
    expect(lib.getState().items[0].redraw?.error?.code).toBe("quota");
    done = lib.redraw("a");
    lib.cancelRedraw("a");
    expect(r.signals.at(-1)!.aborted).toBe(true);
    r.fail(new ApiError("cancelled", "The redraw was cancelled", 499));
    await done;
    expect(lib.getState().items[0].redraw).toMatchObject({ phase: null, error: null });
  });

  it("Use redraw swaps the source, keeps the original's traces and traces again when Trace on open is set", async () => {
    const r = redrawPlatform();
    const lib = createLibrary(r.platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([ok("a")]);
    lib.generate("a");
    r.calls[0].resolve(response("<svg>original</svg>", 100));
    await flush();
    lib.setSettings({ ...DEFAULT_SETTINGS, traceOnOpen: true });
    const done = lib.redraw("a");
    r.finish(RESULT);
    await done;
    await lib.acceptRedraw("a");
    const item = lib.getState().items[0];
    expect(r.platform.acceptRedraw).toHaveBeenCalledWith("a");
    expect(item.image).toMatchObject({ id: "a", previewUrl: "blob:redraw", width: 128 });
    expect(item.redraw).toMatchObject({ active: true, pending: null, drift: DRIFT, original: { id: "a-original" } });
    expect(item.redraw?.kept?.traces["auto|"]?.svg).toBe("<svg>original</svg>");
    expect(item.shown).toBeNull();
    expect(r.platform.vectorize).toHaveBeenCalledTimes(2);
    expect(r.calls[1].req.imageId).toBe("a");
  });

  it("Revert brings the original back with its traces", async () => {
    const r = redrawPlatform();
    const lib = createLibrary(r.platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([ok("a")]);
    lib.generate("a");
    r.calls[0].resolve(response("<svg>original</svg>", 100));
    await flush();
    const done = lib.redraw("a");
    r.finish(RESULT);
    await done;
    await lib.acceptRedraw("a");
    await lib.revertRedraw("a");
    const item = lib.getState().items[0];
    expect(r.platform.revertRedraw).toHaveBeenCalledWith("a");
    expect(item.image.previewUrl).toBe("blob:a");
    expect(item.redraw).toEqual(NO_REDRAW);
    expect(item.shown).toBe("auto|");
    expect(item.traces["auto|"].svg).toBe("<svg>original</svg>");
    expect(item.rough).toBeUndefined();
  });

  it("a second Use redraw or Revert while the first is in flight is ignored", async () => {
    const r = redrawPlatform();
    const lib = createLibrary(r.platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([ok("a")]);
    const done = lib.redraw("a");
    r.finish(RESULT);
    await done;
    const first = lib.acceptRedraw("a");
    const second = lib.acceptRedraw("a"); // a double click
    await Promise.all([first, second]);
    expect(r.platform.acceptRedraw).toHaveBeenCalledTimes(1);
    expect(lib.getState().items[0].redraw).toMatchObject({ active: true, pending: null });
    const reverting = lib.revertRedraw("a");
    const again = lib.revertRedraw("a");
    await Promise.all([reverting, again]);
    expect(r.platform.revertRedraw).toHaveBeenCalledTimes(1);
    expect(lib.getState().items[0].redraw).toEqual(NO_REDRAW);
  });

  it("a failed decision can be tried again", async () => {
    const r = redrawPlatform();
    const lib = createLibrary(r.platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([ok("a")]);
    const done = lib.redraw("a");
    r.finish(RESULT);
    await done;
    r.platform.acceptRedraw.mockImplementationOnce(async () => {
      throw new ApiError("no_redraw", "x", 409);
    });
    await expect(lib.acceptRedraw("a")).rejects.toBeInstanceOf(ApiError);
    await lib.acceptRedraw("a");
    expect(r.platform.acceptRedraw).toHaveBeenCalledTimes(2);
  });

  it("Revert during a Try again redraw abandons it first, and lets a waiting one go", async () => {
    const r = redrawPlatform();
    const lib = createLibrary(r.platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([ok("a")]);
    let done = lib.redraw("a");
    r.finish(RESULT);
    await done;
    await lib.acceptRedraw("a");
    done = lib.redraw("a"); // a second redraw, running
    expect(lib.getState().items[0].redraw).toMatchObject({ active: true, phase: "drawing" });
    await lib.revertRedraw("a");
    expect(r.signals.at(-1)!.aborted).toBe(true);
    expect(lib.getState().items[0].redraw).toEqual(NO_REDRAW);
    r.fail(new ApiError("cancelled", "The redraw was cancelled", 499)); // the abandoned call ends
    await done;
    expect(lib.getState().items[0].redraw).toEqual(NO_REDRAW);
    // a redraw that finished and waits is let go as well
    done = lib.redraw("a");
    r.finish(RESULT);
    await done;
    await lib.acceptRedraw("a");
    done = lib.redraw("a");
    r.finish(RESULT);
    await done;
    expect(lib.getState().items[0].redraw?.pending).not.toBeNull();
    await lib.revertRedraw("a");
    expect(r.platform.discardRedraw).toHaveBeenCalledWith("a");
    expect(lib.getState().items[0].redraw).toEqual(NO_REDRAW);
  });

  it("Revert clears the waiting decision before the app answers, so a failed revert leaves no Drift check", async () => {
    const r = redrawPlatform();
    const lib = createLibrary(r.platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([ok("a")]);
    let done = lib.redraw("a");
    r.finish(RESULT);
    await done;
    await lib.acceptRedraw("a");
    done = lib.redraw("a");
    r.finish(RESULT);
    await done;
    expect(lib.getState().items[0].redraw?.pending).not.toBeNull();
    let fail: (e: unknown) => void = () => {};
    r.platform.revertRedraw.mockImplementationOnce(() => new Promise((_, reject) => (fail = reject)));
    const reverting = lib.revertRedraw("a");
    expect(lib.getState().items[0].redraw).toMatchObject({ active: true, pending: null, phase: null }); // before the app answers
    fail(new ApiError("not_redrawn", "x", 409));
    await expect(reverting).rejects.toMatchObject({ code: "not_redrawn" });
    expect(lib.getState().items[0].redraw).toMatchObject({ active: true, pending: null });
  });

  it("Discard drops the waiting redraw; Try again replaces it", async () => {
    const r = redrawPlatform();
    const lib = createLibrary(r.platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([ok("a")]);
    let done = lib.redraw("a");
    r.finish(RESULT);
    await done;
    lib.discardRedraw("a");
    expect(r.platform.discardRedraw).toHaveBeenCalledWith("a");
    expect(lib.getState().items[0].redraw?.pending).toBeNull();
    expect(lib.getState().items[0].image.previewUrl).toBe("blob:a");
    done = lib.redraw("a");
    r.finish(RESULT);
    await done;
    done = lib.redraw("a"); // Try again
    expect(lib.getState().items[0].redraw?.pending).toBeNull();
    r.finish({ ...RESULT, drift: { ...DRIFT, verdict: "close" } });
    await done;
    expect(lib.getState().items[0].redraw?.pending?.drift.verdict).toBe("close");
  });

  it("removing an image abandons its redraw", () => {
    const r = redrawPlatform();
    const lib = createLibrary(r.platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([ok("a")]);
    void lib.redraw("a");
    lib.remove("a");
    expect(r.signals[0].aborted).toBe(true);
  });

  it("keeps not_allowed and engine_crashed by code, with their words", async () => {
    const r = redrawPlatform();
    const lib = createLibrary(r.platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([ok("a")]);
    let done = lib.redraw("a");
    r.fail(new ApiError("not_allowed", "OpenAI did not allow this key to create images.", 403));
    expect(await done).toBeNull();
    expect(lib.getState().items[0].redraw?.error).toMatchObject({ code: "not_allowed", status: 403 });
    done = lib.redraw("a");
    expect(lib.getState().items[0].redraw?.error).toBeNull();
    r.fail(new ApiError("engine_crashed", "the redraw stopped unexpectedly", 500));
    await done;
    expect(lib.getState().items[0].redraw?.error?.code).toBe("engine_crashed");
  });

  it("ignores phase events once the redraw is not running, and terminal phases never show", async () => {
    const base = fakePlatform();
    let emit: (p: RedrawPhase) => void = () => {};
    let finish: (r: RedrawResult) => void = () => {};
    const platform = {
      ...base.platform,
      redrawImage: vi.fn((_id: string, opts: { signal: AbortSignal; onPhase?: (p: RedrawPhase) => void }) => {
        emit = (p) => opts.onPhase?.(p);
        return new Promise<RedrawResult>((resolve) => (finish = resolve));
      }),
    };
    const lib = createLibrary(platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([ok("a")]);
    const done = lib.redraw("a");
    emit("drawing");
    expect(lib.getState().items[0].redraw?.phase).toBe("drawing");
    emit("failed");
    expect(lib.getState().items[0].redraw?.phase).toBe("drawing");
    lib.cancelRedraw("a");
    emit("checking");
    expect(lib.getState().items[0].redraw?.phase).toBeNull();
    finish(RESULT);
    await done;
    expect(lib.getState().items[0].redraw).toMatchObject({ phase: null, pending: null });
  });

  it("a redraw superseded by Try again is silent and does not touch the new one", async () => {
    const base = fakePlatform();
    const jobs: { resolve: (r: RedrawResult) => void; reject: (e: unknown) => void }[] = [];
    const platform = {
      ...base.platform,
      redrawImage: vi.fn((_id: string, opts: { onPhase?: (p: RedrawPhase) => void }) => {
        opts.onPhase?.("drawing");
        return new Promise<RedrawResult>((resolve, reject) => jobs.push({ resolve, reject }));
      }),
    };
    const lib = createLibrary(platform, CATALOG, DEFAULT_SETTINGS);
    lib.add([ok("a")]);
    const first = lib.redraw("a");
    const second = lib.redraw("a");
    jobs[0].reject(new ApiError("cancelled", "The redraw was cancelled", 499));
    expect(await first).toBeNull();
    expect(lib.getState().items[0].redraw).toMatchObject({ phase: "drawing", error: null });
    jobs[1].resolve(RESULT);
    await second;
    expect(lib.getState().items[0].redraw?.pending).toEqual(RESULT);
  });
});

const PLAN = (lines: TextLine[]): TextPlan => ({ image: "a", reader: "vision", lines, aiError: null, fontsOffline: false });
const LINE: TextLine = { id: "a-t0", quad: [[0, 0], [10, 0], [10, 5], [0, 5]], string: "EST", class: "type", colour: [0, 0, 0], status: { kind: "matched" }, candidates: [{ font: { family: "Montserrat", weight: 400, italic: false, source: "google" }, iou: 0.9, stroke: 1, fit: { size: 10, baseline: 5, x: 0, tracking: 0, nudges: [], iou: 0.9, stroke: 1 }, path: "M0 0h1", text: "<text/>" }], chosen: 0, enabled: true, fit: { size: 10, baseline: 5, x: 0, tracking: 0, nudges: [], iou: 0.9, stroke: 1 }, occluders: [] };

/** A platform whose traces answer at once, with the text calls and the redraw calls left to each test. */
function textPlatform() {
  const order: string[] = [];
  const platform = {
    vectorize: vi.fn(async () => {
      order.push("trace");
      return response("<svg></svg>", 100);
    }),
    closeImage: vi.fn(),
    openPaths: vi.fn(async () => []),
    textPlan: vi.fn(async (): Promise<TextPlan> => {
      order.push("plan");
      return PLAN([LINE]);
    }),
    textSetLine: vi.fn(async (_id: string, _line: string, change: TextLineChange) => ({ plan: PLAN([{ ...LINE, string: change.string ?? LINE.string }]) })),
    redrawImage: vi.fn(async () => RESULT),
    acceptRedraw: vi.fn(async (id: string) => ({ image: image(id), original: image(`${id}-original`) })),
  } satisfies LibraryPlatform;
  return { platform, order };
}

describe("library: text", () => {
  beforeEach(() => vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] }));
  afterEach(() => vi.useRealTimers());

  it("asks for the plan before the first trace", async () => {
    const { platform, order } = textPlatform();
    const lib = createLibrary(platform, CATALOG, { ...DEFAULT_SETTINGS, recogniseText: true });
    lib.add([ok("a")]);
    lib.generate("a");
    await flush();
    expect(order).toEqual(["plan", "trace"]);
    const item = lib.selectedItem()!;
    expect(textOf(item).plan?.lines).toHaveLength(1);
    // composing the plan's glyphs into the trace is Task 5 of the text-toggles plan
    expect(shownAnswer(item)?.base).toBe("<svg></svg>");
  });

  it("does not ask when Recognise text is off, and traces the original", async () => {
    const { platform } = textPlatform();
    const lib = createLibrary(platform, CATALOG, { ...DEFAULT_SETTINGS, recogniseText: false });
    lib.add([ok("a")]);
    lib.generate("a");
    await flush();
    expect(platform.textPlan).not.toHaveBeenCalled();
    expect(lib.selectedItem()!.shown).not.toBeNull();
  });

  it("a plan failure is kept on the item and the trace still runs", async () => {
    const { platform } = textPlatform();
    platform.textPlan.mockImplementation(async () => Promise.reject(new ApiError("text_unavailable", "no", 501)));
    const lib = createLibrary(platform, CATALOG, { ...DEFAULT_SETTINGS, recogniseText: true });
    lib.add([ok("a")]);
    lib.generate("a");
    await flush();
    expect(textOf(lib.selectedItem()!).error?.code).toBe("text_unavailable");
    expect(lib.selectedItem()!.shown).not.toBeNull();
  });

  it("a string edit answers with the plan and does not trace", async () => {
    const { platform } = textPlatform();
    const lib = createLibrary(platform, CATALOG, { ...DEFAULT_SETTINGS, recogniseText: true });
    lib.add([ok("a")]);
    lib.generate("a");
    await flush();
    await lib.setTextLine("a", "a-t0", { string: "EST." });
    expect(platform.vectorize).toHaveBeenCalledTimes(1);
    expect(textOf(lib.selectedItem()!).plan?.lines[0].string).toBe("EST.");
    expect(platform.textPlan).toHaveBeenCalledTimes(1);
  });

  it("using a redraw forgets the plan so it is read again", async () => {
    const { platform } = textPlatform();
    const lib = createLibrary(platform, CATALOG, { ...DEFAULT_SETTINGS, recogniseText: true });
    lib.add([ok("a")]);
    lib.generate("a");
    await flush();
    expect(platform.textPlan).toHaveBeenCalledTimes(1);
    await lib.redraw("a");
    await lib.acceptRedraw("a");
    expect(textOf(lib.selectedItem()!).plan).toBeNull();
    lib.generate("a");
    await flush();
    expect(platform.textPlan).toHaveBeenCalledTimes(2);
  });

  it("a plan is asked once per image bytes, however the settings move", async () => {
    const { platform } = textPlatform();
    const lib = createLibrary(platform, CATALOG, { ...DEFAULT_SETTINGS, recogniseText: true });
    lib.add([ok("a")]);
    lib.generate("a");
    await flush();
    lib.setParam("a", "detail", 9); // another key: generate reaches the plan check again
    lib.generate("a");
    await flush();
    // the trace for detail 9, then (off Auto) the untouched image prefetched: no plan asked for either
    expect(platform.vectorize).toHaveBeenCalledTimes(3);
    expect(platform.textPlan).toHaveBeenCalledTimes(1);
  });

  it("cancelling the plan ends its phase and traces nothing on its account", async () => {
    const { platform } = textPlatform();
    let signal: AbortSignal | undefined;
    platform.textPlan.mockImplementation(((_id: string, opts: { signal: AbortSignal }) => {
      signal = opts.signal;
      return new Promise<TextPlan>((_, reject) => signal!.addEventListener("abort", () => reject(new ApiError("cancelled", "x", 499))));
    }) as never);
    const lib = createLibrary(platform, CATALOG, { ...DEFAULT_SETTINGS, recogniseText: true });
    lib.add([ok("a")]);
    lib.generate("a");
    expect(textOf(lib.selectedItem()!)).toMatchObject({ asked: true, phase: "finding" });
    lib.cancelText("a");
    await flush();
    expect(signal?.aborted).toBe(true);
    expect(textOf(lib.selectedItem()!)).toMatchObject({ phase: null, error: null, plan: null, asked: false });
    expect(lib.selectedItem()!.job).toBeNull();
    expect(platform.vectorize).not.toHaveBeenCalled();
    // a cancelled plan was never answered: the next trace asks again
    platform.textPlan.mockImplementation(async () => PLAN([LINE]));
    lib.generate("a");
    await flush();
    expect(platform.textPlan).toHaveBeenCalledTimes(2);
    expect(platform.vectorize).toHaveBeenCalledTimes(1);
  });

  it("while the plan runs the image holds a text job; the trace job replaces it; Cancel on the trace button stops the plan", async () => {
    const { platform } = textPlatform();
    let resolvePlan: (p: TextPlan) => void = () => {};
    let signal: AbortSignal | undefined;
    platform.textPlan.mockImplementation(((_id: string, opts: { signal: AbortSignal }) => {
      signal = opts.signal;
      return new Promise<TextPlan>((r, reject) => {
        resolvePlan = r;
        signal!.addEventListener("abort", () => reject(new ApiError("cancelled", "x", 499)));
      });
    }) as never);
    const lib = createLibrary(platform, CATALOG, { ...DEFAULT_SETTINGS, recogniseText: true });
    lib.add([ok("a")]);
    lib.generate("a");
    const job = lib.selectedItem()!.job!;
    expect(job).toMatchObject({ key: "auto|", phase: "text" });
    // asking again while it reads starts nothing more
    lib.generate("a");
    expect(platform.textPlan).toHaveBeenCalledTimes(1);
    resolvePlan(PLAN([LINE]));
    await flush();
    expect(platform.vectorize).toHaveBeenCalledTimes(1);
    expect(lib.selectedItem()!.job).toBeNull(); // the fake trace answered at once
    expect(lib.selectedItem()!.shown).toBe("auto|a-t0");
    // a second image: the trace button's Cancel (lib.cancel) during the plan stops the plan and ends the job
    lib.add([ok("b")]);
    lib.generate("b");
    expect(lib.selectedItem()!.job?.phase).toBe("text");
    lib.cancel("b");
    await flush();
    expect(signal?.aborted).toBe(true);
    expect(lib.selectedItem()!.job).toBeNull();
    expect(textOf(lib.selectedItem()!)).toMatchObject({ asked: false, phase: null });
    expect(platform.vectorize).toHaveBeenCalledTimes(1);
  });

  it("Try again after a failed plan asks once more and traces again when the plan re-sets a line", async () => {
    const { platform } = textPlatform();
    platform.textPlan.mockImplementationOnce(async () => Promise.reject(new ApiError("text_failed", "no", 500)));
    const lib = createLibrary(platform, CATALOG, { ...DEFAULT_SETTINGS, recogniseText: true });
    lib.add([ok("a")]);
    lib.generate("a");
    await flush();
    expect(textOf(lib.selectedItem()!).error?.code).toBe("text_failed");
    expect(platform.vectorize).toHaveBeenCalledTimes(1);
    lib.retryText("a");
    expect(textOf(lib.selectedItem()!)).toMatchObject({ error: null, asked: true });
    await flush();
    expect(platform.textPlan).toHaveBeenCalledTimes(2);
    expect(textOf(lib.selectedItem()!).plan?.lines).toHaveLength(1);
    // the trace made without the plan held the text as shapes: it is traced again
    expect(platform.vectorize).toHaveBeenCalledTimes(2);
  });

  it("a plan superseded by a used redraw does not trace the new bytes unasked", async () => {
    const { platform } = textPlatform();
    let resolvePlan: (p: TextPlan) => void = () => {};
    platform.textPlan.mockImplementation((() => new Promise<TextPlan>((r) => (resolvePlan = r))) as never);
    const lib = createLibrary(platform, CATALOG, { ...DEFAULT_SETTINGS, recogniseText: true });
    lib.add([ok("a")]);
    lib.generate("a");
    await lib.redraw("a");
    await lib.acceptRedraw("a");
    resolvePlan(PLAN([LINE]));
    await flush();
    expect(platform.vectorize).not.toHaveBeenCalled();
    expect(textOf(lib.selectedItem()!).plan).toBeNull();
  });

  it("edits to one image are applied in order and none is lost", async () => {
    const { platform } = textPlatform();
    const lib = createLibrary(platform, CATALOG, { ...DEFAULT_SETTINGS, recogniseText: true });
    lib.add([ok("a")]);
    lib.generate("a");
    await flush();
    const seen: (string | undefined)[] = [];
    const releases: (() => void)[] = [];
    platform.textSetLine.mockImplementation((async (_id: string, _line: string, change: TextLineChange) => {
      seen.push(change.string);
      await new Promise<void>((r) => releases.push(r));
      return { plan: PLAN([{ ...LINE, string: change.string! }]) };
    }) as never);
    const first = lib.setTextLine("a", "a-t0", { string: "ONE" });
    const second = lib.setTextLine("a", "a-t0", { string: "TWO" });
    await flush();
    expect(seen).toEqual(["ONE"]); // the second waits for the first
    releases[0]();
    await flush();
    expect(seen).toEqual(["ONE", "TWO"]);
    releases[1]();
    await Promise.all([first, second]);
    expect(textOf(lib.selectedItem()!).plan?.lines[0].string).toBe("TWO");
  });

  it("a failed edit does not stop the one behind it", async () => {
    const { platform } = textPlatform();
    const lib = createLibrary(platform, CATALOG, { ...DEFAULT_SETTINGS, recogniseText: true });
    lib.add([ok("a")]);
    lib.generate("a");
    await flush();
    platform.textSetLine.mockImplementationOnce((async () => Promise.reject(new ApiError("bad", "x", 500))) as never);
    const first = lib.setTextLine("a", "a-t0", { string: "ONE" });
    const second = lib.setTextLine("a", "a-t0", { string: "TWO" });
    await expect(first).rejects.toThrow();
    await second;
    expect(platform.textSetLine).toHaveBeenCalledTimes(2);
  });

  it("a string edit makes the trace on screen unexported again", async () => {
    const { platform } = textPlatform();
    const lib = createLibrary(platform, CATALOG, { ...DEFAULT_SETTINGS, recogniseText: true });
    lib.add([ok("a")]);
    lib.generate("a");
    await flush();
    lib.markExported([{ id: "a", key: lib.selectedItem()!.shown! }]);
    expect(unexportedCount(lib.getState())).toBe(0);
    await lib.setTextLine("a", "a-t0", { string: "EST." });
    expect(unexportedCount(lib.getState())).toBe(1);
  });

  it("an edit answered after a redraw was used is dropped", async () => {
    const { platform } = textPlatform();
    const lib = createLibrary(platform, CATALOG, { ...DEFAULT_SETTINGS, recogniseText: true, traceOnOpen: false });
    lib.add([ok("a")]);
    lib.generate("a");
    await flush();
    let release: () => void = () => {};
    platform.textSetLine.mockImplementationOnce((async () => {
      await new Promise<void>((r) => (release = r));
      return { plan: PLAN([{ ...LINE, string: "LATE" }]) };
    }) as never);
    const edit = lib.setTextLine("a", "a-t0", { string: "LATE" });
    await flush();
    await lib.redraw("a");
    await lib.acceptRedraw("a");
    const calls = platform.vectorize.mock.calls.length;
    release();
    await edit;
    await flush();
    const item = lib.selectedItem()!;
    expect(textOf(item).plan).toBeNull();
    expect(platform.vectorize.mock.calls.length).toBe(calls);
  });
});

const FONT_M = { family: "Montserrat", weight: 400, italic: false, source: "google" } as const;
const FONT_L = { family: "Lato", weight: 400, italic: false, source: "installed" } as const;
const FONT_O = { family: "Oswald", weight: 500, italic: false, source: "google" } as const;
const candidate = (font: TextLine["candidates"][number]["font"], path: string, id: string, word: string) => ({ font, iou: 0.9, stroke: 1, fit: LINE.fit!, path, text: `<text x="0" y="10" fill="none" data-line="${id}">${word}</text>` });
const T0: TextLine = { ...LINE, id: "a-t0", quad: [[0, 0], [30, 0], [30, 10], [0, 10]], string: "EST", candidates: [candidate(FONT_M, "M0 0h10v10z", "a-t0", "EST"), candidate(FONT_L, "M0 0h20v10z", "a-t0", "EST")], chosen: 0 };
const T1: TextLine = { ...LINE, id: "a-t1", quad: [[0, 20], [30, 20], [30, 30], [0, 30]], string: "2026", candidates: [candidate(FONT_M, "M0 20h10v10z", "a-t1", "2026")], chosen: 0 };
const BOTH = ["a-t0", "a-t1"];
/** The engine's answer for a mask: a red square, tagged with the mask so a test can tell the traces apart. */
const baseFor = (mask: string[]) => `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 32"><path d="M40 0h10v10h-10z" fill="#ff0000" id="m${mask.join("_")}"/></svg>`;
const P = paramsKey({ detail: 6, min_region: 8 });
const TEXT_ON = { ...DEFAULT_SETTINGS, recogniseText: true, hiddenText: false, liveUpdate: false };

/** Traces that wait to be answered, with a plan and a line edit the test controls. */
function maskPlatform(plan: TextPlan) {
  const base = fakePlatform();
  const platform = {
    ...base.platform,
    textPlan: vi.fn(async () => plan),
    textSetLine: vi.fn(async (_id: string, _line: string, _change: TextLineChange) => ({ plan })),
    redrawImage: vi.fn(async () => RESULT),
    acceptRedraw: vi.fn(async (id: string) => ({ image: image(id), original: image(`${id}-original`) })),
    discardRedraw: vi.fn(async () => {}),
    revertRedraw: vi.fn(async (id: string) => image(id)),
  } satisfies LibraryPlatform;
  const answer = async (call: Call, elapsed = 500) => {
    call.resolve(response(baseFor(call.req.mask), elapsed));
    await flush();
  };
  return { platform, calls: base.calls, answer };
}

/** An image on Balanced (prefetching never runs under Auto), its plan read and its first trace asked for. */
async function tracedOnBalanced(plan: TextPlan = PLAN([T0, T1]), settings = TEXT_ON) {
  const p = maskPlatform(plan);
  const lib = createLibrary(p.platform, CATALOG, settings);
  lib.add([ok("a")]);
  lib.pickPreset("a", PRESETS[1]);
  lib.generate("a");
  await flush();
  return { ...p, lib, item: () => lib.selectedItem()! };
}

describe("library: traces by the painted set", () => {
  beforeEach(() => vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] }));
  afterEach(() => vi.useRealTimers());

  it("keys the first trace by params and the full mask, then prefetches the empty mask and all-but-one masks in order", async () => {
    const { calls, answer, item } = await tracedOnBalanced();
    expect(calls.map((c) => c.req.mask)).toEqual([BOTH]);
    expect(item().job).toMatchObject({ key: `${P}|a-t0,a-t1`, mask: BOTH, prefetch: false });
    expect(traceKey(item())).toBe(`${P}|a-t0,a-t1`);
    expect(traceKeyFor(item(), [])).toBe(`${P}|`);
    expect(maskOf(textOf(item()))).toEqual(BOTH);
    expect(fullMask(textOf(item()).plan!)).toEqual(BOTH);
    await answer(calls[0]);
    expect(item().shown).toBe(`${P}|a-t0,a-t1`);
    // the trace on screen is its base with both lines set in their fonts
    expect(shownAnswer(item())!.base).toBe(baseFor(BOTH));
    expect(shownAnswer(item())!.svg).toContain('data-line="a-t0"');
    expect(shownAnswer(item())!.svg).toContain('data-line="a-t1"');
    // then the untouched image, in the background: no busy state
    expect(calls).toHaveLength(2);
    expect(calls[1].req).toMatchObject({ mask: [], auto: false, parameters: { detail: 6, min_region: 8 } });
    expect(item().job).toMatchObject({ mask: [], prefetch: true });
    expect(userJob(item())).toBeNull();
    expect(currentJob(item())).toBeNull();
    await answer(calls[1]);
    expect(calls[2].req.mask).toEqual(["a-t1"]);
    await answer(calls[2]);
    expect(calls[3].req.mask).toEqual(["a-t0"]);
    expect(item().job?.prefetch).toBe(true);
    await answer(calls[3]);
    expect(calls).toHaveLength(4);
    const done = item();
    expect(done.job).toBeNull();
    expect(Object.keys(done.traces).sort()).toEqual([`${P}|`, `${P}|a-t0`, `${P}|a-t0,a-t1`, `${P}|a-t1`]);
    // each trace sets the lines its own mask painted out, and only those
    expect(done.traces[`${P}|`].svg).toBe(baseFor([]));
    expect(done.traces[`${P}|a-t1`].svg).toContain('data-line="a-t1"');
    expect(done.traces[`${P}|a-t1`].svg).not.toContain('data-line="a-t0"');
    expect(done.shown).toBe(`${P}|a-t0,a-t1`);
  });

  it("on Auto, the masks after the run are traced under the pick's params: a toggle and the prefetch, never an Auto run", async () => {
    const p = maskPlatform(PLAN([T0, T1]));
    const lib = createLibrary(p.platform, CATALOG, TEXT_ON);
    const item = () => lib.selectedItem()!;
    const L = paramsKey({ detail: 10, min_region: 16 });
    const logo = { detail: 10, min_region: 16 };
    lib.add([ok("a")]);
    lib.generate("a");
    await flush();
    expect(p.calls[0].req).toMatchObject({ auto: true, mask: BOTH });
    expect(traceKey(item())).toBe("auto|a-t0,a-t1");
    // before the pick is known, another mask could only be an Auto run of its own
    expect(traceKeyFor(item(), ["a-t0"])).toBe("auto|a-t0");
    p.calls[0].resolve(
      response(baseFor(BOTH), 900, {
        auto: {
          vexel: {
            engine: "vexel",
            pick: "logo",
            reason: "",
            candidates: [
              { preset: "balanced", label: "Balanced", svg: baseFor(["balanced"]), elapsed_ms: 800, stats: {} },
              { preset: "logo", label: "Logo & icon", svg: baseFor(BOTH), elapsed_ms: 900, stats: {} },
            ],
          },
        },
      }),
    );
    await flush();
    expect(item().shown).toBe("auto|a-t0,a-t1");
    expect(item().params).toEqual(logo);
    expect(traceKeyFor(item(), ["a-t0"])).toBe(`${L}|a-t0`);
    // the prefetch: the empty mask, then each all-but-one, under the pick's params
    expect(p.calls[1].req).toMatchObject({ auto: false, mask: [], parameters: logo });
    expect(item().job).toMatchObject({ key: `${L}|`, prefetch: true });
    await p.answer(p.calls[1]);
    expect(p.calls[2].req).toMatchObject({ auto: false, mask: ["a-t1"], parameters: logo });
    // a toggle, no preset picked: one trace, of the pick's params and the new mask, not an Auto run
    const before = p.calls.length;
    lib.toggleTextLine("a", "a-t1", false);
    expect(p.calls).toHaveLength(before + 1);
    expect(p.calls[before].req).toMatchObject({ auto: false, mask: ["a-t0"], parameters: logo });
    expect(currentJob(item())).toMatchObject({ key: `${L}|a-t0` });
    await p.answer(p.calls[before]);
    expect(item().shown).toBe(`${L}|a-t0`);
    expect(item().preset).toBe("auto");
    // the untouched image is cached under the pick's params, and the full mask is Auto's own answer
    lib.toggleTextLine("a", "a-t0", false);
    expect(item().shown).toBe(`${L}|`);
    lib.toggleTextLine("a", "a-t0", true);
    lib.toggleTextLine("a", "a-t1", true);
    expect(item().shown).toBe("auto|a-t0,a-t1");
    expect(p.calls.slice(before + 1).every((c) => !c.req.auto)).toBe(true);
  });

  it("a prefetch is aborted by a parameter change and never runs on Auto before the pick is known", async () => {
    // Auto: the full mask only
    const auto = maskPlatform(PLAN([T0, T1]));
    const onAuto = createLibrary(auto.platform, CATALOG, TEXT_ON);
    onAuto.add([ok("a")]);
    onAuto.generate("a");
    await flush();
    expect(auto.calls[0].req).toMatchObject({ auto: true, mask: BOTH });
    await auto.answer(auto.calls[0]);
    expect(onAuto.selectedItem()!.shown).toBe("auto|a-t0,a-t1");
    expect(auto.calls).toHaveLength(1);
    expect(onAuto.selectedItem()!.job).toBeNull();

    const { calls, answer, item, lib } = await tracedOnBalanced();
    await answer(calls[0]);
    expect(calls[1].req.mask).toEqual([]);
    lib.setParam("a", "detail", 9);
    expect(calls[1].signal.aborted).toBe(true);
    expect(item().job).toBeNull();
    calls[1].reject(new ApiError("cancelled", "Cancelled", 499));
    await flush();
    vi.advanceTimersByTime(DEBOUNCE_MS * 4);
    await flush();
    expect(calls).toHaveLength(2);
    expect(item().error).toBeNull();
    // the next user trace prefetches again, under the new settings
    lib.generate("a");
    expect(calls[2].req).toMatchObject({ mask: BOTH, parameters: { detail: 9, min_region: 8 } });
    await answer(calls[2]);
    expect(calls[3].req).toMatchObject({ mask: [], parameters: { detail: 9, min_region: 8 } });
  });

  it("toggling a line shows the cached trace for its mask instantly, and traces on demand when it is not cached", async () => {
    const { calls, answer, item, lib } = await tracedOnBalanced();
    await answer(calls[0]);
    await answer(calls[1]); // the empty mask
    expect(calls[2]).toMatchObject({ req: { mask: ["a-t1"] } }); // a prefetch, running
    // a-t1 off: nothing known of [a-t0], so it is traced as a user trace; the screen keeps what it shows meanwhile
    lib.toggleTextLine("a", "a-t1", false);
    expect([...textOf(item()).enabled]).toEqual(["a-t0"]);
    expect(calls[2].signal.aborted).toBe(true);
    expect(calls[3].req.mask).toEqual(["a-t0"]);
    expect(item().job).toMatchObject({ key: `${P}|a-t0`, prefetch: false });
    expect(currentJob(item())).not.toBeNull();
    expect(item().shown).toBe(`${P}|a-t0,a-t1`);
    // a-t0 off too: the untouched image is cached and shows at once, and the trace for [a-t0] runs on
    lib.toggleTextLine("a", "a-t0", false);
    expect(item().shown).toBe(`${P}|`);
    expect(calls).toHaveLength(4);
    expect(calls[3].signal.aborted).toBe(false);
    await answer(calls[3]);
    expect(item().traces[`${P}|a-t0`]).toBeDefined();
    expect(item().shown).toBe(`${P}|`);
    // and a-t0 back on shows it without a request
    const before = calls.length;
    lib.toggleTextLine("a", "a-t0", true);
    expect(item().shown).toBe(`${P}|a-t0`);
    expect(shownAnswer(item())!.svg).toContain('data-line="a-t0"');
    expect(calls.slice(before).every((c) => c.req.mask.join() !== "a-t0")).toBe(true);
  });

  it("toggling while that mask's trace is in flight does not request it twice and lands under its key", async () => {
    const { calls, answer, item, lib } = await tracedOnBalanced();
    await answer(calls[0]);
    expect(calls[1].req.mask).toEqual([]); // the empty mask, prefetching
    lib.toggleTextLine("a", "a-t0", false); // [a-t1]: traced on demand
    expect(calls[2].req.mask).toEqual(["a-t1"]);
    lib.toggleTextLine("a", "a-t0", true); // back: cached, shown, and the trace for [a-t1] runs on
    expect(item().shown).toBe(`${P}|a-t0,a-t1`);
    expect(calls[2].signal.aborted).toBe(false);
    expect(currentJob(item())).toBeNull();
    lib.toggleTextLine("a", "a-t0", false); // and off again: the trace in flight is the one
    expect(calls).toHaveLength(3);
    expect(currentJob(item())).toMatchObject({ key: `${P}|a-t1` });
    expect([...textOf(item()).enabled]).toEqual(["a-t1"]);
    await answer(calls[2]);
    expect(item().shown).toBe(`${P}|a-t1`);
    expect(shownAnswer(item())!.base).toBe(baseFor(["a-t1"]));
    // the prefetch picks up again; switching to the mask it is tracing makes it the user's, not a second request
    expect(calls[3]).toMatchObject({ req: { mask: [] } });
    expect(item().job?.prefetch).toBe(true);
    lib.toggleTextLine("a", "a-t1", false);
    expect(calls).toHaveLength(4);
    expect(calls[3].signal.aborted).toBe(false);
    expect(currentJob(item())).toMatchObject({ key: `${P}|`, prefetch: false });
    await answer(calls[3]);
    expect(item().shown).toBe(`${P}|`);
  });

  it("a font among the candidates swaps locally and tells the app in the background; a search pick round-trips", async () => {
    const { calls, answer, item, lib, platform } = await tracedOnBalanced();
    await answer(calls[0]);
    expect(shownAnswer(item())!.svg).toContain("M0 0h10v10z");
    let release: () => void = () => {};
    platform.textSetLine.mockImplementationOnce((async () => {
      await new Promise<void>((r) => (release = r));
      return { plan: PLAN([{ ...T0, string: "NOT USED", chosen: 1 }, T1]) };
    }) as never);
    void lib.setTextLine("a", "a-t0", { font: FONT_L });
    // at once, before the app has answered
    expect(textOf(item()).plan!.lines[0].chosen).toBe(1);
    expect(shownAnswer(item())!.svg).toContain("M0 0h20v10z");
    expect(shownAnswer(item())!.svg).not.toContain("M0 0h10v10z");
    await flush();
    expect(platform.textSetLine).toHaveBeenCalledWith("a", "a-t0", { font: FONT_L });
    release();
    await flush();
    // the answer adds no candidate, so it is not merged
    expect(textOf(item()).plan!.lines[0].string).toBe("EST");
    expect(calls).toHaveLength(2); // nothing traced; the prefetch runs on
    expect(calls[1].signal.aborted).toBe(false);
    // a font from search is fitted by the app first
    platform.textSetLine.mockImplementationOnce(async () => ({ plan: PLAN([{ ...T0, candidates: [...T0.candidates, candidate(FONT_O, "M0 0h30v10z", "a-t0", "EST")], chosen: 2 }, T1]) }));
    const pick = lib.setTextLine("a", "a-t0", { font: FONT_O });
    expect(shownAnswer(item())!.svg).toContain("M0 0h20v10z");
    await pick;
    expect(platform.textSetLine).toHaveBeenLastCalledWith("a", "a-t0", { font: FONT_O });
    expect(textOf(item()).plan!.lines[0].chosen).toBe(2);
    expect(shownAnswer(item())!.svg).toContain("M0 0h30v10z");
    expect(calls).toHaveLength(2);
  });

  it("a font pick recomposes only the traces that set its line; the others keep their vector as it is", async () => {
    const T1b: TextLine = { ...T1, candidates: [candidate(FONT_M, "M0 20h10v10z", "a-t1", "2026"), candidate(FONT_L, "M0 20h20v10z", "a-t1", "2026")] };
    const { calls, answer, item, lib } = await tracedOnBalanced(PLAN([T0, T1b]));
    for (let i = 0; i < 4; i++) await answer(calls[i]);
    expect(Object.keys(item().traces)).toHaveLength(4);
    const before = item().traces;
    await lib.setTextLine("a", "a-t1", { font: FONT_L });
    const after = item().traces;
    expect(after[`${P}|a-t0`]).toBe(before[`${P}|a-t0`]);
    expect(after[`${P}|`]).toBe(before[`${P}|`]);
    expect(after[`${P}|a-t0,a-t1`].svg).toContain("M0 20h20v10z");
    expect(after[`${P}|a-t0,a-t1`].svg).not.toContain("M0 20h10v10z");
    expect(after[`${P}|a-t1`].svg).toContain("M0 20h20v10z");
  });

  it("keeps at most MASKED_TRACES_MAX masked traces per settings key, dropping the least recently shown", async () => {
    const ids = [0, 1, 2, 3, 4, 5].map((n) => `a-t${n}`);
    const lines = ids.map((id, n): TextLine => ({ ...T1, id, quad: [[0, n * 5], [30, n * 5], [30, n * 5 + 4], [0, n * 5 + 4]], candidates: [candidate(FONT_M, `M0 ${n * 5}h10v4z`, id, "2026")] }));
    const { calls, answer, item, lib } = await tracedOnBalanced(PLAN(lines));
    const answered = new Set<Call>();
    const drain = async () => {
      for (let c = calls.find((x) => !answered.has(x)); c; c = calls.find((x) => !answered.has(x))) {
        answered.add(c);
        if (!c.signal.aborted) await answer(c);
      }
    };
    await drain();
    // every subset of the six lines, one switch at a time (a Gray code), each shown in turn
    const gray = (i: number) => i ^ (i >> 1);
    const shownKeys: string[] = [];
    for (let i = 1; i < 64; i++) {
      const bit = Math.log2(gray(i) ^ gray(i - 1));
      lib.toggleTextLine("a", ids[bit], !((gray(i) >> bit) & 1));
      await drain();
      shownKeys.push(item().shown!);
    }
    const masked = Object.keys(item().traces).filter((k) => k.startsWith(`${P}|`) && k !== `${P}|`);
    expect(masked.length).toBeLessThanOrEqual(MASKED_TRACES_MAX);
    expect(masked.length).toBeGreaterThan(MASKED_TRACES_MAX - 4);
    // never the full mask, the untouched image, or the trace on screen
    expect(item().traces[`${P}|${ids.join(",")}`]).toBeDefined();
    expect(item().traces[`${P}|`]).toBeDefined();
    expect(item().traces[item().shown!]).toBeDefined();
    // the least recently shown went (the second subset shown, two lines off), the last ones shown are all kept
    expect(shownKeys[1]).toBe(`${P}|a-t2,a-t3,a-t4,a-t5`);
    expect(item().traces[shownKeys[1]]).toBeUndefined();
    for (const k of shownKeys.slice(-12)) expect(item().traces[k]).toBeDefined();
  });

  it("a string edit no font fits switches its line off; the traces that painted it out keep their glyphs until the new trace lands", async () => {
    const { calls, answer, item, lib, platform } = await tracedOnBalanced();
    await answer(calls[0]);
    const full = item().traces[`${P}|a-t0,a-t1`].svg;
    expect(full).toContain("M0 0h10v10z");
    platform.textSetLine.mockImplementationOnce(async () => ({ plan: PLAN([{ ...T0, string: "€€€", status: { kind: "declined", reason: "no_fit" }, candidates: [], chosen: null, fit: null }, T1]) }));
    await lib.setTextLine("a", "a-t0", { string: "€€€" });
    expect([...textOf(item()).enabled]).toEqual(["a-t1"]);
    expect(item().traces[`${P}|a-t0,a-t1`].svg).toBe(full);
    expect(item().shown).toBe(`${P}|a-t0,a-t1`);
    // the trace without it is a user trace
    expect(currentJob(item())).toMatchObject({ key: `${P}|a-t1`, mask: ["a-t1"] });
    await answer(calls[calls.length - 1]);
    expect(item().shown).toBe(`${P}|a-t1`);
  });

  it("a string edit recomposes every cached trace from its base", async () => {
    const { calls, answer, item, lib, platform } = await tracedOnBalanced();
    await answer(calls[0]);
    await answer(calls[1]);
    await answer(calls[2]);
    expect(calls[3].req.mask).toEqual(["a-t0"]); // prefetching
    const edited = PLAN([{ ...T0, string: "EST.", candidates: [candidate(FONT_M, "M0 0h11v10z", "a-t0", "EST."), candidate(FONT_L, "M0 0h21v10z", "a-t0", "EST.")] }, T1]);
    platform.textSetLine.mockImplementationOnce(async () => ({ plan: edited }));
    await lib.setTextLine("a", "a-t0", { string: "EST." });
    expect(calls[3].signal.aborted).toBe(true);
    // nothing traced on the edit's account: the aborted prefetch is only queued again
    expect(calls).toHaveLength(5);
    expect(calls[4].req.mask).toEqual(["a-t0"]);
    const done = item();
    expect(textOf(done).plan).toBe(edited);
    for (const [key, t] of Object.entries(done.traces)) {
      const mask = key.slice(key.lastIndexOf("|") + 1).split(",").filter(Boolean);
      expect(t.svg).toBe(composeSvg(t.base, { plan: edited, enabled: new Set(mask), hiddenText: false }));
    }
    expect(done.traces[`${P}|a-t0,a-t1`].svg).toContain("M0 0h11v10z");
    expect(done.traces[`${P}|a-t0,a-t1`].svg).not.toContain("M0 0h10v10z");
    expect(done.traces[`${P}|`].svg).toBe(baseFor([]));
  });

  it("a plan with no matched line traces with an empty mask and prefetches nothing", async () => {
    const declined = (l: TextLine): TextLine => ({ ...l, status: { kind: "declined", reason: "no_fit" }, chosen: null, enabled: false, fit: null });
    const { calls, answer, item } = await tracedOnBalanced(PLAN([declined(T0), declined(T1)]));
    expect(calls.map((c) => c.req.mask)).toEqual([[]]);
    await answer(calls[0]);
    expect(item().shown).toBe(`${P}|`);
    expect(shownAnswer(item())!.svg).toBe(baseFor([]));
    expect(calls).toHaveLength(1);
    expect(item().job).toBeNull();
  });

  it("hidden text off recomposes every trace without <text>", async () => {
    const on = { ...TEXT_ON, hiddenText: true };
    const { calls, answer, item, lib } = await tracedOnBalanced(PLAN([T0, T1]), on);
    await answer(calls[0]);
    await answer(calls[1]);
    await answer(calls[2]);
    lib.markExported([{ id: "a", key: item().shown! }]);
    expect(shownAnswer(item())!.svg).toContain("<text");
    lib.setSettings({ ...on, hiddenText: false });
    for (const t of Object.values(item().traces)) expect(t.svg).not.toContain("<text");
    expect(item().traces[`${P}|a-t0,a-t1`].svg).toContain('data-line="a-t0"');
    expect(item().exported).toBeNull();
    lib.setSettings(on);
    expect(item().traces[`${P}|a-t0,a-t1`].svg).toContain("<text");
    expect(item().traces[`${P}|a-t1`].svg).toContain(">2026</text>");
    expect(item().traces[`${P}|a-t1`].svg).not.toContain(">EST</text>");
  });

  it("turning Recognise text off keeps only the traces of the untouched image, and shows one when it is known", async () => {
    const { calls, answer, item, lib } = await tracedOnBalanced();
    await answer(calls[0]);
    await answer(calls[1]);
    expect(calls[2].req.mask).toEqual(["a-t1"]);
    lib.markExported([{ id: "a", key: item().shown! }]);
    lib.setSettings({ ...TEXT_ON, recogniseText: false });
    expect(calls[2].signal.aborted).toBe(true);
    expect(Object.keys(item().traces)).toEqual([`${P}|`]);
    expect(item().shown).toBe(`${P}|`);
    expect(item().exported).toBeNull();
    await flush();
    expect(calls).toHaveLength(3);
  });

  it("a candidate pick made while a string edit is in flight waits for it and is applied as the app answers", async () => {
    const { calls, answer, item, lib, platform } = await tracedOnBalanced();
    await answer(calls[0]);
    const edited = PLAN([{ ...T0, string: "EST.", candidates: [candidate(FONT_M, "M0 0h11v10z", "a-t0", "EST."), candidate(FONT_L, "M0 0h21v10z", "a-t0", "EST.")], chosen: 0 }, T1]);
    const picked = PLAN([{ ...edited.lines[0], chosen: 1 }, T1]);
    let release: () => void = () => {};
    platform.textSetLine
      .mockImplementationOnce((async () => {
        await new Promise<void>((r) => (release = r));
        return { plan: edited };
      }) as never)
      .mockImplementationOnce(async () => ({ plan: picked }));
    const edit = lib.setTextLine("a", "a-t0", { string: "EST." });
    const pick = lib.setTextLine("a", "a-t0", { font: FONT_L });
    await flush();
    expect(platform.textSetLine).toHaveBeenCalledTimes(1); // the pick waits behind the edit
    release();
    await Promise.all([edit, pick]);
    expect(platform.textSetLine.mock.calls.map((c) => c[2])).toEqual([{ string: "EST." }, { font: FONT_L }]);
    const line = textOf(item()).plan!.lines[0];
    expect(line.string).toBe("EST.");
    expect(line.chosen).toBe(1);
    expect(shownAnswer(item())!.svg).toContain("M0 0h21v10z");
  });

  it("after Revert, Generate asks for the plan again and shows the cached full-mask trace without tracing", async () => {
    const { calls, answer, item, lib, platform } = await tracedOnBalanced();
    await answer(calls[0]);
    await answer(calls[1]); // the untouched image: what the key is without a plan
    await lib.redraw("a");
    await lib.acceptRedraw("a");
    await lib.revertRedraw("a");
    expect(textOf(item()).plan).toBeNull();
    expect(item().traces[`${P}|`]).toBeDefined();
    const traced = calls.length;
    lib.generate("a");
    await flush();
    expect(platform.textPlan).toHaveBeenCalledTimes(2);
    expect(calls).toHaveLength(traced);
    expect(item().shown).toBe(`${P}|a-t0,a-t1`);
    expect(item().job).toBeNull();
  });

  it("after Revert, turning hidden text off keeps the glyphs of the masked trace on screen", async () => {
    const on = { ...TEXT_ON, hiddenText: true };
    const { calls, answer, item, lib } = await tracedOnBalanced(PLAN([T0, T1]), on);
    await answer(calls[0]);
    await lib.redraw("a");
    await lib.acceptRedraw("a");
    await lib.revertRedraw("a");
    expect(textOf(item()).plan).toBeNull();
    expect(item().shown).toBe(`${P}|a-t0,a-t1`);
    const svg = shownAnswer(item())!.svg;
    expect(svg).toContain('data-line="a-t0"');
    lib.setSettings({ ...on, hiddenText: false });
    expect(shownAnswer(item())!.svg).toBe(svg);
  });

  it("prefetches for the selected image only: another selected stops it, and it resumes when selected again", async () => {
    const { calls, answer, lib } = await tracedOnBalanced();
    await answer(calls[0]);
    expect(calls[1].req).toMatchObject({ imageId: "a", mask: [] });
    lib.add([ok("b")]); // b is selected: a's prefetch stops and waits
    expect(calls[1].signal.aborted).toBe(true);
    await flush();
    expect(calls).toHaveLength(2);
    lib.pickPreset("b", PRESETS[1]);
    lib.generate("b");
    await flush();
    expect(calls[2].req).toMatchObject({ imageId: "b", mask: BOTH });
    await answer(calls[2]);
    // b's own prefetch runs, not a's
    expect(calls[3].req).toMatchObject({ imageId: "b", mask: [] });
    lib.select("a");
    expect(calls[3].signal.aborted).toBe(true);
    await flush();
    // a's stopped prefetch was at the head of the queue
    expect(calls[4].req).toMatchObject({ imageId: "a", mask: [] });
    await answer(calls[4]);
    expect(calls[5].req).toMatchObject({ imageId: "a", mask: ["a-t1"] });
    lib.select("b");
    await flush();
    expect(calls[5].signal.aborted).toBe(true);
    expect(calls[6].req).toMatchObject({ imageId: "b", mask: [] });
  });

  it("prefetches only the untouched image when the plan has more than PREFETCH_MAX_LINES matched lines", async () => {
    const many = Array.from({ length: PREFETCH_MAX_LINES + 1 }, (_, n): TextLine => ({ ...T1, id: `a-t${n}`, quad: [[0, n * 3], [30, n * 3], [30, n * 3 + 2], [0, n * 3 + 2]], candidates: [candidate(FONT_M, `M0 ${n * 3}h10v2z`, `a-t${n}`, "2026")] }));
    const { calls, answer } = await tracedOnBalanced(PLAN(many));
    await answer(calls[0]);
    expect(calls[1].req.mask).toEqual([]);
    await answer(calls[1]);
    expect(calls).toHaveLength(2);
    // at the cap, every "all but one" too
    const { calls: c8, answer: a8 } = await tracedOnBalanced(PLAN(many.slice(0, PREFETCH_MAX_LINES)));
    for (let i = 0; i < 2; i++) await a8(c8[i]);
    expect(c8[2].req.mask).toHaveLength(PREFETCH_MAX_LINES - 1);
  });

  it("the prefetch queue is rebuilt after a string edit, and after settings that land on a cached trace once they rest", async () => {
    const { calls, answer, lib } = await tracedOnBalanced();
    await answer(calls[0]);
    expect(calls[1].req.mask).toEqual([]);
    await lib.setTextLine("a", "a-t0", { string: "EST." });
    expect(calls[1].signal.aborted).toBe(true);
    await flush();
    expect(calls[2].req.mask).toEqual([]);
    lib.setParam("a", "detail", 9);
    expect(calls[2].signal.aborted).toBe(true);
    await flush();
    expect(calls).toHaveLength(3);
    lib.setParam("a", "detail", 6); // back to the traced settings
    await flush();
    // a slider's ticks across the cached settings start nothing until they rest
    vi.advanceTimersByTime(PREFETCH_SETTLE_MS / 2);
    lib.setParam("a", "detail", 7);
    lib.setParam("a", "detail", 6);
    vi.advanceTimersByTime(PREFETCH_SETTLE_MS - 1);
    await flush();
    expect(calls).toHaveLength(3);
    vi.advanceTimersByTime(1);
    await flush();
    expect(calls[3].req).toMatchObject({ mask: [], parameters: { detail: 6, min_region: 8 } });
  });
});
