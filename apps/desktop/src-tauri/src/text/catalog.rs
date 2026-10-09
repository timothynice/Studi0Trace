//! The fonts a line can be set in: Google Fonts' families from the embedded index (assets/gfonts.json, built by
//! tools/gfonts_index.py) and the fonts installed on this Mac (fontdb). The shortlist ranks families by how the
//! line's measured per-character widths and stem thickness match each family's features.
use super::glyphs::{width_index, Features, Font};
use super::ink::LineInk;
use super::{FontRef, Source};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const INDEX: &str = include_str!("../../assets/gfonts.json");

#[derive(Debug, Clone, Deserialize)]
pub struct Variant {
    pub weight: u16,
    pub italic: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Family {
    pub name: String,
    pub category: String,
    pub variants: Vec<Variant>,
    pub features: Features,
}

#[derive(Deserialize)]
struct Index {
    families: Vec<Family>,
}

pub struct Catalog {
    google: Vec<Family>,
    installed: fontdb::Database,
    measured: Mutex<HashMap<String, Option<Features>>>,
}

pub fn cache_name(font: &FontRef) -> String {
    format!("{}-{}{}.ttf", font.family, font.weight, if font.italic { "i" } else { "" })
}

/// Google's thickness 1–10 per weight is not used; the weight for a stem is found here and clamped to the
/// family's variants by the caller.
pub fn weight_for_stem(stem_over_cap: f32) -> u16 {
    match stem_over_cap {
        s if s < 0.07 => 300,
        s if s < 0.10 => 400,
        s if s < 0.13 => 500,
        s if s < 0.16 => 600,
        s if s < 0.19 => 700,
        _ => 800,
    }
}

/// The cap height in px of a line's ink: the median vertical extent of the segments that are capitals (or, with
/// none, digits, or tall lowercase letters over their usual 1.07 of the cap), else the ink's own height. The
/// ink's height itself carries the ascenders and descenders, which made every lowercase width read narrow.
fn cap_px(ink: &LineInk, chars: &[char]) -> f32 {
    let Some((_, t, _, b)) = ink.ink_bounds else { return 0.0 };
    let extent = |l: u32, r: u32| -> Option<f32> {
        let rows: Vec<u32> = (t..b).filter(|y| (l..r).any(|x| ink.mask[(y * ink.width + x) as usize])).collect();
        Some((rows.last()? - rows.first()? + 1) as f32)
    };
    let median = |pick: &dyn Fn(char) -> bool| -> Option<f32> {
        let mut v: Vec<f32> = chars.iter().zip(&ink.segments).filter(|(c, _)| pick(**c)).filter_map(|(_, (l, r))| extent(*l, *r)).collect();
        v.sort_by(f32::total_cmp);
        v.get(v.len() / 2).copied()
    };
    median(&|c| c.is_ascii_uppercase() && !"QJ".contains(c))
        .or_else(|| median(&|c| c.is_ascii_digit()))
        .or_else(|| median(&|c| "bdhkl".contains(c)).map(|h| h / 1.07))
        .unwrap_or((b - t) as f32)
}

/// The ink's per-character bbox widths over its cap height, for the characters of `string` that have a width
/// index, in order; a line whose segments do not number the string's non-space characters gives an empty list.
pub fn measured_widths(ink: &LineInk, string: &str) -> Vec<(char, f32)> {
    let chars: Vec<char> = string.chars().filter(|c| !c.is_whitespace()).collect();
    if chars.len() != ink.segments.len() {
        return Vec::new();
    }
    let cap = cap_px(ink, &chars);
    if cap <= 0.0 {
        return Vec::new();
    }
    chars.iter().zip(&ink.segments).filter(|(c, _)| width_index(**c).is_some()).map(|(c, (l, r))| (*c, (r - l) as f32 / cap)).collect()
}

/// Stem thickness over cap height: the median horizontal ink run at the rows between 40 % and 60 % of the ink's height.
pub fn measured_stem(ink: &LineInk) -> f32 {
    let Some((_, t, _, b)) = ink.ink_bounds else { return 0.1 };
    let cap = (b - t) as f32;
    let mut runs = Vec::new();
    for y in t + (cap * 0.4) as u32..t + (cap * 0.6) as u32 {
        let mut run = 0u32;
        for x in 0..ink.width {
            if ink.mask[(y * ink.width + x) as usize] {
                run += 1;
            } else if run > 0 {
                runs.push(run);
                run = 0;
            }
        }
    }
    if runs.is_empty() {
        return 0.1;
    }
    runs.sort_unstable();
    runs[runs.len() / 2] as f32 / cap
}

/// How much a family's widths are charged for the scale they need to match the line's: weight moves every width
/// together (Josefin Sans Light is about 8 % narrower than its regular, which is what the index holds), so each
/// family is compared at its own best scale and only the scale's distance from 1 counts, lightly.
const SCALE_PENALTY: f32 = 0.07;

/// A family's distance from a line: the mean absolute difference between the line's per-character widths and
/// the family's, at the family's best scale, plus the scale's penalty. The stem is not in it: the line's stem is
/// a run of ink pixels and the family's is its I's bbox width, which are one thing in a sans and not in a serif
/// (Playfair's I is 0.36 of the cap, its stems 0.15), and a family's regular says nothing of its other weights.
/// The stem picks the weight instead (`weight_for_stem`). A line with no widths has no preference.
fn distance(f: &Features, widths: &[(char, f32)]) -> f32 {
    let pairs: Vec<(f32, f32)> = widths
        .iter()
        .filter_map(|(c, w)| {
            let fw = f.widths[width_index(*c)?] as f32 / 100.0;
            (fw > 0.0).then_some((fw, *w))
        })
        .collect();
    if pairs.is_empty() {
        return 0.5;
    }
    let scale = (pairs.iter().map(|(fw, w)| fw * w).sum::<f32>() / pairs.iter().map(|(fw, _)| fw * fw).sum::<f32>()).max(1e-3);
    pairs.iter().map(|(fw, w)| (fw * scale - w).abs()).sum::<f32>() / pairs.len() as f32 + SCALE_PENALTY * scale.ln().abs()
}

impl Catalog {
    pub fn load() -> Catalog {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        Catalog::from_index(INDEX, db)
    }

