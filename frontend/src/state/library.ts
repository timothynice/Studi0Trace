/**
 * The images open in the window, each with its own settings, every trace seen for it and the
 * job running for it. Framework-free: React reads it through `useLibrary`, tests call it directly.
 */
import { ApiError, type AutoResult, type EngineDescription, type ParamValues, type Preset, type VectorizeResponse } from "@/lib/api";
import { composeSvg } from "@/lib/textsvg";
import { normalizeValues, paramsKey, specsFor } from "@/lib/schema";
import type { Drift, FontRef, OpenedImage, OpenFailure, OpenOutcome, Phase, Platform, RedrawPhase, RedrawResult, Roughness, Settings, TextLine, TextLineChange, TextPhase, TextPlan } from "@/platform/types";

export const ENGINE = "vexel";
/** A trace quicker than this is run again by itself when its settings move. */
export const LIVE_MS = 2000;
/** How long a control must rest before a live update traces. */
export const DEBOUNCE_MS = 250;

export interface TraceAnswer {
  /** What is shown and exported: `base` with the lines of the trace's own mask set in their fonts (`composeSvg`). */
  svg: string;
  /** The SVG as the engine traced it, of the image with the mask's lines painted out. */
  base: string;
  elapsedMs: number;
  stats: Record<string, number>;
}

export interface Job {
  id: string;
  /** The trace key it traces (`traceKeyFor`). */
  key: string;
  startedAt: number;
  phase: Phase;
  /** The lines painted out of the image it traces. */
  mask: string[];
  /** Traced ahead in the background, so a switch is instant: shows no busy state, and fails silently. */
  prefetch: boolean;
}

export interface ImageItem {
  image: OpenedImage;
  /** "auto", a preset's id, or null once a control has been moved by hand. */
  preset: string | null;
  /** The engine's values the panel shows; after an Auto run, those of Auto's pick. */
  params: ParamValues;
  /** Every answer seen for this image, by trace key (`traceKeyFor`: the settings and the lines painted out). */
  traces: Record<string, TraceAnswer>;
  /** The key of the answer on screen; null before the first trace. */
  shown: string | null;
  /** The key of the trace last exported or copied; null before the first. */
  exported: string | null;
  job: Job | null;
  /** The last trace that failed, and the settings key it was for: see `errorOf`. */
  error: ApiError | null;
  errorKey: string | null;
  /** The last Auto run on this image. */
  auto: AutoResult | null;
  /** AI redraw (the Mac app); absent until one is asked for. */
  redraw?: RedrawState;
  /** Whether the image looks rough (the inspector's hint); absent until the app has answered. */
  rough?: Roughness;
  /** Text re-setting (the Mac app); absent until asked. */
  text?: TextState;
}

/** An image's text plan: asked once per image bytes, before the first trace. */
export interface TextState {
  plan: TextPlan | null;
  phase: TextPhase | null;
  error: ApiError | null;
  asked: boolean;
  /** Lines currently set in a font: the plan's `enabled` flags when it lands, the store's own afterwards. */
  enabled: ReadonlySet<string>;
}

export const NO_TEXT: TextState = { plan: null, phase: null, error: null, asked: false, enabled: new Set() };

export const textOf = (item: Pick<ImageItem, "text">): TextState => item.text ?? NO_TEXT;

const matchedLines = (plan: TextPlan | null): TextLine[] => (plan?.lines ?? []).filter((l) => l.status.kind === "matched");

/** The lines painted out of the next trace: the sorted ids of the enabled, matched lines. */
export const maskOf = (text: TextState): string[] => matchedLines(text.plan).filter((l) => text.enabled.has(l.id)).map((l) => l.id).sort();

/** Every matched line, sorted: the mask of the first trace. */
export const fullMask = (plan: TextPlan): string[] => matchedLines(plan).map((l) => l.id).sort();

/**
 * The key a trace of `item`'s settings with `mask` painted out is kept under. On Auto, the full mask is Auto's own
 * run (`auto|…`); every other mask is traced and kept under the pick's params once the pick is known (an Auto run
 * leaves them in `item.params`), as Auto's candidates are, and is an Auto run of its own only before that.
 */
export function traceKeyFor(item: Pick<ImageItem, "preset" | "params"> & Partial<Pick<ImageItem, "auto" | "text">>, mask: string[]): string {
  const m = mask.join(",");
  if (item.preset !== "auto") return `${paramsKey(item.params)}|${m}`;
  const plan = item.text?.plan ?? null;
  const full = plan ? fullMask(plan).join(",") : "";
  return `${m === full || !item.auto?.pick ? "auto" : paramsKey(item.params)}|${m}`;
}

/** The mask a trace key was traced with. */
export const maskOfKey = (key: string): string[] => key.slice(key.lastIndexOf("|") + 1).split(",").filter(Boolean);

/** An Auto run's key. */
export const isAutoKey = (key: string): boolean => key.startsWith("auto|");

const sameFont = (a: FontRef, b: FontRef) => a.family === b.family && a.weight === b.weight && a.italic === b.italic && a.source === b.source;

