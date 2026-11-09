//! Cursor store for grep pagination.
//!
//! Keeps opaque cursor IDs and offsets across workers when a session file is set.
//! Cursors are evicted LRU-style when the store exceeds capacity.

use std::collections::{HashMap, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::{self, Seek, Write};

const MAX_CURSORS: usize = 20;

/// Stores cursor state for paginated grep results.
#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct CursorStore {
    counter: u64,
    /// Map from cursor ID string → file offset for next page.
    cursors: HashMap<String, usize>,
    /// Insertion order for LRU eviction.
    insertion_order: VecDeque<String>,
    #[serde(skip)]
    session: Option<File>,
    #[serde(skip)]
    dirty: bool,
}

impl CursorStore {
    pub fn new() -> io::Result<(Self, bool)> {
        let Some(path) = std::env::var_os("FFF_MCP_CURSOR_STATE") else {
            return Ok((Self::default(), false));
        };
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let resuming = file.metadata()?.len() > 0;
        let mut store = if resuming {
            serde_json::from_reader(&file).map_err(io::Error::other)?
        } else {
            Self::default()
        };
        store.session = Some(file);
        if !resuming {
            store.dirty = true;
            store.save_session()?;
        }
        Ok((store, resuming))
    }

    pub fn save_session(&mut self) -> io::Result<()> {
        if !self.dirty {
            return Ok(());
        }
        if let Some(mut file) = self.session.as_ref() {
            let bytes = serde_json::to_vec(self).map_err(io::Error::other)?;
            file.rewind()?;
            file.write_all(&bytes)?;
            file.set_len(bytes.len() as u64)?;
        }
        self.dirty = false;
        Ok(())
    }

    /// Store a cursor and return its opaque ID string.
    pub fn store(&mut self, file_offset: usize) -> String {
        self.counter = self.counter.wrapping_add(1);
        let id = self.counter.to_string();

        self.cursors.insert(id.clone(), file_offset);
        self.insertion_order.push_back(id.clone());

        // Evict oldest cursors
        while self.cursors.len() > MAX_CURSORS {
            if let Some(oldest) = self.insertion_order.pop_front() {
                self.cursors.remove(&oldest);
            } else {
                break;
            }
        }

        self.dirty = true;
        id
    }

    /// Retrieve the file offset for a cursor ID.
    pub fn get(&self, id: &str) -> Option<usize> {
        self.cursors.get(id).copied()
    }
}
