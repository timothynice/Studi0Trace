//! Google Fonts on demand: the only code that talks to Google. The css2 endpoint, asked with a non-browser user
//! agent, answers `@font-face` rules whose `src` is a TTF on fonts.gstatic.com; the file is kept in the app's
//! data folder under the font's cache name and never fetched twice. Nothing of the user's goes with the request
//! but the family name. A failure is never an error: the line is matched against installed fonts instead.
use super::catalog::cache_name;
use super::glyphs::Font;
use super::FontRef;
use crate::error::CommandError;
use std::path::PathBuf;
use std::time::Duration;

pub const DEFAULT_BASE: &str = "https://fonts.googleapis.com";
pub const BASE_ENV: &str = "STUDI0TRACE_GFONTS_BASE";
const UA: &str = concat!("Studi0Trace/", env!("CARGO_PKG_VERSION"), " (+https://github.com/timothynice/Studi0Trace)");
const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_FONT_BYTES: usize = 8 * 1024 * 1024;

/// `STUDI0TRACE_GFONTS_BASE` when it is an `https://` address or a loopback one; else Google.
pub fn base_url() -> String {
    match std::env::var(BASE_ENV) {
        Ok(b) if b.starts_with("https://") || crate::redraw::openai::is_loopback_base(&b) => b.trim_end_matches('/').to_string(),
        _ => DEFAULT_BASE.to_string(),
    }
}

pub struct GoogleFonts {
    dir: PathBuf,
    base: String,
    client: reqwest::Client,
}

#[derive(Debug, Default)]
pub struct Ensured {
    pub missing: Vec<FontRef>,
    pub offline: bool,
}

/// The first `src: url(…)` of the stylesheet, when it is a TTF.
pub fn parse_css(css: &str) -> Option<String> {
    let start = css.find("src: url(")? + "src: url(".len();
    let end = css[start..].find(')')? + start;
    let url = &css[start..end];
    url.ends_with(".ttf").then(|| url.to_string())
}

/// Err(true) when the request never got an answer (offline), Err(false) when this font cannot be had.
fn transport(e: reqwest::Error) -> bool {
    e.is_connect() || e.is_timeout()
}

impl GoogleFonts {
    pub fn new(dir: PathBuf, base: &str) -> Result<GoogleFonts, CommandError> {
        std::fs::create_dir_all(&dir).map_err(|e| CommandError::new(500, "io_error", format!("{}: {e}", dir.display())))?;
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        let base = base.trim().trim_end_matches('/').to_string();
        let mut builder = reqwest::Client::builder().timeout(TIMEOUT).connect_timeout(Duration::from_secs(10)).user_agent(UA);
        if crate::redraw::openai::is_loopback_base(&base) {
            builder = builder.no_proxy();
        }
        let client = builder.build().map_err(|_| CommandError::new(503, "offline", "Studi0Trace could not reach Google Fonts."))?;
        Ok(GoogleFonts { dir, base, client })
    }

    pub fn css_url(base: &str, font: &FontRef) -> String {
        format!("{}/css2?family={}:ital,wght@{},{}", base.trim_end_matches('/'), font.family.replace(' ', "+"), u8::from(font.italic), font.weight)
    }

    async fn fetch(&self, font: &FontRef) -> Result<(), bool> {
        let css = self.client.get(Self::css_url(&self.base, font)).send().await.map_err(transport)?;
        if !css.status().is_success() {
            return Err(false);
        }
        let css = css.text().await.map_err(transport)?;
        let url = parse_css(&css).ok_or(false)?;
        let ttf = self.client.get(url).send().await.map_err(transport)?;
        if !ttf.status().is_success() {
            return Err(false);
        }
        let bytes = ttf.bytes().await.map_err(transport)?;
        if bytes.is_empty() || bytes.len() > MAX_FONT_BYTES {
            return Err(false);
        }
        // a 200 whose body is not a font is not cached; the file is written beside its name and renamed into place,
        // so a failed write never leaves a partial file under the cache name
        if Font::from_bytes(bytes.to_vec(), 0).is_err() {
            return Err(false);
        }
        let name = cache_name(font);
        let part = self.dir.join(format!("{name}.part"));
        let written = std::fs::write(&part, &bytes).and_then(|()| std::fs::rename(&part, self.dir.join(&name)));
        if written.is_err() {
            let _ = std::fs::remove_file(&part);
            return Err(false);
        }
        Ok(())
    }

    /// Whether the font is in `dir` and is a font. A file that does not parse (a partial write of an older run, a
    /// stray file) is removed, so the font is fetched again instead of being skipped for good.
    fn cached(&self, font: &FontRef) -> bool {
        let path = self.dir.join(cache_name(font));
        let Ok(bytes) = std::fs::read(&path) else { return false };
        if Font::from_bytes(bytes, 0).is_ok() {
            return true;
        }
        let _ = std::fs::remove_file(&path);
        false
    }

    /// Every font not yet in `dir` is fetched: the css2 stylesheet (non-browser UA → TTF `src`), then the file. A
    /// font that cannot be fetched is `missing`; a transport failure sets `offline` and skips the rest. Never an error.
    pub async fn ensure(&self, fonts: &[FontRef]) -> Ensured {
        let mut out = Ensured::default();
        for font in fonts {
            if self.cached(font) {
                continue;
            }
            match self.fetch(font).await {
                Ok(()) => {}
                Err(true) => {
                    out.offline = true;
                    out.missing = Vec::new();
                    for f in fonts {
                        if !self.cached(f) && !out.missing.contains(f) {
                            out.missing.push(f.clone());
                        }
                    }
                    return out;
                }
                Err(false) => out.missing.push(font.clone()),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::Source;

    #[test]
    fn the_css_url_names_the_family_weight_and_style() {
        let f = FontRef { family: "Josefin Sans".into(), weight: 300, italic: false, source: Source::Google };
        assert_eq!(GoogleFonts::css_url("https://fonts.googleapis.com", &f), "https://fonts.googleapis.com/css2?family=Josefin+Sans:ital,wght@0,300");
        let i = FontRef { italic: true, ..f };
        assert!(GoogleFonts::css_url("http://127.0.0.1:9/", &i).ends_with("css2?family=Josefin+Sans:ital,wght@1,300"));
    }

    #[test]
    fn parses_the_ttf_source_out_of_the_stylesheet() {
        let css = "@font-face {\n  font-family: 'Josefin Sans';\n  src: url(https://fonts.gstatic.com/s/josefinsans/v34/abc.ttf) format('truetype');\n}";
        assert_eq!(parse_css(css).as_deref(), Some("https://fonts.gstatic.com/s/josefinsans/v34/abc.ttf"));
        assert_eq!(parse_css("nothing here"), None);
        assert_eq!(parse_css("src: url(https://x/y.woff2)"), None, "only a TTF is taken");
    }
}
