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

/// A mock that answers every connection by its path, each on a thread of its own, for as long as the test runs:
/// a css2 request gets a stylesheet naming `/s/<family>.ttf` on the mock, a `.ttf` the fixture font after `delay`
/// (so two fetches of one font are in flight together), anything else a 404. Returns the base and the request count.
fn serve_by_path(ttf: Vec<u8>, delay: std::time::Duration) -> (String, Arc<Mutex<usize>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let count: Arc<Mutex<usize>> = Arc::default();
    let (seen, own) = (count.clone(), base.clone());
    std::thread::spawn(move || {
        for s in listener.incoming() {
            let Ok(mut s) = s else { return };
            let (ttf, base, seen) = (ttf.clone(), own.clone(), seen.clone());
            std::thread::spawn(move || {
                let mut buf = vec![0u8; 8192];
                let n = s.read(&mut buf).unwrap_or(0);
                *seen.lock().unwrap() += 1;
                let line = String::from_utf8_lossy(&buf[..n]).lines().next().unwrap_or("").to_string();
                let path = line.split(' ').nth(1).unwrap_or("").to_string();
                let (status, ctype, body) = if let Some(family) = path.strip_prefix("/css2?family=") {
                    let family = family.split(':').next().unwrap_or("").replace('+', "");
                    (200, "text/css", format!("@font-face {{ src: url({base}/s/{family}.ttf) format('truetype'); }}").into_bytes())
                } else if path.ends_with(".ttf") {
                    std::thread::sleep(delay);
                    (200, "font/ttf", ttf)
                } else {
                    (404, "text/plain", b"no".to_vec())
                };
                let _ = write!(s, "HTTP/1.1 {status} OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = s.write_all(&body);
            });
        }
    });
    (base, count)
}

#[test]
fn a_part_file_left_by_an_earlier_run_is_swept_and_the_cached_fonts_stay() {
    let dir = Scratch::new("fonts-sweep");
    std::fs::create_dir_all(&dir.0).unwrap();
    let ttf = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/text/fonts/JosefinSans-Light.ttf")).unwrap();
    std::fs::write(dir.0.join(cache_name(&font())), &ttf).unwrap();
    std::fs::write(dir.0.join(format!("{}.4242.7.part", cache_name(&font()))), b"half a fo").unwrap();
    std::fs::write(dir.0.join("Other-400.ttf.part"), b"older name").unwrap();
    let _gf = GoogleFonts::new(dir.0.clone(), "http://127.0.0.1:9").unwrap();
    let mut names: Vec<String> = std::fs::read_dir(&dir.0).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    names.sort();
    assert_eq!(names, [cache_name(&font())]);
}

#[test]
fn two_plans_fetching_one_font_at_once_leave_one_valid_file_and_no_part() {
    let dir = Scratch::new("fonts-race");
    let ttf = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/text/fonts/JosefinSans-Light.ttf")).unwrap();
    let (base, count) = serve_by_path(ttf.clone(), std::time::Duration::from_millis(150));
    let gf = GoogleFonts::new(dir.0.clone(), &base).unwrap();
    let (a, b) = tauri::async_runtime::block_on(async { futures_util::future::join(gf.ensure(&[font()]), gf.ensure(&[font()])).await });
    assert!(!a.offline && a.missing.is_empty() && !b.offline && b.missing.is_empty(), "{a:?} {b:?}");
    assert_eq!(*count.lock().unwrap(), 4, "both fetched: the race is real");
    let names: Vec<String> = std::fs::read_dir(&dir.0).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    assert_eq!(names, [cache_name(&font())]);
    assert_eq!(std::fs::read(dir.0.join(cache_name(&font()))).unwrap(), ttf);
}

#[test]
fn several_fonts_are_fetched_together_and_the_missing_ones_come_back_in_the_order_asked() {
    let dir = Scratch::new("fonts-many");
    let ttf = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/text/fonts/JosefinSans-Light.ttf")).unwrap();
    let (base, _) = serve_by_path(ttf, std::time::Duration::from_millis(20));
    let gf = GoogleFonts::new(dir.0.clone(), &base).unwrap();
    let fonts: Vec<FontRef> = (0..8).map(|i| FontRef { family: format!("Fam{i}"), weight: 400, italic: false, source: Source::Google }).collect();
    let out = tauri::async_runtime::block_on(gf.ensure(&fonts));
    assert!(!out.offline && out.missing.is_empty(), "{out:?}");
    for f in &fonts {
        assert!(dir.0.join(cache_name(f)).exists());
    }
    // fonts the server cannot serve are missing, in the order they were asked for, the cached one is not asked
    let (base, _) = serve(|_| vec![(404, "text/plain", b"no".to_vec()), (404, "text/plain", b"no".to_vec())]);
    let gf = GoogleFonts::new(dir.0.clone(), &base).unwrap();
    let missing: Vec<FontRef> = ["Zed", "Ay"].iter().map(|n| FontRef { family: n.to_string(), weight: 400, italic: false, source: Source::Google }).collect();
    let out = tauri::async_runtime::block_on(gf.ensure(&[missing[0].clone(), fonts[0].clone(), missing[1].clone()]));
    assert!(!out.offline);
    assert_eq!(out.missing, missing);
}