    pub fn from_index(json: &str, installed: fontdb::Database) -> Catalog {
        let index: Index = serde_json::from_str(json).expect("the font index is JSON");
        Catalog { google: index.families, installed, measured: Mutex::new(HashMap::new()) }
    }

    pub fn families(&self) -> usize {
        self.google.len()
    }

    fn installed_families(&self) -> Vec<String> {
        let mut names: Vec<String> = self.installed.faces().filter_map(|f| f.families.first().map(|(n, _)| n.clone())).collect();
        names.sort();
        names.dedup();
        names
    }

    fn installed_face(&self, family: &str, weight: u16, italic: bool) -> Option<&fontdb::FaceInfo> {
        self.installed.faces().filter(|f| f.families.iter().any(|(n, _)| n == family) && (f.style == fontdb::Style::Italic) == italic).min_by_key(|f| (f.weight.0 as i32 - weight as i32).abs())
    }

    fn installed_features(&self, family: &str) -> Option<Features> {
        let mut cache = self.measured.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(f) = cache.get(family) {
            return f.clone();
        }
        let feats = self.installed_face(family, 400, false).and_then(|face| match &face.source {
            fontdb::Source::File(p) => Font::load(p, face.index).ok().map(|f| f.measure()),
            fontdb::Source::Binary(b) => Font::from_bytes(b.as_ref().as_ref().to_vec(), face.index).ok().map(|f| f.measure()),
            fontdb::Source::SharedFile(p, _) => Font::load(p, face.index).ok().map(|f| f.measure()),
        });
        cache.insert(family.to_string(), feats.clone());
        feats
    }

    fn nearest_weight(weights: &[u16], want: u16) -> u16 {
        weights.iter().copied().min_by_key(|w| (*w as i32 - want as i32).abs()).unwrap_or(400)
    }

    /// Ranked families for a line: the measured per-character widths (chars of `string` with a width index) and
    /// the stem over cap, against each family's features; installed families are measured on first use. The
    /// nearest `n`, each at the upright weight nearest the stem's weight class.
    pub fn shortlist(&self, _string: &str, widths: &[(char, f32)], stem: f32, n: usize) -> Vec<FontRef> {
        let want = weight_for_stem(stem);
        let mut scored: Vec<(f32, FontRef)> = self
            .google
            .iter()
            .map(|f| {
                let weights: Vec<u16> = f.variants.iter().filter(|v| !v.italic).map(|v| v.weight).collect();
                (distance(&f.features, widths), FontRef { family: f.name.clone(), weight: Self::nearest_weight(&weights, want), italic: false, source: Source::Google })
            })
            .collect();
        for family in self.installed_families() {
            if let Some(feats) = self.installed_features(&family) {
                let weights = self.weights_of(&family, Source::Installed);
                scored.push((distance(&feats, widths), FontRef { family, weight: Self::nearest_weight(&weights, want), italic: false, source: Source::Installed }));
            }
        }
        scored.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.family.cmp(&b.1.family)));
        scored.into_iter().map(|(_, f)| f).take(n).collect()
    }

    /// Prefix match over Google then installed family names, at the regular weight.
    pub fn search(&self, query: &str, n: usize) -> Vec<FontRef> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return Vec::new();
        }
        let mut out: Vec<FontRef> = self
            .google
            .iter()
            .filter(|f| f.name.to_lowercase().starts_with(&q))
            .map(|f| FontRef { family: f.name.clone(), weight: Self::nearest_weight(&f.variants.iter().filter(|v| !v.italic).map(|v| v.weight).collect::<Vec<_>>(), 400), italic: false, source: Source::Google })
            .collect();
        out.extend(self.installed_families().into_iter().filter(|f| f.to_lowercase().starts_with(&q)).map(|family| {
            let weight = Self::nearest_weight(&self.weights_of(&family, Source::Installed), 400);
            FontRef { family, weight, italic: false, source: Source::Installed }
        }));
        out.truncate(n);
        out
    }

    pub fn weights_of(&self, family: &str, source: Source) -> Vec<u16> {
        let mut w: Vec<u16> = match source {
            Source::Google => self.google.iter().find(|f| f.name == family).map(|f| f.variants.iter().filter(|v| !v.italic).map(|v| v.weight).collect()).unwrap_or_default(),
            Source::Installed => self.installed.faces().filter(|f| f.families.iter().any(|(n, _)| n == family) && f.style != fontdb::Style::Italic).map(|f| f.weight.0).collect(),
        };
        w.sort_unstable();
        w.dedup();
        w
    }

    /// Where the font file is: a Google family's cache path from `cache_dir`, an installed face's file + index.
    pub fn locate(&self, font: &FontRef, cache_dir: &Path) -> Option<(PathBuf, u32)> {
        match font.source {
            Source::Google => {
                let p = cache_dir.join(cache_name(font));
                p.exists().then_some((p, 0))
            }
            Source::Installed => self.installed_face(&font.family, font.weight, font.italic).and_then(|f| match &f.source {
                fontdb::Source::File(p) | fontdb::Source::SharedFile(p, _) => Some((p.clone(), f.index)),
                fontdb::Source::Binary(_) => None,
            }),
        }
    }
}
