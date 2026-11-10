//! Suivi de l'evolution d'un dossier entre deux analyses.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::config::write_atomic;
use crate::model::Scan;
use crate::util::path_key;

const MAX_PER_ROOT: usize = 30;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Snapshot {
    pub timestamp: i64,
    pub size: u64,
    pub files: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Snapshots {
    roots: BTreeMap<String, Vec<Snapshot>>,
}

impl Snapshots {
    pub fn load(file: &Path) -> Self {
        fs::read_to_string(file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self, file: &Path) -> Result<()> {
        write_atomic(file, serde_json::to_string(self)?.as_bytes())
    }

    fn key(scan: &Scan) -> String {
        scan.roots.iter().map(|r| path_key(r)).collect::<Vec<_>>().join("|")
    }

    /// Enregistre l'analyse et retourne l'historique du dossier.
    pub fn record(&mut self, scan: &Scan) -> &[Snapshot] {
        let list = self.roots.entry(Self::key(scan)).or_default();
        list.push(Snapshot { timestamp: scan.scanned_at, size: scan.total_size(), files: scan.total_files() });
        if list.len() > MAX_PER_ROOT {
            list.drain(..list.len() - MAX_PER_ROOT);
        }
        list
    }

    pub fn history(&self, scan: &Scan) -> &[Snapshot] {
        self.roots.get(&Self::key(scan)).map_or(&[], Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn history_is_capped_and_keeps_latest() {
        let mut snapshots = Snapshots::default();
        let mut scan = Scan { roots: vec![PathBuf::from(r"C:\Data")], ..Scan::default() };
        for i in 0..40 {
            scan.scanned_at = i;
            snapshots.record(&scan);
        }
        let list = snapshots.history(&scan);
        assert_eq!(list.len(), MAX_PER_ROOT);
        assert_eq!(list.last().unwrap().timestamp, 39);
    }
}