/** An image's AI redraw: the one running, the one waiting for a decision, and the original while the source is a redraw. */
export interface RedrawState {
  phase: RedrawPhase | null;
  pending: RedrawResult | null;
  /** The image as it was opened; set while the source is a redraw. */
  original: OpenedImage | null;
  /** How far the redraw in use moved the image. */
  drift: Drift | null;
  /** The source is the redraw. */
  active: boolean;
  /** The last redraw that failed (never `cancelled`, never `no_key`: the consent sheet answers that one). */
  error: ApiError | null;
  /** The original's traces and settings, back on Revert. */
  kept: KeptTraces | null;
}

/** Traces are kept with their keys (masks and all) and their bases, so Revert brings back what they showed. */
export type KeptTraces = Pick<ImageItem, "preset" | "params" | "traces" | "shown" | "exported" | "auto">;

export const NO_REDRAW: RedrawState = { phase: null, pending: null, original: null, drift: null, active: false, error: null, kept: null };

export const redrawOf = (item: Pick<ImageItem, "redraw">): RedrawState => item.redraw ?? NO_REDRAW;

export interface LibraryState {
  items: ImageItem[];
  /** Files that could not be opened, kept in the sidebar with what went wrong. */
  failed: OpenFailure[];
  selected: string | null;
}

export interface Catalog {
  engine: EngineDescription;
  presets: Preset[];
}

/** What the library asks of the platform; the redraw methods are there in the Mac app (tests may leave them out). */
export type LibraryPlatform = Pick<Platform, "vectorize" | "closeImage" | "openPaths"> & Partial<Pick<Platform, "imageRoughness" | "redrawImage" | "acceptRedraw" | "discardRedraw" | "revertRedraw" | "textPlan" | "textSetLine">>;

export interface Library {
  getState(): LibraryState;
  subscribe(listener: () => void): () => void;
  selectedItem(): ImageItem | null;
  add(outcomes: OpenOutcome[]): void;
  select(id: string | null): void;
  selectNext(delta: 1 | -1): void;
  remove(id: string): void;
  clear(): void;
  dismissFailure(index: number): void;
  /** Open a too-large file again, scaled to fit the cap, in place of its failure. */
  downscale(index: number): Promise<void>;
  pickPreset(id: string, preset: Preset): void;
  setParam(id: string, name: string, value: unknown): void;
  generate(id: string): void;
  cancel(id: string): void;
  setSettings(settings: Settings): void;
  /** These traces were exported or copied (by image id and trace key). */
  markExported(marks: ExportMark[]): void;
  /** Asks the app, once, whether the image looks rough. */
  checkRough(id: string): void;
  /** Redraws the image with AI; the answer waits in `redraw.pending`. Resolves with the error when it failed for want of a key (`no_key`), else null. */
  redraw(id: string): Promise<ApiError | null>;
  cancelRedraw(id: string): void;
  /** The waiting redraw becomes the image's source; the original's traces are kept for Revert. */
  acceptRedraw(id: string): Promise<void>;
  discardRedraw(id: string): void;
  /** The original is the source again, with its traces. */
  revertRedraw(id: string): Promise<void>;
  /**
   * A line's string or font changed. A font among the line's candidates is chosen here and every trace composed
   * again at once, the app told in the background; a string, or a font from search, waits for the app's plan.
   */
  setTextLine(id: string, lineId: string, change: TextLineChange): Promise<void>;
  /** A line's switch: shows the cached trace for the new mask, or traces it (a user trace). Frontend-only. */
  toggleTextLine(id: string, lineId: string, on: boolean): void;
  /** Abandons the text plan being asked, and the trace waiting for it; the next trace asks again. */
  cancelText(id: string): void;
  /** Asks for the text plan again after it failed; traces made without it are traced again when it re-sets a line. */
  retryText(id: string): void;
}

export interface ExportMark {
  id: string;
  key: string;
}

/** The key of the trace the controls and the switches ask for now. */
export function traceKey(item: Pick<ImageItem, "preset" | "params" | "text"> & Partial<Pick<ImageItem, "auto">>): string {
  return traceKeyFor(item, maskOf(textOf(item)));
}

/** The error to show: a failure belongs to the settings that failed, and says nothing about any others. */
export function errorOf(item: Pick<ImageItem, "preset" | "params" | "text" | "error" | "errorKey"> & Partial<Pick<ImageItem, "auto">>): ApiError | null {
  return item.error && item.errorKey === traceKey(item) ? item.error : null;
}

/** The image's job unless it is a prefetch: what shows busy. */
export function userJob(item: Pick<ImageItem, "job">): Job | null {
  return item.job && !item.job.prefetch ? item.job : null;
}

/** The job tracing the settings now on the controls; a job for other settings runs on in the background and is not it. */
export function currentJob(item: ImageItem): Job | null {
  const job = userJob(item);
  return job && job.key === traceKey(item) ? job : null;
}

export function shownAnswer(item: ImageItem): TraceAnswer | null {
  return item.shown ? (item.traces[item.shown] ?? null) : null;
}

/** Images whose vector on screen has not been exported or copied since it was traced: what quitting would lose. */
export function unexportedCount(state: LibraryState): number {
  return state.items.filter((i) => i.shown !== null && i.exported !== i.shown).length;
}

/** The settings have moved since the trace on screen, and no job is tracing them. */
export function needsUpdate(item: ImageItem): boolean {
  const key = traceKey(item);
  return item.shown !== null && item.shown !== key && userJob(item)?.key !== key && !item.traces[key];
}

