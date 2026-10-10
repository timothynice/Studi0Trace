//! What the app keeps per image: the plan with what it needs to be edited and used (by the hash of the image's
//! bytes), and the plan that is running (by generation).
use super::plan::{paint_mask, Planned};
use crate::error::CommandError;
use crate::redraw::Source as Image;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

pub fn bytes_hash(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).into()
}

/// The sorted ids of the lines painted out, joined by ",": the empty string is the untouched image.
pub type MaskKey = String;

/// The painted images kept per image: past it the least recently asked for goes, never the full mask's.
pub const PAINTED_MAX: usize = 16;

pub struct Entry {
    pub planned: Arc<Planned>,
    pub hash: [u8; 32],
    /// The painted PNG of the masks asked for, least recently asked for first, at most `PAINTED_MAX`. What it holds
    /// depends on the inks and the source alone, so an edit of a string or a font (`put_if`) keeps it.
    painted: Vec<(MaskKey, Arc<Vec<u8>>)>,
}

impl Entry {
    fn new(planned: Arc<Planned>, hash: [u8; 32]) -> Entry {
        Entry { planned, hash, painted: Vec::new() }
    }

    /// The painted image of `key`, now the most recently asked for.
    fn painted(&mut self, key: &str) -> Option<Arc<Vec<u8>>> {
        let at = self.painted.iter().position(|(k, _)| k == key)?;
        let hit = self.painted.remove(at);
        let png = hit.1.clone();
        self.painted.push(hit);
        Some(png)
    }

    /// Keeps `png` as `key`'s, the most recently asked for, and drops the least recently asked for past `PAINTED_MAX`
    /// (never the full mask: every matched line, the first trace's and the one a switch comes back to).
    fn keep(&mut self, key: MaskKey, png: Arc<Vec<u8>>) {
        self.painted.retain(|(k, _)| *k != key);
        self.painted.push((key, png));
        if self.painted.len() <= PAINTED_MAX {
            return;
        }
        let mut full: Vec<&str> = self.planned.plan.lines.iter().filter(|l| l.status == super::Status::Matched).map(|l| l.id.as_str()).collect();
        full.sort_unstable();
        let full = full.join(",");
        if let Some(at) = self.painted.iter().position(|(k, _)| *k != full) {
            self.painted.remove(at);
        }
    }
}

type Abort = Box<dyn Fn() + Send + Sync>;

struct Running {
    generation: u64,
    abort: Option<Abort>,
}

#[derive(Default)]
struct State {
    entries: HashMap<String, Entry>,
    running: HashMap<String, Running>,
}

/// Per image: the plan, and the plan being made. One lock holds both, so a result is stored or refused against
/// the generation in the same breath; generations come from one counter, so none is ever handed out twice.
#[derive(Default)]
pub struct Plans {
    state: Mutex<State>,
    next: AtomicU64,
}

impl Plans {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The plan, only while the image's bytes still hash to `hash`.
    pub fn get(&self, image: &str, hash: &[u8; 32]) -> Option<Arc<Planned>> {
        self.lock().entries.get(image).filter(|e| &e.hash == hash).map(|e| e.planned.clone())
    }

    /// Stores a plan that has no generation (an edit of the one kept).
    pub fn put(&self, image: &str, hash: [u8; 32], planned: Planned) -> Arc<Planned> {
        let p = Arc::new(planned);
        self.lock().entries.insert(image.to_string(), Entry::new(p.clone(), hash));
        p
    }

    /// Stores an edit of the kept plan, only while the image's entry is still there for `expected_hash`: None (and
    /// nothing stored) when the image was closed, or its bytes swapped by a redraw or a revert, while the edit ran.
    pub fn put_if(&self, image: &str, expected_hash: [u8; 32], planned: Planned) -> Option<Arc<Planned>> {
        let mut state = self.lock();
        let entry = state.entries.get_mut(image).filter(|e| e.hash == expected_hash)?;
        let p = Arc::new(planned);
        entry.planned = p.clone();
        Some(p)
    }

