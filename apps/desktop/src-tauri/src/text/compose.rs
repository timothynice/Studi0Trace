//! The text group composed over a trace: per matched, enabled line, the glyph outlines as one path in the
//! ink's colour, and an invisible <text> carrying the string for search and screen readers when the setting
//! asks. Source-pixel coordinates; the frontend inserts the group before </svg>.
use super::catalog::Catalog;
use super::fit::path_data;
use super::glyphs::Font;
use super::{Status, TextPlan};
use std::path::Path;

/// `& < > "` as entities.
pub fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
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
        let d = path_data(&shaped, fit);
        let [r, g, b] = line.colour;
        out.push_str(&format!("<path d=\"{d}\" fill=\"#{r:02x}{g:02x}{b:02x}\" data-line=\"{}\"/>", line.id));
        lines.push(line.id.clone());
        if hidden_text {
            out.push_str(&format!(
                "<text x=\"{}\" y=\"{}\" font-family=\"{}\" font-size=\"{}\" letter-spacing=\"{}\" fill=\"none\" pointer-events=\"none\" data-line=\"{}\">{}</text>",
                num(fit.x),
                num(fit.baseline),
                escape_text(&chosen.font.family),
                num(fit.size),
                num(fit.tracking),
                line.id,
                escape_text(&line.string)
            ));
        }
    }
    out.push_str("</g>");
    Composed { group: out, lines }
}

/// `compose_group`'s group alone.
pub fn text_group(plan: &TextPlan, hidden_text: bool, catalog: &Catalog, cache_dir: &Path) -> String {
    compose_group(plan, hidden_text, catalog, cache_dir).group
}
