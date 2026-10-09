//! A font as the fit sees it: shaped glyphs with outlines in em units (y up), and the features the shortlist
//! ranks by. Shaping runs with kerning off: the fit's per-glyph nudges absorb the pairs, and the fixture set is
//! drawn glyph by glyph the same way.
use crate::error::CommandError;
use rustybuzz::ttf_parser::{self, GlyphId, OutlineBuilder};
use rustybuzz::{Feature, UnicodeBuffer};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tiny_skia::{Path as SkPath, PathBuilder};

pub struct Font {
    data: Vec<u8>,
    index: u32,
}

impl std::fmt::Debug for Font {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Font({} bytes, index {})", self.data.len(), self.index)
    }
}

pub struct ShapedGlyph {
    pub ch: char,
    /// Em units, y up, origin at the glyph's origin; None for a glyph with no outline (a space).
    pub path: Option<SkPath>,
    /// Em units.
    pub advance: f32,
    pub x_offset: f32,
}

pub struct Shaped {
    pub glyphs: Vec<ShapedGlyph>,
    pub cap_height: f32,
    pub x_height: f32,
}

impl Shaped {
    pub fn advance(&self) -> f32 {
        self.glyphs.iter().map(|g| g.advance).sum()
    }

    /// The outlines' extent in y, em units.
    pub fn bbox_height(&self) -> f32 {
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for g in self.glyphs.iter().filter_map(|g| g.path.as_ref()) {
            let b = g.bounds();
            lo = lo.min(b.top());
            hi = hi.max(b.bottom());
        }
        if lo > hi { 0.0 } else { hi - lo }
    }
}

/// What the shortlist ranks a face by.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Features {
    /// Cap height over em.
    pub cap: f32,
    /// x-height over em.
    pub x: f32,
    /// Stem thickness over cap height, the mean of the I's width and the H's left stem.
    pub stem: f32,
    /// Glyph bbox width over cap height, in hundredths, for A–Z a–z 0–9; 0 for a glyph the face lacks.
    #[serde(with = "widths_array")]
    pub widths: [u8; 62],
}

/// serde has no impl for arrays past 32: the widths travel as a plain list of 62 numbers.
mod widths_array {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(widths: &[u8; 62], s: S) -> Result<S::Ok, S::Error> {
        widths.as_slice().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 62], D::Error> {
        let v = Vec::<u8>::deserialize(d)?;
        <[u8; 62]>::try_from(v).map_err(|v| serde::de::Error::invalid_length(v.len(), &"62 widths"))
    }
}

pub fn width_index(ch: char) -> Option<usize> {
    match ch {
        'A'..='Z' => Some(ch as usize - 'A' as usize),
        'a'..='z' => Some(26 + ch as usize - 'a' as usize),
        '0'..='9' => Some(52 + ch as usize - '0' as usize),
        _ => None,
    }
}

struct Builder {
    pb: PathBuilder,
    scale: f32,
}

impl OutlineBuilder for Builder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.pb.move_to(x * self.scale, y * self.scale);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.pb.line_to(x * self.scale, y * self.scale);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.pb.quad_to(x1 * self.scale, y1 * self.scale, x * self.scale, y * self.scale);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.pb.cubic_to(x1 * self.scale, y1 * self.scale, x2 * self.scale, y2 * self.scale, x * self.scale, y * self.scale);
    }
    fn close(&mut self) {
        self.pb.close();
    }
}

impl Font {
    pub fn load(path: &Path, index: u32) -> Result<Font, CommandError> {
        let data = std::fs::read(path).map_err(|e| CommandError::new(500, "io_error", format!("{}: {e}", path.display())))?;
        Font::from_bytes(data, index)
    }

    pub fn from_bytes(data: Vec<u8>, index: u32) -> Result<Font, CommandError> {
        if ttf_parser::Face::parse(&data, index).is_err() {
            return Err(super::error("bad_font"));
        }
        Ok(Font { data, index })
    }