    /// Stores the plan `generation` made, and ends it, when it is still the image's current one; None (and
    /// nothing stored) when a cancel, a newer request or the image's closing came first.
    pub fn put_if_current(&self, image: &str, generation: u64, hash: [u8; 32], planned: Planned) -> Option<Arc<Planned>> {
        let mut state = self.lock();
        if !state.running.get(image).is_some_and(|r| r.generation == generation) {
            return None;
        }
        state.running.remove(image);
        let p = Arc::new(planned);
        state.entries.insert(image.to_string(), Entry::new(p.clone(), hash));
        Some(p)
    }

    /// `generation` ended without a plan. True when it was still the current one (a real failure); false when a
    /// cancel or a newer request had already ended it.
    pub fn fail(&self, image: &str, generation: u64) -> bool {
        let mut state = self.lock();
        if state.running.get(image).is_some_and(|r| r.generation == generation) {
            state.running.remove(image);
            return true;
        }
        false
    }

    /// The plan is dropped and a running one is aborted.
    pub fn forget(&self, image: &str) {
        let running = {
            let mut state = self.lock();
            state.entries.remove(image);
            state.running.remove(image)
        };
        if let Some(a) = running.and_then(|r| r.abort) {
            a();
        }
    }

    /// The image with the lines of `mask` painted out, as PNG, for the trace of that mask: built from the kept plan and
    /// `source` (the image's bytes decoded by the caller, who has checked they hash to `hash`) on a miss and cached
    /// per mask (at most `PAINTED_MAX`, the least recently asked for dropped first). Ids the plan does not hold are
    /// ignored; None for a mask that names no line (the trace takes the original), and when no plan is kept for these
    /// bytes. The painting runs outside the lock: two builders of one mask may race, and the last to finish is kept
    /// (both are the same bytes).
    pub fn painted_for(&self, image: &str, hash: &[u8; 32], mask: &[String], source: &Image) -> Result<Option<Arc<Vec<u8>>>, CommandError> {
        if mask.is_empty() {
            return Ok(None);
        }
        let mut ids: Vec<String> = mask.to_vec();
        ids.sort();
        ids.dedup();
        let (planned, key) = {
            let mut state = self.lock();
            let Some(e) = state.entries.get_mut(image).filter(|e| &e.hash == hash) else { return Ok(None) };
            let known: Vec<String> = ids.into_iter().filter(|id| e.planned.plan.lines.iter().any(|l| &l.id == id)).collect();
            if known.is_empty() {
                return Ok(None);
            }
            let key: MaskKey = known.join(",");
            if let Some(p) = e.painted(&key) {
                return Ok(Some(p));
            }
            (e.planned.clone(), (key, known))
        };
        let (key, known) = key;
        let Some(png) = paint_mask(source, &planned, &known)? else { return Ok(None) };
        let png = Arc::new(png);
        // kept only while the entry is still the one the bytes were painted for
        if let Some(e) = self.lock().entries.get_mut(image).filter(|e| &e.hash == hash) {
            e.keep(key, png.clone());
        }
        Ok(Some(png))
    }

    /// The masks whose painted image is kept for `image`, least recently asked for first.
    #[cfg(test)]
    fn painted_masks(&self, image: &str) -> Vec<MaskKey> {
        self.lock().entries.get(image).map(|e| e.painted.iter().map(|(k, _)| k.clone()).collect()).unwrap_or_default()
    }

    /// A new plan of `image`: the one running is aborted. Answers this plan's generation.
    pub fn reserve(&self, image: &str) -> u64 {
        let generation = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let old = self.lock().running.insert(image.to_string(), Running { generation, abort: None });
        if let Some(a) = old.and_then(|o| o.abort) {
            a();
        }
        generation
    }

    pub fn is_current(&self, image: &str, generation: u64) -> bool {
        self.lock().running.get(image).is_some_and(|r| r.generation == generation)
    }