function isCancel(err: unknown): boolean {
  return (err instanceof ApiError && err.code === "cancelled") || (err as Error | null)?.name === "AbortError";
}

function toApiError(err: unknown): ApiError {
  if (err instanceof ApiError) return err;
  return new ApiError("engine_crashed", (err as Error | null)?.message || "The trace failed", 0, err);
}

let jobCounter = 0;
const newJobId = () => `job-${Date.now().toString(36)}-${(jobCounter += 1)}`;

export function createLibrary(platform: LibraryPlatform, catalog: Catalog, initialSettings: Settings): Library {
  const specs = specsFor(catalog.engine);
  const resolve = (params: ParamValues) => normalizeValues(specs, { ...catalog.engine.defaults, ...params });
  const defaults = resolve({});
  const presetById = new Map(catalog.presets.map((p) => [p.id, p]));

  let state: LibraryState = { items: [], failed: [], selected: null };
  let settings = initialSettings;
  const listeners = new Set<() => void>();
  const controllers = new Map<string, AbortController>();
  const timers = new Map<string, ReturnType<typeof setTimeout>>();

  const emit = () => listeners.forEach((l) => l());
  const set = (next: LibraryState) => {
    state = next;
    emit();
  };
  const find = (id: string) => state.items.find((i) => i.image.id === id);
  const patch = (id: string, change: Partial<ImageItem> | ((item: ImageItem) => Partial<ImageItem>)) =>
    set({ ...state, items: state.items.map((i) => (i.image.id === id ? { ...i, ...(typeof change === "function" ? change(i) : change) } : i)) });

  const redrawing = new Map<string, AbortController>();
  const roughAsked = new Set<string>();
  /** Images whose Use redraw or Revert is in flight: a second click is ignored, since the host answers it with a 409. */
  const deciding = new Set<string>();
  const patchRedraw = (id: string, change: Partial<RedrawState>) => patch(id, (i) => ({ redraw: { ...redrawOf(i), ...change } }));
  const abortRedraw = (id: string) => {
    redrawing.get(id)?.abort();
    redrawing.delete(id);
  };

  const texting = new Map<string, { controller: AbortController; done: Promise<boolean> }>();
  /** Each image's line edits run one after another, in the order they were made. */
  const editQueue = new Map<string, Promise<void>>();
  const patchText = (id: string, change: Partial<TextState>) => patch(id, (i) => ({ text: { ...textOf(i), ...change } }));
  const abortText = (id: string) => {
    texting.get(id)?.controller.abort();
    texting.delete(id);
  };

  const clearTimer = (id: string) => {
    clearTimeout(timers.get(id));
    timers.delete(id);
  };

  const abortJob = (item: ImageItem | undefined) => {
    if (!item?.job) return;
    if (prefetching?.jobId === item.job.id) prefetching = null;
    controllers.get(item.job.id)?.abort();
    controllers.delete(item.job.id);
  };

  const answerOf = (res: VectorizeResponse): TraceAnswer | ApiError => {
    const r = res.results[ENGINE];
    if (!r || r.error || !r.svg) return new ApiError(r?.error?.code ?? "engine_failed", r?.error?.message ?? "The engine returned no vector");
    return { svg: r.svg, base: r.svg, elapsedMs: r.elapsed_ms ?? 0, stats: r.stats ?? {} };
  };

  /** `base` with the lines of `mask` set in their fonts, as `plan` has them now. */
  const composed = (base: string, mask: string[], plan: TextPlan | null, hiddenText = settings.hiddenText) =>
    composeSvg(base, { plan, enabled: new Set(mask), hiddenText });

  /**
   * Every trace composed again from its base, each with its own key's mask. Without a plan (after a Revert, whose
   * kept traces carry their glyphs while the plan is gone) a trace with lines painted out keeps the vector it has:
   * its base alone would show the image with the text missing.
   */
  const recomposed = (traces: Record<string, TraceAnswer>, plan: TextPlan | null, hiddenText = settings.hiddenText) =>
    Object.fromEntries(Object.entries(traces).map(([k, t]) => [k, !plan && maskOfKey(k).length ? t : { ...t, svg: composed(t.base, maskOfKey(k), plan, hiddenText) }]));

  /** What was exported stays exported only while its vector is unchanged. */
  const keptExport = (item: ImageItem, traces: Record<string, TraceAnswer>) =>
    item.exported !== null && traces[item.exported] && traces[item.exported].svg === item.traces[item.exported]?.svg ? item.exported : null;

  // Prefetching: the masks a switch may ask for, traced ahead one at a time across every image, only while no
  // user job runs anywhere; on Auto once its pick is known, under the pick's params (never an Auto run). `queue`
  // holds what waits; `prefetching` the one running.
  interface Prefetch { id: string; key: string; mask: string[]; params: ParamValues }
  let queue: Prefetch[] = [];
  let prefetching: (Prefetch & { jobId: string }) | null = null;

  /** After a user trace: the untouched image, then each matched line's "all but this one", in plan order, that are not cached. */
  const schedulePrefetch = (id: string) => {
    const item = find(id);
    const plan = item ? textOf(item).plan : null;
    if (!item || !plan) return;
    const full = fullMask(plan);
    if (!full.length) return;
    const masks = [[], ...matchedLines(plan).map((l) => full.filter((m) => m !== l.id))];
    for (const mask of masks) {
      const key = traceKeyFor(item, mask);
      // an Auto image whose pick is not known yet: its masks are Auto runs, which are never traced ahead
      if (isAutoKey(key) || item.traces[key] || item.job?.key === key || queue.some((q) => q.id === id && q.key === key)) continue;
      queue.push({ id, key, mask, params: item.params });
    }
  };

  /** Starts the next prefetch, when none runs and no image has a user job. */
  const runPrefetch = () => {
    if (prefetching || state.items.some((i) => userJob(i))) return;
    while (queue.length) {
      const next = queue.shift()!;
      const item = find(next.id);
      // gone, traced meanwhile, or the settings moved on since it was queued
      if (!item || item.job || isAutoKey(next.key) || item.traces[next.key] || traceKeyFor(item, next.mask) !== next.key) continue;
      const job: Job = { id: newJobId(), key: next.key, startedAt: Date.now(), phase: "queued", mask: next.mask, prefetch: true };
      prefetching = { ...next, jobId: job.id };
      launch(next.id, job, next.params, false);
      return;
    }
  };

  /**
   * A user action: the queue of `id` (every image's when none is named) is cleared, and the running prefetch is
   * aborted when it is of that image, or when `starting` (a user job is about to start and needs the worker); a
   * prefetch of another image aborted for it goes back to the head of the queue. The next one starts once no user
   * job is left (this runs it in a microtask, after the caller has started its own job, if any).
   */
  const abortPrefetch = (id?: string, starting = false) => {
    queue = id === undefined ? [] : queue.filter((q) => q.id !== id);
    const running = prefetching;
    if (running && (id === undefined || running.id === id || starting)) {
      prefetching = null;
      controllers.get(running.jobId)?.abort();
      controllers.delete(running.jobId);
      if (find(running.id)?.job?.id === running.jobId) patch(running.id, { job: null });
      if (id !== undefined && running.id !== id) queue.unshift({ id: running.id, key: running.key, mask: running.mask, params: running.params });
    }
    queueMicrotask(runPrefetch);
  };

  /** The prefetch tracing what the user now asks for becomes the user's job: busy, and not aborted as a prefetch. */
  const promote = (id: string) => {
    const item = find(id);
    if (!item?.job?.prefetch) return;
    if (prefetching?.jobId === item.job.id) prefetching = null;
    patch(id, { job: { ...item.job, prefetch: false }, error: null, errorKey: null });
  };

  const finish = (id: string, jobId: string, res: VectorizeResponse) => {
    const item = find(id);
    if (!item || item.job?.id !== jobId) return; // superseded or cancelled
    const job = item.job;
    if (prefetching?.jobId === jobId) prefetching = null;
    const answer = answerOf(res);
    if (answer instanceof ApiError) {
      // a prefetch fails silently
      patch(id, job.prefetch ? { job: null } : { job: null, error: answer, errorKey: job.key });
      runPrefetch();
      return;
    }
    const plan = textOf(item).plan;
    const traces = { ...item.traces, [job.key]: { ...answer, svg: composed(answer.base, job.mask, plan) } };
    const auto = res.auto?.[ENGINE] ?? null;
    let params = item.params;
    if (auto) {
      for (const c of auto.candidates) {
        const preset = presetById.get(c.preset);
        if (preset && c.svg) traces[traceKeyFor({ preset: null, params: resolve(preset.params) }, job.mask)] = { svg: composed(c.svg, job.mask, plan), base: c.svg, elapsedMs: c.elapsed_ms ?? 0, stats: c.stats ?? {} };
      }
      const pick = auto.pick ? presetById.get(auto.pick) : undefined;
      if (pick) {
        // Auto's answer is its pick's trace: known under the pick's key too, which the other masks are keyed by
        traces[traceKeyFor({ preset: null, params: resolve(pick.params) }, job.mask)] ??= traces[job.key];
        // an image still on Auto takes its pick's values, whatever it shows: the masks it traces next are keyed by them
        if (item.preset === "auto") params = resolve(pick.params);
      }
    }
    // the answer is always kept; it takes the screen only if the settings still are the ones it traced, and settings
    // that moved on while it ran show their own trace if one is known (a candidate, say), else stay as they are
    const want = traceKey({ ...item, params, auto: auto ?? item.auto });
    const now = traces[want] ? want : item.shown;
    patch(id, { job: null, ...(job.prefetch ? {} : { error: null, errorKey: null }), traces, shown: now, auto: auto ?? item.auto, params });
    if (!job.prefetch) schedulePrefetch(id);
    runPrefetch();
  };

  const fail = (id: string, jobId: string, err: unknown) => {
    const item = find(id);
    if (!item || item.job?.id !== jobId) return;
    if (prefetching?.jobId === jobId) prefetching = null;
    // kept with the key it failed for: shown only while the settings are those again; a prefetch fails silently
    patch(id, isCancel(err) || item.job.prefetch ? { job: null } : { job: null, error: toApiError(err), errorKey: item.job.key });
    runPrefetch();
  };

  /** Sends `job` to the platform as the image's job. */
  const launch = (id: string, job: Job, parameters: ParamValues, auto: boolean) => {
    const controller = new AbortController();
    controllers.set(job.id, controller);
    patch(id, job.prefetch ? { job } : { job, error: null, errorKey: null });
    platform
      .vectorize(
        { imageId: id, parameters: auto ? {} : parameters, auto, job: job.id, mask: job.mask },
        {
          signal: controller.signal,
          onPhase: (phase) => {
            const now = find(id);
            if (now?.job?.id === job.id) patch(id, { job: { ...now.job, phase } });
          },
        },
      )
      .then((res) => finish(id, job.id, res))
      .catch((err) => fail(id, job.id, err))
      .finally(() => controllers.delete(job.id));
  };

  /**
   * The plan, once per image bytes, before the first trace; a failure is kept and the trace goes on.
   * Resolves true when the trace should go ahead, false when this plan was cancelled or superseded
   * (the image was removed, its bytes changed, Cancel was pressed): then nothing is traced on its account.
   */
  const ensureText = (id: string): Promise<boolean> => {
    const item = find(id);
    if (!item || !settings.recogniseText || !platform.textPlan) return Promise.resolve(true);
    const running = texting.get(id);
    if (running) return running.done; // a trace asked for meanwhile waits for the same plan
    const t = textOf(item);
    if (t.plan || t.asked) return Promise.resolve(true);
    const controller = new AbortController();
    const current = () => texting.get(id)?.controller === controller && !!find(id);
    const entry = { controller, done: Promise.resolve(false) };
    texting.set(id, entry);
    patchText(id, { asked: true, phase: "finding", error: null });
    entry.done = (async () => {
      try {
        const plan = await platform.textPlan!(id, {
          signal: controller.signal,
          onPhase: (phase) => {
            if (current() && phase !== "done" && phase !== "failed") patchText(id, { phase });
          },
        });
        if (!current()) return false;
        // the plan's flags say which lines start set in a font; the switches own the set from here
        patchText(id, { plan, phase: null, enabled: new Set(plan.lines.filter((l) => l.enabled).map((l) => l.id)) });
        return true;
      } catch (err) {
        if (!current()) return false;
        const e = toApiError(err);
        // a cancel is not an answer: the next trace asks again
        patchText(id, e.code === "cancelled" ? { phase: null, error: null, asked: false } : { phase: null, error: e });
        return e.code !== "cancelled";
      } finally {
        if (texting.get(id)?.controller === controller) texting.delete(id);
      }
    })();
    return entry.done;
  };

  const needsText = (id: string) => {
    const item = find(id);
    return !!item && settings.recogniseText && !!platform.textPlan && (texting.has(id) || (!textOf(item).plan && !textOf(item).asked));
  };

  const generate = (id: string) => {
    const item = find(id);
    if (!item) return;
    const key = traceKey(item);
    if (item.job?.key === key) {
      promote(id);
      return;
    }
    // a plan to ask comes first even when the key without it is cached (after a Revert the plan is gone and the
    // untouched trace is known): once the plan lands, its own key is shown if cached, else traced
    const text = needsText(id);
    if (item.traces[key] && !text) {
      patch(id, { shown: key, error: null, errorKey: null });
      return;
    }
    // a user job: the background work stops for it
    abortPrefetch(id, true);
    // the plan comes first, and is only waited for when there is one to ask; otherwise the trace starts at once.
    // While it runs the image holds a job of phase "text", so the trace button shows busy with Cancel and the
    // canvas says what is happening; the real trace job replaces it
    if (text) {
      abortJob(item);
      const textJob = newJobId();
      patch(id, { job: { id: textJob, key, startedAt: Date.now(), phase: "text", mask: maskOf(textOf(item)), prefetch: false }, error: null, errorKey: null });
      void ensureText(id).then((go) => {
        if (go) startTrace(id);
        else if (find(id)?.job?.id === textJob) {
          patch(id, { job: null });
          runPrefetch();
        }
      });
    } else startTrace(id);
  };

  /** The image's job, unless it is the stand-in for a text plan (which `startTrace` replaces). */
  const traceJob = (item: ImageItem) => (item.job?.phase === "text" ? null : item.job);

  const startTrace = (id: string) => {
    const item = find(id);
    if (!item) return;
    const key = traceKey(item);
    if (item.traces[key]) {
      patch(id, { shown: key, error: null, errorKey: null, job: traceJob(item) });
      if (!userJob(find(id)!)) runPrefetch();
      return;
    }
    if (traceJob(item)?.key === key) {
      promote(id);
      return;
    }
    abortJob(item);
    const mask = maskOf(textOf(item));
    launch(id, { id: newJobId(), key, startedAt: Date.now(), phase: "queued", mask, prefetch: false }, item.params, isAutoKey(key));
  };

  /** After the enabled set or the plan moved the key: show what is known for it, or trace it (a user trace). */
  const showOrTrace = (id: string) => {
    const item = find(id);
    // nothing traced or tracing yet: the first trace takes the set as it is then
    if (!item || (item.shown === null && !userJob(item))) return;
    const key = traceKey(item);
    if (item.traces[key]) {
      // a job for another mask runs on: its answer is kept under its own key
      patch(id, { shown: key, error: null, errorKey: null });
      return;
    }
    generate(id);
  };

  /** After the settings of a traced image move: show what is known, or trace if traces here are quick. */
  const settle = (id: string, debounce: boolean) => {
    const item = find(id);
    if (!item) return;
    const key = traceKey(item);
    if (item.traces[key]) {
      // a job for other settings runs on: its answer is kept as any other, and it takes nothing from the screen
      patch(id, { shown: key });
      // the settings changed, so the background work stopped: it starts again for these
      schedulePrefetch(id);
      runPrefetch();
      return;
    }
    const last = item.shown ? item.traces[item.shown] : undefined;
    if (!settings.liveUpdate || !last || last.elapsedMs >= LIVE_MS) return;
    clearTimer(id);
    if (!debounce) {
      generate(id);
      return;
    }
    timers.set(
      id,
      setTimeout(() => {
        timers.delete(id);
        if (settings.liveUpdate) generate(id);
      }, DEBOUNCE_MS),
    );
  };

  const selectAfterRemoving = (id: string): string | null => {
    const at = state.items.findIndex((i) => i.image.id === id);
    if (state.selected !== id) return state.selected;
    const rest = state.items.filter((i) => i.image.id !== id);
    return rest[Math.min(at, rest.length - 1)]?.image.id ?? null;
  };

  /** `plan` with line `lineId` set in its candidate `at`: the app's choice of a known font, made here. */
  const withChosen = (plan: TextPlan, lineId: string, at: number): TextPlan => ({
    ...plan,
    lines: plan.lines.map((l) => (l.id === lineId ? { ...l, chosen: at, fit: l.candidates[at].fit, status: { kind: "matched" as const } } : l)),
  });

  /**
   * The image's plan is now `plan`: a line it matched that was not matched before is set in a font (the app never
   * changes a line's `enabled` once the plan has landed, so the store does), every trace is composed again from
   * its base, and a mask that moved shows its cached trace or is traced.
   */
  const applyPlan = (id: string, plan: TextPlan) => {
    const item = find(id);
    if (!item) return;
    const t = textOf(item);
    const wasMatched = new Set(matchedLines(t.plan).map((l) => l.id));
    const enabled = new Set(t.enabled);
    for (const l of matchedLines(plan)) if (!wasMatched.has(l.id)) enabled.add(l.id);
    const before = traceKey(item);
    const traces = recomposed(item.traces, plan);
    patch(id, { traces, exported: keptExport(item, traces), text: { ...t, plan, enabled } });
    if (traceKey(find(id)!) !== before) showOrTrace(id);
  };

  const lib: Library = {
    getState: () => state,
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    selectedItem: () => (state.selected ? (find(state.selected) ?? null) : null),

    add(outcomes) {
      const items = [...state.items];
      const failed = [...state.failed];
      const added: string[] = [];
      let selected = state.selected;
      for (const o of outcomes) {
        if ("failed" in o) {
          if (!failed.some((f) => f.name === o.failed.name && f.path === o.failed.path)) failed.push(o.failed);
          continue;
        }
        selected = o.ok.id;
        if (items.some((i) => i.image.id === o.ok.id)) continue;
        items.push({ image: o.ok, preset: "auto", params: defaults, traces: {}, shown: null, exported: null, job: null, error: null, errorKey: null, auto: null });
        added.push(o.ok.id);
      }
      set({ items, failed, selected });
      if (settings.traceOnOpen) added.forEach(generate);
    },

    select(id) {
      if (id === null || find(id)) set({ ...state, selected: id });
    },

    selectNext(delta) {
      const at = state.items.findIndex((i) => i.image.id === state.selected);
      const next = state.items[Math.max(0, Math.min(state.items.length - 1, at + delta))];
      if (next) set({ ...state, selected: next.image.id });
    },

    remove(id) {
      const item = find(id);
      if (!item) return;
      abortPrefetch(id);
      abortJob(find(id));
      clearTimer(id);
      abortRedraw(id);
      abortText(id);
      roughAsked.delete(id);
      platform.closeImage(id);
      set({ ...state, selected: selectAfterRemoving(id), items: state.items.filter((i) => i.image.id !== id) });
    },

    clear() {
      abortPrefetch();
      for (const item of state.items) {
        abortJob(item);
        clearTimer(item.image.id);
        abortRedraw(item.image.id);
        abortText(item.image.id);
        roughAsked.delete(item.image.id);
        platform.closeImage(item.image.id);
      }
      set({ items: [], failed: [], selected: null });
    },

    dismissFailure(index) {
      set({ ...state, failed: state.failed.filter((_, i) => i !== index) });
    },

    async downscale(index) {
      const failure = state.failed[index];
      if (!failure?.path) return;
      const outcomes = await platform.openPaths([failure.path], { downscale: true });
      set({ ...state, failed: state.failed.filter((f) => f !== failure) });
      lib.add(outcomes);
    },

    pickPreset(id, preset) {
      const item = find(id);
      if (!item) return;
      abortPrefetch(id);
      if (preset.kind === "auto") {
        // back to Auto shows its pick's values again, not those of the preset tried since
        const pick = item.auto?.pick ? presetById.get(item.auto.pick) : undefined;
        patch(id, pick ? { preset: "auto", params: resolve(pick.params) } : { preset: "auto" });
      }
      else patch(id, { preset: preset.id, params: resolve(preset.params) });
      settle(id, false);
    },

    setParam(id, name, value) {
      const item = find(id);
      if (!item) return;
      const params = normalizeValues(specs, { ...item.params, [name]: value });
      // a control left as it was moves nothing: the preset (Auto) stays, and nothing traces
      if (paramsKey(params) === paramsKey(item.params)) return;
      abortPrefetch(id);
      patch(id, { preset: null, params });
      settle(id, true);
    },

    generate,

    cancel(id) {
      clearTimer(id);
      const item = find(id);
      // a prefetch is not the user's to cancel: nothing shows it
      if (!item?.job || item.job.prefetch) return;
      // the trace is waiting for its text plan: that is what is cancelled
      if (item.job.phase === "text") {
        lib.cancelText(id);
        return;
      }
      abortJob(item);
      patch(id, { job: null });
      // with the user's trace gone, the background work goes on
      runPrefetch();
    },

    setSettings(next) {
      const prev = settings;
      settings = next;
      if (!settings.liveUpdate) [...timers.keys()].forEach(clearTimer);
      const dropText = prev.recogniseText && !next.recogniseText;
      const recompose = prev.hiddenText !== next.hiddenText;
      if (!dropText && !recompose) return;
      if (dropText) abortPrefetch();
      const items = state.items.map((item) => {
        let traces = item.traces;
        let text = item.text;
        // Recognise text off: a trace of a painted image is not the image, and the next trace is of the original.
        // The plan goes too (unless it is being read), so the key's mask is empty and the untouched trace is the one
        if (dropText) {
          traces = Object.fromEntries(Object.entries(traces).filter(([k]) => maskOfKey(k).length === 0));
          if (!texting.has(item.image.id)) text = undefined;
        }
        // the hidden text is in every candidate the app wrote; the setting decides whether the SVG keeps it
        if (recompose) traces = recomposed(traces, textOf({ text }).plan, next.hiddenText);
        const now = { ...item, traces, text };
        const key = traceKey(now);
        return {
          ...now,
          shown: traces[key] ? key : item.shown !== null && traces[item.shown] ? item.shown : null,
          // what was exported is no longer what is on screen
          exported: keptExport(item, traces),
          auto: Object.keys(traces).some(isAutoKey) || !Object.keys(item.traces).some(isAutoKey) ? item.auto : null,
        };
      });
      set({ ...state, items });
    },

    markExported(marks) {
      const byId = new Map(marks.map((m) => [m.id, m.key]));
      if (!state.items.some((i) => byId.has(i.image.id))) return;
      set({ ...state, items: state.items.map((i) => (byId.has(i.image.id) ? { ...i, exported: byId.get(i.image.id)! } : i)) });
    },

    checkRough(id) {
      if (!find(id) || roughAsked.has(id) || !platform.imageRoughness) return;
      roughAsked.add(id);
      platform.imageRoughness(id).then(
        (rough) => {
          if (find(id)) patch(id, { rough });
        },
        () => roughAsked.delete(id),
      );
    },

    async redraw(id) {
      if (!find(id) || !platform.redrawImage) return null;
      abortRedraw(id);
      const controller = new AbortController();
      redrawing.set(id, controller);
      const current = () => redrawing.get(id) === controller && !!find(id);
      patchRedraw(id, { phase: "uploading", pending: null, error: null });
      try {
        const result = await platform.redrawImage(id, {
          signal: controller.signal,
          onPhase: (phase) => {
            // the command's answer is the authority on how it ended: done and failed are not shown as phases
            if (current() && phase !== "done" && phase !== "failed") patchRedraw(id, { phase });
          },
        });
        if (current()) patchRedraw(id, { phase: null, pending: result });
        return null;
      } catch (err) {
        if (!current()) return null;
        const e = err instanceof ApiError ? err : new ApiError("bad_reply", (err as Error | null)?.message || "The redraw failed");
        patchRedraw(id, { phase: null, error: e.code === "cancelled" || e.code === "no_key" ? null : e });
        return e.code === "no_key" ? e : null;
      } finally {
        if (redrawing.get(id) === controller) redrawing.delete(id);
      }
    },

    cancelRedraw(id) {
      abortRedraw(id);
      if (find(id)) patchRedraw(id, { phase: null });
    },

    async acceptRedraw(id) {
      const item = find(id);
      const r = item ? redrawOf(item) : NO_REDRAW;
      if (!item || !r.pending || !platform.acceptRedraw || deciding.has(id)) return;
      deciding.add(id);
      try {
        const accepted = await platform.acceptRedraw(id);
        if (!find(id)) return;
        abortPrefetch(id);
        const now = find(id)!;
        abortJob(now);
        clearTimer(id);
        // a second redraw used keeps the first original's traces
        const kept: KeptTraces = r.kept ?? { preset: now.preset, params: now.params, traces: now.traces, shown: now.shown, exported: now.exported, auto: now.auto };
        patch(id, {
          image: accepted.image,
          traces: {},
          shown: null,
          exported: null,
          auto: null,
          job: null,
          error: null,
          errorKey: null,
          rough: { rough: false, reason: null },
          text: undefined,
          redraw: { ...NO_REDRAW, active: true, original: accepted.original, drift: r.pending.drift, kept },
        });
        abortText(id);
        if (settings.traceOnOpen) generate(id);
      } finally {
        deciding.delete(id);
      }
    },

    discardRedraw(id) {
      const item = find(id);
      if (!item || !redrawOf(item).pending) return;
      void platform.discardRedraw?.(id).catch(() => {});
      patchRedraw(id, { pending: null });
    },

    async setTextLine(id, lineId, change) {
      if (!platform.textSetLine) return;
      const enqueue = (run: () => Promise<void>) => {
        // one edit after another, in order: a failed edit does not stop the ones behind it
        const mine = (editQueue.get(id) ?? Promise.resolve()).then(run, run);
        editQueue.set(id, mine);
        void mine.then(
          () => editQueue.get(id) === mine && editQueue.delete(id),
          () => editQueue.get(id) === mine && editQueue.delete(id),
        );
        return mine;
      };
      const item = find(id);
      const line = item ? textOf(item).plan?.lines.find((l) => l.id === lineId) : undefined;
      if (!item || !line) return;
      const known = change.font && change.string === undefined ? line.candidates.findIndex((c) => sameFont(c.font, change.font!)) : -1;
      // a pick made while an edit is in flight waits for it: applied here to the plan before the edit's answer, the
      // answer would take it back, and the app would answer it from a plan the frontend no longer shows
      if (known >= 0 && !editQueue.has(id)) {
        // a font the app has already fitted: chosen here, as the app would choose it, and composed at once
        applyPlan(id, withChosen(textOf(item).plan!, lineId, known));
        const image = item.image;
        void enqueue(async () => {
          const answer = await platform.textSetLine!(id, lineId, change);
          const now = find(id);
          const plan = now ? textOf(now).plan : null;
          const mine = plan?.lines.find((l) => l.id === lineId);
          const theirs = answer.plan.lines.find((l) => l.id === lineId);
          // the answer is only news when it fitted a candidate the line does not have
          if (!now || now.image !== image || !plan || !mine || !theirs || theirs.candidates.length <= mine.candidates.length) return;
          const chosenFont = mine.chosen !== null ? mine.candidates[mine.chosen]?.font : undefined;
          const at = chosenFont ? theirs.candidates.findIndex((c) => sameFont(c.font, chosenFont)) : -1;
          const merged = at >= 0 ? { ...theirs, chosen: at, fit: theirs.candidates[at].fit, status: mine.status } : theirs;
          applyPlan(id, { ...plan, lines: plan.lines.map((l) => (l.id === lineId ? merged : l)) });
        }).catch(() => {});
        return;
      }
      // a new string re-fits in the app, and the background traces are of settings it may move: they stop
      if (change.string !== undefined) abortPrefetch(id);
      await enqueue(async () => {
        const before = find(id);
        if (!before) return;
        const answer = await platform.textSetLine!(id, lineId, change);
        const now = find(id);
        // a redraw used or a revert while the edit ran swapped the image: the answer is about bytes it no longer has
        if (!now || now.image !== before.image) return;
        applyPlan(id, answer.plan);
        // what is painted out did not change, so the masks still wanted are prefetched again
        schedulePrefetch(id);
        runPrefetch();
      });
    },

    toggleTextLine(id, lineId, on) {
      const item = find(id);
      const t = item ? textOf(item) : NO_TEXT;
      if (!item || !t.plan || t.enabled.has(lineId) === on) return;
      const enabled = new Set(t.enabled);
      if (on) enabled.add(lineId);
      else enabled.delete(lineId);
      patchText(id, { enabled });
      showOrTrace(id);
    },

    cancelText(id) {
      abortText(id);
      const item = find(id);
      if (!item) return;
      // never answered, so not asked: the next trace asks again
      patch(id, { text: { ...textOf(item), phase: null, asked: textOf(item).plan ? textOf(item).asked : false }, ...(item.job?.phase === "text" ? { job: null } : {}) });
      runPrefetch();
    },

    retryText(id) {
      const item = find(id);
      if (!item || texting.has(id)) return;
      patchText(id, { asked: false, error: null });
      void ensureText(id).then((go) => {
        const now = find(id);
        const plan = now ? textOf(now).plan : null;
        // the traces on screen were made without the plan; when it re-sets a line they hold the text as shapes
        // (they are kept: they are the traces of the untouched image, under the empty mask's keys)
        if (!go || !now || !plan || maskOf(textOf(now)).length === 0 || Object.keys(now.traces).length === 0) return;
        clearTimer(id);
        generate(id);
      });
    },

    async revertRedraw(id) {
      const item = find(id);
      if (!item || !redrawOf(item).active || !platform.revertRedraw || deciding.has(id)) return;
      deciding.add(id);
      try {
        // Revert is open mid-run: a redraw still running is abandoned and one waiting for a decision is let go,
        // before the original comes back, so neither can land on it
        abortRedraw(id);
        // the decision goes now, so a slow or failed revert never leaves a Drift check whose redraw has been let go of
        const waiting = redrawOf(item).pending;
        patchRedraw(id, { phase: null, pending: null });
        if (waiting) void platform.discardRedraw?.(id).catch(() => {});
        const image = await platform.revertRedraw(id);
        if (!find(id)) return;
        abortPrefetch(id);
        const now = find(id)!;
        abortJob(now);
        abortText(id);
        clearTimer(id);
        const kept = redrawOf(now).kept ?? { preset: now.preset, params: now.params, traces: {}, shown: null, exported: null, auto: null };
        roughAsked.delete(id);
        patch(id, { ...kept, image, job: null, error: null, errorKey: null, rough: undefined, text: undefined, redraw: NO_REDRAW });
      } finally {
        deciding.delete(id);
      }
    },
  };
  return lib;
}