    pub fn face(&self) -> rustybuzz::Face<'_> {
        rustybuzz::Face::from_slice(&self.data, self.index).expect("checked in from_bytes")
    }

    pub fn units_per_em(&self) -> f32 {
        self.face().units_per_em() as f32
    }

    fn glyph_height(&self, ch: char) -> Option<f32> {
        let face = self.face();
        let id = face.glyph_index(ch)?;
        let b = face.glyph_bounding_box(id)?;
        Some((b.y_max - b.y_min) as f32 / self.units_per_em())
    }

    pub fn cap_height(&self) -> f32 {
        self.face().capital_height().map(|h| h as f32 / self.units_per_em()).filter(|h| *h > 0.0).or_else(|| self.glyph_height('H')).unwrap_or(0.7)
    }

    pub fn x_height(&self) -> f32 {
        self.face().x_height().map(|h| h as f32 / self.units_per_em()).filter(|h| *h > 0.0).or_else(|| self.glyph_height('x')).unwrap_or(0.5)
    }

    fn outline(&self, face: &rustybuzz::Face<'_>, id: GlyphId) -> Option<SkPath> {
        let mut b = Builder { pb: PathBuilder::new(), scale: 1.0 / self.units_per_em() };
        face.outline_glyph(id, &mut b)?;
        b.pb.finish()
    }

    /// The string shaped with kerning off, one glyph per cluster; a character the face lacks is shaped as
    /// .notdef and keeps its advance.
    pub fn shape(&self, text: &str) -> Shaped {
        let face = self.face();
        let upem = self.units_per_em();
        let mut buf = UnicodeBuffer::new();
        buf.push_str(text);
        let kern_off = Feature::new(rustybuzz::ttf_parser::Tag::from_bytes(b"kern"), 0, ..);
        let out = rustybuzz::shape(&face, &[kern_off], buf);
        let chars: Vec<char> = text.chars().collect();
        let mut glyphs = Vec::with_capacity(out.len());
        for (info, pos) in out.glyph_infos().iter().zip(out.glyph_positions()) {
            let ch = chars.get(info.cluster as usize).copied().unwrap_or('\u{FFFD}');
            let path = self.outline(&face, GlyphId(info.glyph_id as u16));
            glyphs.push(ShapedGlyph { ch, path, advance: pos.x_advance as f32 / upem, x_offset: pos.x_offset as f32 / upem });
        }
        Shaped { glyphs, cap_height: self.cap_height(), x_height: self.x_height() }
    }

    /// Stem thickness: the I's bbox width over the cap height (tools/gfonts_index.py measures the same).
    pub fn measure(&self) -> Features {
        let cap = self.cap_height();
        let x = self.x_height();
        let face = self.face();
        let upem = self.units_per_em();
        let mut widths = [0u8; 62];
        for ch in ('A'..='Z').chain('a'..='z').chain('0'..='9') {
            if let Some(b) = face.glyph_index(ch).and_then(|id| face.glyph_bounding_box(id)) {
                let w = (b.x_max - b.x_min) as f32 / upem / cap * 100.0;
                widths[width_index(ch).unwrap()] = w.round().clamp(0.0, 255.0) as u8;
            }
        }
        let i_width = face.glyph_index('I').and_then(|id| face.glyph_bounding_box(id)).map(|b| (b.x_max - b.x_min) as f32 / upem).unwrap_or(0.1 * cap);
        Features { cap, x, stem: i_width / cap, widths }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/text/fonts").join(name)
    }

    #[test]
    fn shapes_a_string_into_glyphs_with_advances_and_outlines() {
        let font = Font::load(&fixture("Montserrat-Regular.ttf"), 0).unwrap();
        let shaped = font.shape("EST. 2026");
        assert_eq!(shaped.glyphs.len(), 9);
        assert_eq!(shaped.glyphs[0].ch, 'E');
        assert!(shaped.glyphs[0].path.is_some());
        assert!(shaped.glyphs[4].path.is_none(), "a space has no outline");
        assert!(shaped.glyphs[4].advance > 0.1 && shaped.glyphs[4].advance < 0.5);
        assert!((shaped.advance() - shaped.glyphs.iter().map(|g| g.advance).sum::<f32>()).abs() < 1e-6);
        assert!((0.6..0.8).contains(&shaped.cap_height), "{}", shaped.cap_height);
        // the E's outline spans the cap height: y up, from 0 to cap
        let b = shaped.glyphs[0].path.as_ref().unwrap().bounds();
        assert!(b.top().abs() < 0.02 && (b.bottom() - shaped.cap_height).abs() < 0.03 || b.bottom().abs() < 0.02 && (b.top().abs() - shaped.cap_height).abs() < 0.03, "{b:?}");
    }

    #[test]
    fn measures_features_in_the_expected_ranges() {
        let light = Font::load(&fixture("JosefinSans-Light.ttf"), 0).unwrap().measure();
        let regular = Font::load(&fixture("Montserrat-Regular.ttf"), 0).unwrap().measure();
        assert!(light.stem < regular.stem, "a light face has thinner stems: {} vs {}", light.stem, regular.stem);
        assert!((0.05..0.2).contains(&regular.stem), "{}", regular.stem);
        assert!(regular.widths[width_index('W').unwrap()] > regular.widths[width_index('I').unwrap()]);
        assert_eq!(width_index('a'), Some(26));
        assert_eq!(width_index('9'), Some(61));
        assert_eq!(width_index('.'), None);
    }

    #[test]
    fn a_file_that_is_not_a_font_is_refused() {
        let err = Font::from_bytes(vec![0u8; 100], 0).unwrap_err();
        assert_eq!(err.code(), Some("bad_font"));
    }
}
