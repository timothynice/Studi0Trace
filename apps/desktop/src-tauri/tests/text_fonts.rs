//! Downloading: the stylesheet is asked for with a non-browser user agent, the TTF it names is fetched into the
//! cache under the font's cache name, a cached font is not fetched again, and a dead server means `offline`, never
//! an error.
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use studi0trace_desktop::text::catalog::cache_name;
use studi0trace_desktop::text::fonts::GoogleFonts;
use studi0trace_desktop::text::{FontRef, Source};

type Reply = (u16, &'static str, Vec<u8>);

/// A one-connection-per-reply mock on loopback. The replies are built from the mock's own base, so a stylesheet
/// can name a file on the same server. Returns the base and every request it received, as text.
fn serve(replies: impl FnOnce(&str) -> Vec<Reply>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let replies = replies(&base);
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let log = seen.clone();
    std::thread::spawn(move || {
        for (status, ctype, body) in replies {
            let Ok((mut s, _)) = listener.accept() else { return };
            let mut buf = vec![0u8; 8192];
            let n = s.read(&mut buf).unwrap_or(0);
            log.lock().unwrap().push(String::from_utf8_lossy(&buf[..n]).to_string());
            let _ = write!(s, "HTTP/1.1 {status} OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = s.write_all(&body);
        }
    });
    (base, seen)
}

fn font() -> FontRef {
    FontRef { family: "Josefin Sans".into(), weight: 300, italic: false, source: Source::Google }
}

/// A scratch directory removed when it drops, assertion failure or not.
struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("studi0trace-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Scratch(dir)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn fetches_the_stylesheet_then_the_ttf_into_the_cache_once() {
    let dir = Scratch::new("fonts");
    let ttf = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/text/fonts/JosefinSans-Light.ttf")).unwrap();
    let served = ttf.clone();
    let (base, seen) = serve(move |base| {
        // the stylesheet names the TTF on the same mock
        let css = format!("@font-face {{ src: url({base}/s/josefin.ttf) format('truetype'); }}");
        vec![(200, "text/css", css.into_bytes()), (200, "font/ttf", served)]
    });
    let gf = GoogleFonts::new(dir.0.clone(), &base).unwrap();
    let outcome = tauri::async_runtime::block_on(gf.ensure(&[font()]));
    assert!(!outcome.offline && outcome.missing.is_empty(), "{:?}", outcome.missing);
    assert_eq!(std::fs::read(dir.0.join(cache_name(&font()))).unwrap(), ttf);
    let log = seen.lock().unwrap();
    assert!(log[0].starts_with("GET /css2?family=Josefin+Sans:ital,wght@0,300"), "{}", log[0]);
    assert!(log[0].to_lowercase().contains("user-agent: studi0trace"), "{}", log[0]);
    assert!(log[1].starts_with("GET /s/josefin.ttf"), "{}", log[1]);
    drop(log);
    // cached: no request
    let again = tauri::async_runtime::block_on(gf.ensure(&[font()]));
    assert!(again.missing.is_empty() && !again.offline);
    assert_eq!(seen.lock().unwrap().len(), 2);
}

#[test]
fn a_dead_server_is_offline_not_an_error_and_a_404_is_missing() {
    let dir = Scratch::new("fonts-dead");
    let gf = GoogleFonts::new(dir.0.clone(), "http://127.0.0.1:9").unwrap();
    let out = tauri::async_runtime::block_on(gf.ensure(&[font()]));
    assert!(out.offline);
    assert_eq!(out.missing, vec![font()]);
    let (base, _) = serve(|_| vec![(404, "text/plain", b"no".to_vec())]);
    let gf = GoogleFonts::new(dir.0.clone(), &base).unwrap();
    let out = tauri::async_runtime::block_on(gf.ensure(&[font()]));
    assert!(!out.offline && out.missing == vec![font()]);
}

#[test]
fn a_body_that_is_not_a_font_is_missing_and_leaves_nothing_in_the_cache() {
    let dir = Scratch::new("fonts-garbage");
    let (base, _) = serve(|base| {
        let css = format!("@font-face {{ src: url({base}/s/josefin.ttf) format('truetype'); }}");
        vec![(200, "text/css", css.into_bytes()), (200, "font/ttf", b"<html>not a font</html>".to_vec())]
    });
    let gf = GoogleFonts::new(dir.0.clone(), &base).unwrap();
    let out = tauri::async_runtime::block_on(gf.ensure(&[font()]));
    assert!(!out.offline && out.missing == vec![font()]);
    let left: Vec<_> = std::fs::read_dir(&dir.0).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn a_garbage_file_under_the_cache_name_is_replaced_by_the_fetched_font() {
    let dir = Scratch::new("fonts-heal");
    let ttf = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/text/fonts/JosefinSans-Light.ttf")).unwrap();
    std::fs::create_dir_all(&dir.0).unwrap();
    std::fs::write(dir.0.join(cache_name(&font())), b"half a fo").unwrap();
    let served = ttf.clone();
    let (base, seen) = serve(move |base| {
        let css = format!("@font-face {{ src: url({base}/s/josefin.ttf) format('truetype'); }}");
        vec![(200, "text/css", css.into_bytes()), (200, "font/ttf", served)]
    });
    let gf = GoogleFonts::new(dir.0.clone(), &base).unwrap();
    let out = tauri::async_runtime::block_on(gf.ensure(&[font()]));
    assert!(!out.offline && out.missing.is_empty(), "{:?}", out.missing);
    assert_eq!(std::fs::read(dir.0.join(cache_name(&font()))).unwrap(), ttf);
    assert_eq!(seen.lock().unwrap().len(), 2);
    let names: Vec<_> = std::fs::read_dir(&dir.0).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names.len(), 1, "{names:?}");
}
