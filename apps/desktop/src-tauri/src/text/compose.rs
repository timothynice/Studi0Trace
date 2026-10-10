//! What a line's candidates carry for the frontend to compose over a trace: the glyph outlines as one SVG path `d`,
//! and the invisible <text> element carrying the string for search and screen readers. Source-pixel coordinates.
use super::fit::{origins, path_data};
use super::glyphs::Shaped;
use super::Fit;

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
