//! Text re-setting: the lines of text in an image are found and read (vision.rs, reader_openai.rs), the font is
//! matched (catalog.rs, fonts.rs) and fitted (glyphs.rs, ink.rs, fit.rs), the text is painted out of what the
//! trace sees (ink.rs) and the glyph outlines are composed on top (compose.rs). Nothing here is seen by the
//! engine, the core or the worker.
pub mod catalog;
pub mod compose;
pub mod fit;
pub mod fonts;
pub mod glyphs;
pub mod ink;
pub mod plan;
pub mod plans;
pub mod reader;
pub mod reader_openai;
pub mod vision;

use crate::error::CommandError;
use serde::{Deserialize, Serialize};

/// A line whose best fit scores under this IoU is traced as shapes. Set on real sources (AI-made logos, hairline
/// strokes, see the spec's Status): a right face there scores 0.67-0.83 where the crisp fixtures score 0.84 up, so
/// the IoU is a floor and the weight is judged by the stroke (`STROKE_AGREE`).
pub const FIT_ACCEPT: f32 = 0.6;
/// A fit whose drawn stroke differs from the ink's by more than this share is the wrong weight, and not accepted.
pub const STROKE_AGREE: f32 = 0.35;
/// 90th-percentile ΔE of the covered pixels from the ink colour above which the ink is a gradient.
pub const INK_SPREAD_MAX: f32 = 8.0;
/// The share by which the ink's segment count may differ from the string's non-space characters.
pub const MISMATCH_SHARE: f32 = 0.34;
/// A quad turned more than this is declined.
pub const ROTATED_DEG: f32 = 5.0;
/// Families fitted per line beyond the reader's hints.
pub const SHORTLIST: usize = 12;
/// How far one glyph may be nudged along the line, in px.
pub const NUDGE_MAX: f32 = 2.0;
/// Vision observations under this confidence are dropped.
pub const VISION_MIN_CONFIDENCE: f32 = 0.3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Google,
    Installed,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FontRef {
    pub family: String,
    pub weight: u16,
    pub italic: bool,
    pub source: Source,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Class {
    Type,
    Script,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Script,
    Custom,
    Gradient,
    Mismatch,
    NoFit,
    Rotated,
    Fonts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "kind", content = "reason")]
pub enum Status {
    Matched,
    Declined(Reason),
    Off,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fit {
    pub size: f32,
    pub baseline: f32,
    pub x: f32,
    pub tracking: f32,
    pub nudges: Vec<f32>,
    pub iou: f32,
    /// The drawn stroke's width over the ink's (`fit::stroke_ratio`): 1 is the ink's weight, 0 unmeasured.
    #[serde(default)]
    pub stroke: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub font: FontRef,
    pub iou: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextLine {
    pub id: String,
    pub quad: [[f32; 2]; 4],
    pub string: String,
    pub class: Class,
    pub colour: [u8; 3],
    pub status: Status,
    pub candidates: Vec<Candidate>,
    pub chosen: Option<usize>,
    pub enabled: bool,
    pub fit: Option<Fit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReaderKind {
    Vision,
    Ai,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextPlan {
    pub image: String,
    pub reader: ReaderKind,
    pub lines: Vec<TextLine>,
    pub ai_error: Option<String>,
    pub fonts_offline: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Finding,
    Reading,
    Matching,
    Done,
    Failed,
}

/// A text failure by its code: the app's own, else redraw's table.
pub fn error(code: &'static str) -> CommandError {
    match code {
        "text_unavailable" => CommandError::new(501, code, "Text recognition needs macOS 10.15 or later."),
        "bad_font" => CommandError::new(422, code, "The font file could not be read."),
        _ => crate::redraw::error(code),
    }
}

pub use plan::{plan, set_line, Deps, LineChange, Planned};
pub use plans::{bytes_hash, Entry, Plans};
