//! The AI reader against a mock server: the request is a JSON POST to /v1/responses with the key only in the
//! header, the image and one crop per line as data URLs, and a strict schema; the reply's output_text is parsed
//! into readings; errors map to redraw's codes; an unnamed line keeps its Vision reading.
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use studi0trace_desktop::redraw::Source;
use studi0trace_desktop::text::reader::{LineBox, Reader};
use studi0trace_desktop::text::reader_openai::{parse_readings, AiReader};
use studi0trace_desktop::text::Class;

struct Seen {
    head: String,
    body: Vec<u8>,
}

fn read_request(s: &mut std::net::TcpStream) -> Seen {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 65536];
    loop {
        let n = s.read(&mut chunk).unwrap_or(0);
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..at]).to_string();
            let len: usize = head.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap())).unwrap_or(0);
            while buf.len() < at + 4 + len {
                let n = s.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            return Seen { head, body: buf[at + 4..].to_vec() };
        }
    }
    Seen { head: String::new(), body: buf }
}

fn serve(replies: Vec<(u16, Value)>) -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
    let log = seen.clone();
    std::thread::spawn(move || {
        for (status, body) in replies {
            let Ok((mut s, _)) = listener.accept() else { return };
            log.lock().unwrap().push(read_request(&mut s));
            let body = body.to_string();
            let _ = write!(s, "HTTP/1.1 {status} Mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = s.write_all(body.as_bytes());
        }
    });
    (base, seen)
}

fn source() -> Source {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/text/montserrat-est-30-t0-white.png");
    Source::decode(&std::fs::read(p).unwrap()).unwrap()
}

fn lines() -> Vec<LineBox> {
    vec![
        LineBox { quad: [[25.0, 15.0], [200.0, 15.0], [200.0, 55.0], [25.0, 55.0]], string: "EST. 2O26".into(), confidence: 0.9 },
        LineBox { quad: [[25.0, 15.0], [120.0, 15.0], [120.0, 55.0], [25.0, 55.0]], string: "second".into(), confidence: 0.9 },
    ]
}

fn reply(text: &str) -> Value {
    json!({ "id": "resp_1", "output": [{ "type": "message", "role": "assistant", "content": [{ "type": "output_text", "text": text }] }] })
}

fn run<T>(f: impl std::future::Future<Output = T>) -> T {
    tauri::async_runtime::block_on(f)
}

#[test]
fn sends_the_image_the_crops_and_the_schema_with_the_key_only_in_the_header() {
    let answer = json!({ "lines": [{ "index": 0, "string": "EST. 2026", "class": "type", "fonts": [{ "family": "Montserrat", "weight": 400, "italic": false }] }] });
    let (base, seen) = serve(vec![(200, reply(&answer.to_string()))]);
    let reader = AiReader::new(&base, "sk-test-key", "gpt-5-mini").unwrap();
    let out = run(reader.read(&source(), &lines())).unwrap();
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].string, "EST. 2026");
    assert_eq!(out[0].hints[0].family, "Montserrat");
    assert_eq!(out[0].class, Class::Type);
    assert_eq!(out[1].string, "second", "an unnamed line keeps Vision's reading");
    let seen = seen.lock().unwrap();
    assert!(seen[0].head.starts_with("POST /v1/responses HTTP/1.1"), "{}", seen[0].head);
    assert!(seen[0].head.to_lowercase().contains("authorization: bearer sk-test-key"));
    let body: Value = serde_json::from_slice(&seen[0].body).unwrap();
    assert_eq!(body["model"], "gpt-5-mini");
    assert!(!seen[0].body.windows(11).any(|w| w == b"sk-test-key"), "the key is never in the body");
    let content = body["input"][0]["content"].as_array().unwrap();
    let images: Vec<&Value> = content.iter().filter(|c| c["type"] == "input_image").collect();
    assert_eq!(images.len(), 3, "the whole image and one crop per line");
    assert!(images[0]["image_url"].as_str().unwrap().starts_with("data:image/png;base64,"));
    assert_eq!(body["text"]["format"]["type"], "json_schema");
    assert_eq!(body["text"]["format"]["strict"], true);
    assert!(content[0]["text"].as_str().unwrap().contains("EST. 2O26"), "the prompt carries what Vision read");
}

#[test]
fn errors_map_to_redraws_codes_and_a_bad_reply_is_bad_reply() {
    for (status, body, code) in [
        (401, json!({"error": {"code": "invalid_api_key"}}), "invalid_key"),
        (429, json!({"error": {"code": "rate_limit_exceeded"}}), "quota"),
        (403, json!({"error": {}}), "not_allowed"),
        (400, json!({"error": {"code": "model_not_found"}}), "bad_reply"),
        (200, json!({"output": []}), "bad_reply"),
        (200, reply("not json"), "bad_reply"),
    ] {
        let (base, _) = serve(vec![(status, body)]);
        let reader = AiReader::new(&base, "k", "gpt-5-mini").unwrap();
        let err = run(reader.read(&source(), &lines())).unwrap_err();
        assert_eq!(err.code(), Some(code), "status {status}");
    }
    let reader = AiReader::new("http://127.0.0.1:9/v1", "k", "gpt-5-mini").unwrap();
    assert_eq!(run(reader.read(&source(), &lines())).unwrap_err().code(), Some("offline"));
}

#[test]
fn parse_readings_classes_and_clamps_weights() {
    let text = json!({ "lines": [{ "index": 1, "string": "Awaits", "class": "script", "fonts": [{ "family": "Pacifico", "weight": 950, "italic": true }] }] }).to_string();
    let out = parse_readings(&reply(&text), &lines()).unwrap();
    assert_eq!(out[1].class, Class::Script);
    assert_eq!(out[1].hints[0].weight, 900);
    assert!(out[1].hints[0].italic);
    assert_eq!(out[0].string, "EST. 2O26");
}
