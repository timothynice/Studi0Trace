//! The text group composed over a trace: per matched, enabled line, the glyph outlines as one path in the
//! ink's colour, and an invisible <text> carrying the string for search and screen readers when the setting
//! asks. Source-pixel coordinates; the frontend inserts the group before </svg>.
use super::catalog::Catalog;
use super::fit::{origins, path_data};
use super::glyphs::{Font, Shaped};
use super::{Fit, Status, TextPlan};
use std::path::Path;

/// The string without the control characters XML 1.0 cannot carry: U+0000–U+001F but tab, LF and CR, and U+007F.
/// A reader's string or an edit holding one would make the SVG invalid (a blank viewer, a corrupt export).
pub fn clean_text(s: &str) -> String {
    s.chars().filter(|c| !matches!(c, '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' | '\u{7f}')).collect()
}

/// `& < > "` as entities, the control characters `clean_text` drops left out.
pub fn escape_text(s: &str) -> String {
    clean_text(s).replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn num(v: f32) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s == "-0" {
        "0".into()
    } else {
        s
    }
}

/// The glyph outlines of `shaped` under `fit` as one SVG path `d`, in source pixels.
pub fn candidate_path(shaped: &Shaped, fit: &Fit) -> String {
    path_data(shaped, fit)
}

/// The hidden `<text>` element for a line: one x per character, where its glyph is drawn (spaces at their computed
/// place): a line laid out from its ink has nudges of ten pixels, which one start and a letter-spacing cannot place.
pub fn candidate_text(line_id: &str, string: &str, family: &str, shaped: &Shaped, fit: &Fit) -> String {
    let xs: Vec<String> = origins(shaped, fit).into_iter().map(num).collect();
    format!(
        "<text x=\"{}\" y=\"{}\" font-family=\"{}\" font-size=\"{}\" fill=\"none\" pointer-events=\"none\" data-line=\"{}\">{}</text>",
        xs.join(" "),
        num(fit.baseline),
        escape_text(family),
        num(fit.size),
        line_id,
        escape_text(string)
    )
}

/// The group, and the ids of the lines it carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composed {
    pub group: String,
    pub lines: Vec<String>,
}

/// `<g id="text">…</g>` for the plan's matched, enabled lines: one path per line in the ink colour, and the hidden
/// text when asked. A line whose font cannot be located or loaded is skipped, and is not in `lines`: the caller
/// paints out only what is composed.
pub fn compose_group(plan: &TextPlan, hidden_text: bool, catalog: &Catalog, cache_dir: &Path) -> Composed {
    let mut out = String::from("<g id=\"text\">");
    let mut lines = Vec::new();
    for line in &plan.lines {
        if line.status != Status::Matched || !line.enabled {
            continue;
        }
        let (Some(fit), Some(chosen)) = (&line.fit, line.chosen.and_then(|i| line.candidates.get(i))) else { continue };
        let Some((path, index)) = catalog.locate(&chosen.font, cache_dir) else { continue };
        let Ok(font) = Font::load(&path, index) else { continue };
        let shaped = font.shape(&line.string);
        let d = candidate_path(&shaped, fit);
        let [r, g, b] = line.colour;
        out.push_str(&format!("<path d=\"{d}\" fill=\"#{r:02x}{g:02x}{b:02x}\" data-line=\"{}\"/>", line.id));
        lines.push(line.id.clone());
        if hidden_text {
            out.push_str(&candidate_text(&line.id, &line.string, &chosen.font.family, &shaped, fit));
        }
    }
    out.push_str("</g>");
    Composed { group: out, lines }
}

/// `compose_group`'s group alone.
pub fn text_group(plan: &TextPlan, hidden_text: bool, catalog: &Catalog, cache_dir: &Path) -> String {
    compose_group(plan, hidden_text, catalog, cache_dir).group
}
