"""Builds src-tauri/assets/gfonts.json from Google Fonts' public metadata: per family, the category, the
variants and the measured features of its regular face (cap, x, stem, per-glyph widths) — the same measure
glyphs.rs makes of an installed font, computed here with fontTools so the two agree.

Run by hand to refresh the index (about ten minutes; it downloads ~1950 regular faces into a cache dir):
    ../../backend/.venv/bin/python tools/gfonts_index.py --cache /tmp/gfonts-cache
It needs `fonttools` and `requests` in that venv: `uv pip install -p ../../backend/.venv/bin/python fonttools requests`.
"""
import argparse, json, re, sys, time
from pathlib import Path

import requests
from fontTools.pens.boundsPen import BoundsPen
from fontTools.ttLib import TTFont

META = "https://fonts.google.com/metadata/fonts"
CSS = "https://fonts.googleapis.com/css2?family={family}:ital,wght@{ital},{weight}"
UA = {"User-Agent": "Studi0Trace-index/1 (+https://github.com/timothynice/Studi0Trace)"}
CHARS = [chr(c) for c in range(65, 91)] + [chr(c) for c in range(97, 123)] + [chr(c) for c in range(48, 58)]


def variants(entry):
    out = []
    for key in entry["fonts"]:
        italic = key.endswith("i")
        weight = int(key.rstrip("i"))
        out.append({"weight": weight, "italic": italic})
    return sorted(out, key=lambda v: (v["italic"], v["weight"]))


def regular(vs):
    uprights = [v["weight"] for v in vs if not v["italic"]] or [v["weight"] for v in vs]
    return min(uprights, key=lambda w: abs(w - 400))


def ttf_url(family, weight, italic):
    css = requests.get(CSS.format(family=family.replace(" ", "+"), ital=1 if italic else 0, weight=weight), headers=UA, timeout=30).text
    m = re.search(r"src: url\((https://fonts\.gstatic\.com[^)]+\.ttf)\)", css)
    return m.group(1) if m else None


def measure(path):
    f = TTFont(path)
    upem = f["head"].unitsPerEm
    os2 = f["OS/2"]
    gs = f.getGlyphSet()
    cmap = f.getBestCmap()

    def bbox(ch):
        g = cmap.get(ord(ch))
        if g is None:
            return None
        pen = BoundsPen(gs)
        gs[g].draw(pen)
        return pen.bounds

    cap = getattr(os2, "sCapHeight", 0) / upem
    if cap <= 0:
        b = bbox("H")
        cap = (b[3] - b[1]) / upem if b else 0.7
    xh = getattr(os2, "sxHeight", 0) / upem
    if xh <= 0:
        b = bbox("x")
        xh = (b[3] - b[1]) / upem if b else 0.5
    widths = []
    for ch in CHARS:
        b = bbox(ch)
        widths.append(int(round((b[2] - b[0]) / upem / cap * 100)) if b else 0)
    # the stem: the I's bbox width over the cap height — the same measure glyphs.rs makes
    bi = bbox("I")
    i_w = (bi[2] - bi[0]) / upem if bi else 0.1 * cap
    return {"cap": round(cap, 4), "x": round(xh, 4), "stem": round(i_w / cap, 4), "widths": [min(255, max(0, w)) for w in widths]}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cache", required=True)
    ap.add_argument("--out", default=str(Path(__file__).resolve().parents[1] / "src-tauri/assets/gfonts.json"))
    args = ap.parse_args()
    cache = Path(args.cache)
    cache.mkdir(parents=True, exist_ok=True)
    meta = requests.get(META, headers=UA, timeout=60).text
    meta = json.loads(meta[meta.index("{"):])  # the endpoint prefixes `)]}'`
    families = []
    for entry in meta["familyMetadataList"]:
        name = entry["family"]
        vs = variants(entry)
        if not vs or "latin" not in entry.get("subsets", []):
            continue
        w = regular(vs)
        path = cache / f"{name}-{w}.ttf"
        if not path.exists():
            url = ttf_url(name, w, False)
            if not url:
                print("no ttf for", name, file=sys.stderr)
                continue
            path.write_bytes(requests.get(url, headers=UA, timeout=60).content)
            time.sleep(0.05)
        try:
            feats = measure(path)
        except Exception as e:  # a font fontTools cannot read is left out
            print("skip", name, e, file=sys.stderr)
            continue
        families.append({"name": name, "category": entry["category"], "variants": vs, "features": feats})
    Path(args.out).write_text(json.dumps({"families": families}, separators=(",", ":")))
    print(len(families), "families →", args.out)


if __name__ == "__main__":
    main()
