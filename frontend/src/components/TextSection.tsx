import { Type } from "lucide-react";
import { useEffect, useState } from "react";

import { declineText, fitPercent, fontLabel, TEXT_PHASE_TEXT, textErrorText } from "@/lib/text";
import type { FontRef, TextLine, TextLineChange } from "@/platform/types";
import { shownAnswer, textOf, type ImageItem } from "@/state/library";
import { Switch } from "./Switch";

export interface TextSectionProps {
  item: ImageItem;
  /** Settings ▸ Text ▸ Recognise text. */
  enabled: boolean;
  onChange: (lineId: string, change: TextLineChange) => void;
  onHover: (lineId: string | null) => void;
  onCancel: () => void;
  /** Asks for the plan again after it failed. */
  onRetry: () => void;
  searchFonts: (query: string) => Promise<FontRef[]>;
}

// the app's one field look (SettingsView's key field)
const FIELD = "h-7 w-full min-w-0 rounded-md border bg-background/70 px-2 text-[12px]";

/** The font picker: the ranked candidates with their fit, then a search over Google Fonts and the installed families. */
function FontPicker({ line, onPick, searchFonts }: { line: TextLine; onPick: (font: FontRef) => void; searchFonts: (q: string) => Promise<FontRef[]> }) {
  const [searching, setSearching] = useState(false);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<FontRef[]>([]);
  useEffect(() => {
    if (!searching || query.trim().length < 2) {
      setHits((h) => (h.length ? [] : h));
      return;
    }
    let live = true;
    const t = setTimeout(() => {
      searchFonts(query).then(
        (r) => live && setHits(r),
        () => live && setHits([]),
      );
    }, 200);
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [query, searching, searchFonts]);
  const value = searching ? "search" : String(line.chosen ?? "");
  return (
    <div className="space-y-1">
      <select
        aria-label="Font"
        className={FIELD}
        value={value}
        onChange={(e) => {
          if (e.target.value === "search") {
            setSearching(true);
            return;
          }
          setSearching(false);
          const c = line.candidates[Number(e.target.value)];
          if (c) onPick(c.font);
        }}
      >
        {line.chosen === null && <option value="">Choose a font…</option>}
        {line.candidates.map((c, i) => (
          <option key={`${c.font.source}:${c.font.family}:${c.font.weight}:${c.font.italic}`} value={String(i)}>
            {fontLabel(c.font)} · {fitPercent(c.iou)}
          </option>
        ))}
        <option value="search">Search fonts…</option>
      </select>
      {searching && (
        <div className="space-y-1">
          <input aria-label="Search fonts" className={FIELD} placeholder="Google Fonts or installed" spellCheck={false} value={query} onChange={(e) => setQuery(e.target.value)} autoFocus />
          {hits.length > 0 && (
            <ul className="max-h-40 overflow-y-auto rounded-md border bg-card">
              {hits.map((f) => (
                <li key={`${f.source}:${f.family}:${f.weight}:${f.italic}`}>
                  <button
                    type="button"
                    className="mac-ghost h-7 w-full justify-start px-2 text-[12px]"
                    onClick={() => {
                      setSearching(false);
                      setQuery("");
                      onPick(f);
                    }}
                  >
                    {fontLabel(f)}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}

function LineRow({ line, n, onChange, onHover, searchFonts }: { line: TextLine; n: number; onChange: (c: TextLineChange) => void; onHover: (id: string | null) => void; searchFonts: (q: string) => Promise<FontRef[]> }) {
  const [draft, setDraft] = useState(line.string);
  useEffect(() => setDraft(line.string), [line.string]);
  const commit = () => {
    const s = draft.trim();
    if (s && s !== line.string) onChange({ string: s });
    else setDraft(line.string);
  };
  const declined = line.status.kind === "declined" ? declineText(line.status.reason) : null;
  return (
    <div role="group" aria-label={`Line ${n}`} className="space-y-1.5 rounded-lg px-2 py-2 hover:bg-muted/60" onMouseEnter={() => onHover(line.id)} onMouseLeave={() => onHover(null)}>
      <div className="flex items-center gap-2">
        <input
          aria-label={`Text of line ${n}`}
          className={`${FIELD} flex-1`}
          value={draft}
          spellCheck={false}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === "Enter") (e.target as HTMLInputElement).blur();
            if (e.key === "Escape") setDraft(line.string);
          }}
        />
        <Switch aria-label={`Re-set line ${n} in a font`} checked={line.enabled} onCheckedChange={(on) => onChange({ enabled: on })} />
      </div>
      <FontPicker line={line} onPick={(font) => onChange({ font })} searchFonts={searchFonts} />
      <div className="flex items-center justify-between text-[11px] text-muted-foreground">
        {declined ? (
          <span>{declined}</span>
        ) : line.status.kind === "off" ? (
          <span>Traced as shapes</span>
        ) : line.fit ? (
          <span className="mac-tint inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 font-medium">Fit {fitPercent(line.fit.iou)}</span>
        ) : null}
      </div>
    </div>
  );
}

/** The inspector's Text: the lines found in the image, each with its string, its font, how well it fits and whether it is re-set. */
export function TextSection({ item, enabled, onChange, onHover, onCancel, onRetry, searchFonts }: TextSectionProps) {
  if (!enabled) return null;
  const t = textOf(item);
  const count = t.plan?.lines.length ?? 0;
  // a redraw's Revert restores traces that carry a text group while the plan is gone
  const staleGroup = !t.plan && !t.phase && !!shownAnswer(item)?.textGroup;
  return (
    <section aria-labelledby="text-section-title" className="space-y-2">
      <div className="flex items-center justify-between">
        <h4 id="text-section-title" className="flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
          <Type className="h-3.5 w-3.5" aria-hidden="true" />
          Text{t.plan ? ` · ${count} ${count === 1 ? "line" : "lines"}` : ""}
        </h4>
        {t.plan && <span className="text-[11px] text-muted-foreground">{t.plan.reader === "ai" ? "AI" : "On-device"}</span>}
      </div>
      {t.phase && (
        <div className="space-y-1.5">
          <p className="flex items-start gap-2 text-[12px] leading-snug text-muted-foreground" aria-live="polite">
            <span className="mac-dot mt-[5px] shrink-0 animate-pulse" aria-hidden="true" />
            <span className="min-w-0">
              <span className="block text-foreground">Reading the text…</span>
              <span className="block text-[11px]">{TEXT_PHASE_TEXT[t.phase]}</span>
            </span>
          </p>
          <button type="button" className="mac-button h-8 w-full" onClick={onCancel}>
            Cancel
          </button>
        </div>
      )}
      {t.error && !t.phase && (
        <div className="space-y-1.5">
          <p role="alert" className="text-[12px] leading-snug text-destructive">
            {textErrorText(t.error.code)}
          </p>
          <button type="button" className="mac-button h-8 w-full" onClick={onRetry}>
            Try again
          </button>
        </div>
      )}
      {t.plan?.aiError && (
        <p role="alert" className="text-[12px] leading-snug text-destructive">
          {textErrorText(t.plan.aiError)}
        </p>
      )}
      {t.plan?.fontsOffline && <p className="mac-tint rounded-lg px-2.5 py-1.5 text-[11px]">Google Fonts unreachable — installed fonts only</p>}
      {staleGroup && <p className="text-[12px] text-muted-foreground">Text was re-set — trace again to edit it</p>}
      {t.plan && count === 0 && !t.phase && <p className="text-[12px] text-muted-foreground">No text found</p>}
      {!t.plan && !t.asked && !t.phase && !t.error && !staleGroup && <p className="text-[12px] text-muted-foreground">Trace to read the text</p>}
      {t.plan && count > 0 && (
        <div className="-mx-2 space-y-0.5">
          {t.plan.lines.map((l, i) => (
            <LineRow key={l.id} line={l} n={i + 1} onChange={(c) => onChange(l.id, c)} onHover={onHover} searchFonts={searchFonts} />
          ))}
        </div>
      )}
    </section>
  );
}
