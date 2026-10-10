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
/// Fonts fetched at once by one `ensure`.
const CONCURRENT_FETCHES: usize = 6;

/// A `.part` name no other write in this process or another shares: two plans fetching one font each write their
/// own and rename it into place, and the second rename replaces the first complete file with an identical one.
fn part_name(name: &str) -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    format!("{name}.{}.{}.part", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

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
        // what a run that died mid-write left behind; a cache is made once per app, before anything is fetched
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                if e.file_name().to_string_lossy().ends_with(".part") {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
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
        // a 200 whose body is not a font is not cached; the file is written beside its name under a name of its own
        // and renamed into place (an atomic replace), so a failed write never leaves a partial file under the cache
        // name and two fetches of one font never write into one file
        if Font::from_bytes(bytes.to_vec(), 0).is_err() {
            return Err(false);
        }
        let name = cache_name(font);
        let part = self.dir.join(part_name(&name));
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

    /// Every font not yet in `dir` is fetched, `CONCURRENT_FETCHES` at a time: the css2 stylesheet (non-browser UA →
    /// TTF `src`), then the file. A font that cannot be fetched is `missing`; a transport failure sets `offline`,
    /// stops the fetches still running and makes every font not in the cache missing. Never an error.
    pub async fn ensure(&self, fonts: &[FontRef]) -> Ensured {
        use futures_util::StreamExt;
        let mut wanted: Vec<FontRef> = Vec::new();
        for f in fonts {
            if !wanted.contains(f) && !self.cached(f) {
                wanted.push(f.clone());
            }
        }
        let mut out = Ensured::default();
        // owned fonts: a future over a borrowed one is not `Send` for every lifetime, which a spawned plan needs
        let mut fetches = futures_util::stream::iter(wanted)
            .map(|f| async move {
                let got = self.fetch(&f).await;
                (f, got)
            })
            .buffer_unordered(CONCURRENT_FETCHES);
        let mut missing: Vec<FontRef> = Vec::new();
        while let Some((font, got)) = fetches.next().await {
            match got {
                Ok(()) => {}
                Err(true) => {
                    out.offline = true;
                    break;
                }
                Err(false) => missing.push(font),
            }
        }
        // dropping the stream ends the fetches still running (a write and its rename have no await between them)
        drop(fetches);
        if out.offline {
            for f in fonts {
                if !self.cached(f) && !out.missing.contains(f) {
                    out.missing.push(f.clone());
                }
            }
        } else {
            // in the order asked, whatever order the fetches finished in
            out.missing = fonts.iter().filter(|f| missing.contains(f)).fold(Vec::new(), |mut v, f| {
                if !v.contains(f) {
                    v.push(f.clone());
                }
                v
            });
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
