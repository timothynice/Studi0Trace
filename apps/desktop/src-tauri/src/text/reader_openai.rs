//! The AI reader: one Responses-API call with the whole image (at most MAX_SIDE a side) and a crop per line,
//! asking for each line's exact string, its lettering class and up to three typefaces, as strict JSON. It never
//! supplies boxes. The key goes in the header, as redraw's does, and OpenAI's messages are never kept.
use super::reader::{FontHint, LineBox, ReadFuture, Reader, Reading};
use super::Class;
use crate::error::CommandError;
use crate::redraw::error;
use crate::redraw::openai::Api;
use crate::redraw::Source;
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use studi0trace_core::resample::{resize_rgba, Filter};

/// The whole image is sent at most this large.
pub const MAX_SIDE: u32 = 1024;

pub const PROMPT: &str = "The first image is a design that contains lines of text; each following image is a crop of one line, \
numbered in order. Indices in your answer are zero-based: the first crop is index 0. For every crop give: `string`, \
the exact text as set (correct recognition errors; keep case and punctuation); `class`, \"type\" when the line is \
set in a typeface, \"script\" for handwritten, brush or connected script lettering, \"custom\" for a hand-drawn or \
heavily customised wordmark; and `fonts`, up to three typefaces the line is most likely set in, best first, as \
Google Fonts family names or common system font names, each with its weight (100–900) and whether it is italic. \
Answer only in the schema.";

pub struct AiReader {
    api: Api,
    model: String,
}

impl AiReader {
    pub fn new(base: &str, key: &str, model: &str) -> Result<AiReader, CommandError> {
        Ok(AiReader { api: Api::new(base, key)?, model: model.to_string() })
    }
}

fn png_data_url(rgba: &[u8], w: u32, h: u32) -> Result<String, CommandError> {
    let png = crate::redraw::encode_png(rgba, w, h, image::ExtendedColorType::Rgba8)?;
    Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)))
}

/// The line's bounding box, grown by a fifth of its height, cut out of the source (always at least one pixel).
fn crop(source: &Source, quad: [[f32; 2]; 4]) -> (Vec<u8>, u32, u32) {
    let xs = quad.iter().map(|p| p[0]);
    let ys = quad.iter().map(|p| p[1]);
    let (x0, x1) = (xs.clone().fold(f32::MAX, f32::min), xs.fold(f32::MIN, f32::max));
    let (y0, y1) = (ys.clone().fold(f32::MAX, f32::min), ys.fold(f32::MIN, f32::max));
    let grow = (y1 - y0) * 0.2;
    let l = ((x0 - grow).floor().max(0.0) as u32).min(source.width - 1);
    let t = ((y0 - grow).floor().max(0.0) as u32).min(source.height - 1);
    let r = ((x1 + grow).ceil() as u32).min(source.width).max(l + 1);
    let b = ((y1 + grow).ceil() as u32).min(source.height).max(t + 1);
    let (w, h) = (r - l, b - t);
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in t..b {
        let row = &source.rgba[((y * source.width + l) * 4) as usize..((y * source.width + r) * 4) as usize];
        out.extend_from_slice(row);
    }
    (out, w, h)
}

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "lines": { "type": "array", "items": {
                "type": "object",
                "properties": {
                    "index": { "type": "integer" },
                    "string": { "type": "string" },
                    "class": { "type": "string", "enum": ["type", "script", "custom"] },
                    "fonts": { "type": "array", "items": {
                        "type": "object",
                        "properties": { "family": { "type": "string" }, "weight": { "type": "integer" }, "italic": { "type": "boolean" } },
                        "required": ["family", "weight", "italic"], "additionalProperties": false } }
                },
                "required": ["index", "string", "class", "fonts"], "additionalProperties": false } }
        },
        "required": ["lines"], "additionalProperties": false
    })
}

/// The request body: model, the prompt with the lines Vision read, the whole image and one crop per line as data URLs, and the strict JSON schema.
pub fn request_body(model: &str, source: &Source, lines: &[LineBox]) -> Result<Value, CommandError> {
    let side = source.width.max(source.height);
    let whole = if side > MAX_SIDE {
        let s = MAX_SIDE as f32 / side as f32;
        let (w, h) = ((source.width as f32 * s).round().max(1.0) as u32, (source.height as f32 * s).round().max(1.0) as u32);
        let rgba = resize_rgba(&source.rgba, source.width, source.height, w, h, Filter::Lanczos).map_err(|_| error("bad_reply"))?;
        png_data_url(&rgba, w, h)?
    } else {
        png_data_url(&source.rgba, source.width, source.height)?
    };
    let mut prompt = String::from(PROMPT);
    prompt.push_str("\nRecognised so far:");
    for (i, l) in lines.iter().enumerate() {
        prompt.push_str(&format!("\n{}: {:?}", i + 1, l.string));
    }
    let mut content = vec![json!({ "type": "input_text", "text": prompt }), json!({ "type": "input_image", "image_url": whole })];
    for l in lines {
        let (rgba, w, h) = crop(source, l.quad);
        content.push(json!({ "type": "input_image", "image_url": png_data_url(&rgba, w, h)? }));
    }
    Ok(json!({
        "model": model,
        "input": [{ "role": "user", "content": content }],
        "text": { "format": { "type": "json_schema", "name": "text_lines", "strict": true, "schema": schema() } }
    }))
}

#[derive(Deserialize)]
struct Answer {
    lines: Vec<AnsweredLine>,
}

#[derive(Deserialize)]
struct AnsweredLine {
    index: usize,
    string: String,
    class: String,
    #[serde(default)]
    fonts: Vec<FontHint>,
}

/// The readings out of a Responses reply: the first `output_text` parsed as the schema; a line the reply does not name keeps its Vision reading.
pub fn parse_readings(reply: &Value, lines: &[LineBox]) -> Result<Vec<Reading>, CommandError> {
    let text = reply["output"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|o| o["type"] == "message")
        .flat_map(|o| o["content"].as_array().cloned().unwrap_or_default())
        .find(|c| c["type"] == "output_text")
        .and_then(|c| c["text"].as_str().map(str::to_string))
        .ok_or_else(|| error("bad_reply"))?;
    let answer: Answer = serde_json::from_str(&text).map_err(|_| error("bad_reply"))?;
    let mut out = super::reader::vision_readings(lines);
    for a in answer.lines {
        let Some(slot) = out.get_mut(a.index) else { continue };
        if !a.string.trim().is_empty() {
            slot.string = a.string.trim().to_string();
        }
        slot.class = match a.class.as_str() {
            "script" => Class::Script,
            "custom" => Class::Custom,
            _ => Class::Type,
        };
        slot.hints = a
            .fonts
            .into_iter()
            .take(3)
            .map(|h| FontHint { family: h.family.trim().to_string(), weight: ((h.weight.clamp(100, 900) + 50) / 100 * 100).clamp(100, 900), italic: h.italic })
            .collect();
    }
    Ok(out)
}

impl Reader for AiReader {
    fn read<'a>(&'a self, source: &'a Source, lines: &'a [LineBox]) -> ReadFuture<'a> {
        Box::pin(async move {
            let body = request_body(&self.model, source, lines)?;
            let reply = self.api.post_json("/responses", &body).await?;
            parse_readings(&reply, lines)
        })
    }
}
