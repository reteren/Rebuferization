//! Persistence: SQLite metadata, content-addressed blobs, retention janitor.
//!
//! OWNER: worker W1. Everything under `store/` belongs to this module; nothing
//! else in the crate opens the database or touches `blobs/` directly.

pub mod blobs;
pub mod db;
pub mod janitor;
pub mod queries;

use std::path::{Path, PathBuf};

use crate::capture::Capture;
use crate::error::AppResult;
use crate::model::{CleanupResult, Facet, Filter, ItemDto, Sort, StorageStats};

/// The whole persistence layer. Cheap to clone (shares one pooled connection
/// behind a mutex), so it lives in Tauri managed state.
pub struct Store {
    #[allow(dead_code)]
    root: PathBuf,
}

impl Store {
    /// Opens (creating if needed) the store at `root`, applies migrations, and
    /// runs the startup integrity sweep.
    pub fn open(_root: &Path) -> AppResult<Store> {
        todo!("W1: db::open + migrations + integrity sweep")
    }

    /// Root of the store folder — `%APPDATA%\Rebuffer` by default.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Persists a capture, or bumps the existing row when the hash already
    /// exists. Returns the row as the UI will see it.
    pub fn insert_capture(&self, _cap: Capture) -> AppResult<ItemDto> {
        todo!("W1")
    }

    pub fn list(
        &self,
        _filter: &Filter,
        _sort: Sort,
        _offset: u32,
        _limit: u32,
    ) -> AppResult<Vec<ItemDto>> {
        todo!("W1")
    }

    pub fn search(&self, _query: &str, _filter: &Filter, _limit: u32) -> AppResult<Vec<ItemDto>> {
        todo!("W1")
    }

    pub fn get(&self, _id: i64) -> AppResult<ItemDto> {
        todo!("W1")
    }

    /// Absolute path of an item's blob, or of the referenced file.
    pub fn blob_path(&self, _id: i64) -> AppResult<PathBuf> {
        todo!("W1")
    }

    /// Every stored format for an item, in paste-restore order.
    pub fn formats(&self, _id: i64) -> AppResult<Vec<(String, Vec<u8>)>> {
        todo!("W1")
    }

    pub fn set_pinned(&self, _ids: &[i64], _pinned: bool) -> AppResult<()> {
        todo!("W1")
    }

    pub fn rename(&self, _id: i64, _title: &str) -> AppResult<()> {
        todo!("W1")
    }

    pub fn delete(&self, _ids: &[i64]) -> AppResult<()> {
        todo!("W1")
    }

    pub fn add_references(&self, _paths: &[String]) -> AppResult<Vec<ItemDto>> {
        todo!("W1")
    }

    pub fn touch_used(&self, _id: i64) -> AppResult<()> {
        todo!("W1")
    }

    pub fn ext_facets(&self, _filter: &Filter) -> AppResult<Vec<Facet>> {
        todo!("W1")
    }

    pub fn stats(&self) -> AppResult<StorageStats> {
        todo!("W1")
    }

    /// Age sweep plus, if configured, the size cap. Safe to call repeatedly.
    pub fn run_cleanup(&self, _older_than_days: Option<u32>) -> AppResult<CleanupResult> {
        todo!("W1")
    }
}
