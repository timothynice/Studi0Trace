//! What finds the lines and what reads them. Vision (vision.rs) does both on device; the AI reader
//! (reader_openai.rs) reads better and names fonts, and never supplies boxes. One `Reader` trait, so the
//! orchestration does not care which.
use super::Class;
use crate::error::CommandError;
use crate::redraw::Source;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;

#[derive(Debug, Clone, PartialEq)]
pub struct LineBox {
    /// Source pixels: top-left, top-right, bottom-right, bottom-left.
    pub quad: [[f32; 2]; 4],
    pub string: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FontHint {
    pub family: String,
    pub weight: u16,
    pub italic: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub string: String,
    pub hints: Vec<FontHint>,
    pub class: Class,
}

pub trait Finder: Send + Sync {
    fn find(&self, source: &Source) -> Result<Vec<LineBox>, CommandError>;
}

pub type ReadFuture<'a> = Pin<Box<dyn Future<Output = Result<Vec<Reading>, CommandError>> + Send + 'a>>;

pub trait Reader: Send + Sync {
    fn read<'a>(&'a self, source: &'a Source, lines: &'a [LineBox]) -> ReadFuture<'a>;
}

/// The lines' own strings: what Vision read, no hints, every line `Type`.
pub fn vision_readings(lines: &[LineBox]) -> Vec<Reading> {
    lines.iter().map(|l| Reading { string: l.string.clone(), hints: Vec::new(), class: Class::Type }).collect()
}
