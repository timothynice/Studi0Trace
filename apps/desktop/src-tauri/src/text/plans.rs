//! What the app keeps per image: the plan with what it needs to be edited and used (by the hash of the image's
//! bytes), and the plan that is running (by generation).
use super::plan::Planned;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

pub fn bytes_hash(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).into()
}

pub struct Entry {
    pub planned: Arc<Planned>,
    pub hash: [u8; 32],
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
        self.lock().entries.insert(image.to_string(), Entry { planned: p.clone(), hash });
        p
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
        state.entries.insert(image.to_string(), Entry { planned: p.clone(), hash });
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

    /// What a trace should use: the painted PNG and the group, when the plan has a composed line.
    pub fn for_trace(&self, image: &str, hash: &[u8; 32]) -> Option<(Arc<Vec<u8>>, String)> {
        let state = self.lock();
        let e = state.entries.get(image).filter(|e| &e.hash == hash)?;
        e.planned.painted.as_ref().map(|p| (p.clone(), e.planned.group.clone()))
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