    /// How to abort `generation`'s task, kept while it is the current one; called at once when it no longer is
    /// (a cancel that came first), so a late arm never replaces a newer task's.
    pub fn arm(&self, image: &str, generation: u64, abort: Abort) {
        let stale = {
            let mut state = self.lock();
            match state.running.get_mut(image).filter(|r| r.generation == generation) {
                Some(r) => {
                    r.abort = Some(abort);
                    None
                }
                None => Some(abort),
            }
        };
        if let Some(a) = stale {
            a();
        }
    }

    /// Aborts the running plan, if any; false when none is (it finished, or was cancelled already).
    pub fn cancel(&self, image: &str) -> bool {
        let running = self.lock().running.remove(image);
        match running {
            Some(r) => {
                if let Some(a) = r.abort {
                    a();
                }
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{ink::LineInk, Class, ReaderKind, Status, TextLine, TextPlan};

    /// An image of `n` one-pixel "lines", one per column of a 1-high strip, each matched.
    fn planned(n: u32) -> (Planned, Image) {
        let lines = (0..n).map(|i| TextLine { id: format!("i-t{i:02}"), quad: [[i as f32, 0.0], [i as f32 + 1.0, 0.0], [i as f32 + 1.0, 1.0], [i as f32, 1.0]], string: "l".into(), class: Class::Type, colour: [0; 3], status: Status::Matched, candidates: Vec::new(), chosen: None, enabled: true, fit: None, occluders: Vec::new() }).collect();
        let inks = (0..n).map(|i| Arc::new(LineInk { left: i, top: 0, width: 1, height: 1, coverage: vec![1.0], mask: vec![true], backdrop: [255; 3], colour: [0; 3], spread: 0.0, segments: vec![(0, 1)], ink_bounds: Some((0, 0, 1, 1)), occluders: Vec::new(), foreign: Vec::new() })).collect();
        let rgba = (0..n).flat_map(|_| [0u8, 0, 0, 255]).collect();
        (Planned { plan: TextPlan { image: "i".into(), reader: ReaderKind::Vision, lines, ai_error: None, fonts_offline: false }, inks }, Image { rgba, width: n, height: 1 })
    }

    #[test]
    fn at_most_painted_max_masks_are_kept_the_least_recently_asked_for_going_first_and_never_the_full_mask() {
        let n = PAINTED_MAX as u32 + 4;
        let (p, source) = planned(n);
        let ids: Vec<String> = p.plan.lines.iter().map(|l| l.id.clone()).collect();
        let full = ids.join(",");
        let (plans, hash) = (Plans::default(), bytes_hash(b"strip"));
        plans.put("i", hash, p);
        // the full mask first, then one mask per line
        assert!(plans.painted_for("i", &hash, &ids, &source).unwrap().is_some());
        let first = plans.painted_for("i", &hash, &ids[..1], &source).unwrap().unwrap();
        for i in 1..PAINTED_MAX - 1 {
            plans.painted_for("i", &hash, &ids[i..=i], &source).unwrap();
        }
        assert_eq!(plans.painted_masks("i").len(), PAINTED_MAX);
        // the first line's mask asked for again: the most recent now, and the same bytes
        assert!(Arc::ptr_eq(&first, &plans.painted_for("i", &hash, &ids[..1], &source).unwrap().unwrap()));
        // past the cap: the least recently asked for go, the full mask (the oldest) stays
        for i in PAINTED_MAX - 1..PAINTED_MAX + 2 {
            plans.painted_for("i", &hash, &ids[i..=i], &source).unwrap();
        }
        let kept = plans.painted_masks("i");
        assert_eq!(kept.len(), PAINTED_MAX);
        assert_eq!(kept[0], full);
        assert!(kept.contains(&ids[0]), "asked for again, so kept: {kept:?}");
        assert!(!kept.contains(&ids[1]) && !kept.contains(&ids[2]) && !kept.contains(&ids[3]), "{kept:?}");
        assert_eq!(kept.last(), Some(&ids[PAINTED_MAX + 1]));
    }
}
