use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use market_domain::{MarketSnapshot, MarketSource, Validate};

use crate::{
    ports::StoreCanonicalSnapshots,
    storage::{CanonicalSnapshotBatch, StorageError},
};

/// Repository abstraction for canonical market snapshots.
pub trait SnapshotRepository {
    /// Upserts one canonical snapshot. Returns `true` when storage changed.
    fn upsert_snapshot(&self, snapshot: &MarketSnapshot) -> Result<bool, StorageError>;

    /// Returns all stored snapshots.
    fn list_snapshots(&self) -> Result<Vec<MarketSnapshot>, StorageError>;

    /// Looks up one snapshot by stable idempotency key parts.
    fn find_snapshot(
        &self,
        source: MarketSource,
        market_id: &str,
        observed_at: DateTime<Utc>,
    ) -> Result<Option<MarketSnapshot>, StorageError>;
}

/// Filesystem-backed JSONL snapshot repository with idempotent upsert.
#[derive(Debug, Clone)]
pub struct FilesystemSnapshotRepository {
    path: PathBuf,
}

impl FilesystemSnapshotRepository {
    /// Creates a new filesystem snapshot repository.
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, StorageError> {
        let path = path.into();
        ensure_store_file(&path)?;
        Ok(Self { path })
    }

    /// Returns the underlying repository path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn write_all(&self, snapshots: &[MarketSnapshot]) -> Result<(), StorageError> {
        let temp_path = temp_store_path(&self.path)?;

        let result = (|| -> Result<(), StorageError> {
            let mut file = OpenOptions::new()
                .write(true)
                .truncate(true)
                .create(true)
                .open(&temp_path)?;

            for snapshot in snapshots {
                serde_json::to_writer(&mut file, snapshot)?;
                file.write_all(b"\n")?;
            }

            file.flush()?;
            file.sync_all()?;
            std::fs::rename(&temp_path, &self.path)?;
            Ok(())
        })();

        if result.is_err() {
            let _ = std::fs::remove_file(&temp_path);
        }

        result
    }
}

impl SnapshotRepository for FilesystemSnapshotRepository {
    fn upsert_snapshot(&self, snapshot: &MarketSnapshot) -> Result<bool, StorageError> {
        snapshot
            .validate()
            .map_err(|error| StorageError::invalid_data(error.to_string()))?;

        let mut snapshots = self.list_snapshots()?;
        let key = snapshot_key(snapshot.source, &snapshot.market_id, snapshot.observed_at);

        if let Some(existing) = snapshots.iter_mut().find(|stored| {
            snapshot_key(stored.source, &stored.market_id, stored.observed_at) == key
        }) {
            if existing == snapshot {
                return Ok(false);
            }

            *existing = snapshot.clone();
            self.write_all(&snapshots)?;
            return Ok(true);
        }

        snapshots.push(snapshot.clone());
        self.write_all(&snapshots)?;
        Ok(true)
    }

    fn list_snapshots(&self) -> Result<Vec<MarketSnapshot>, StorageError> {
        ensure_store_file(&self.path)?;

        let file = File::open(&self.path)?;
        let reader = BufReader::new(file);
        let mut snapshots = Vec::new();

        for (index, line) in reader.lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let snapshot: MarketSnapshot = serde_json::from_str(&line).map_err(|error| {
                StorageError::invalid_data(format!(
                    "failed to parse JSONL line {}: {error}",
                    index + 1
                ))
            })?;
            snapshot.validate().map_err(|error| {
                StorageError::invalid_data(format!(
                    "invalid snapshot at line {}: {error}",
                    index + 1
                ))
            })?;
            snapshots.push(snapshot);
        }

        Ok(snapshots)
    }

    fn find_snapshot(
        &self,
        source: MarketSource,
        market_id: &str,
        observed_at: DateTime<Utc>,
    ) -> Result<Option<MarketSnapshot>, StorageError> {
        let target = snapshot_key(source, market_id, observed_at);

        Ok(self.list_snapshots()?.into_iter().find(|snapshot| {
            snapshot_key(snapshot.source, &snapshot.market_id, snapshot.observed_at) == target
        }))
    }
}

impl StoreCanonicalSnapshots for FilesystemSnapshotRepository {
    type Error = StorageError;

    fn store_canonical_snapshots(&self, batch: &CanonicalSnapshotBatch) -> Result<(), Self::Error> {
        for snapshot in &batch.snapshots {
            self.upsert_snapshot(snapshot)?;
        }

        Ok(())
    }
}

fn snapshot_key(source: MarketSource, market_id: &str, observed_at: DateTime<Utc>) -> String {
    format!("{source:?}:{market_id}:{}", observed_at.to_rfc3339())
}

fn ensure_store_file(path: &Path) -> Result<(), StorageError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    OpenOptions::new().create(true).append(true).open(path)?;
    Ok(())
}

fn temp_store_path(path: &Path) -> Result<PathBuf, StorageError> {
    let file_name = path.file_name().ok_or_else(|| {
        StorageError::invalid_data("snapshot store path must include a file name")
    })?;
    let mut temp_name = file_name.to_os_string();
    temp_name.push(".tmp");
    Ok(path.with_file_name(temp_name))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use chrono::{TimeZone, Utc};
    use market_domain::{MarketSnapshot, MarketSource, MarketStatus};

    use super::{FilesystemSnapshotRepository, SnapshotRepository};

    #[test]
    fn writes_and_reads_snapshots() {
        let path = unique_path("snapshot-repository");
        let repo = FilesystemSnapshotRepository::new(&path).unwrap();
        let snapshot = sample_snapshot("market-1", 0.41, 0.43);

        assert!(repo.upsert_snapshot(&snapshot).unwrap());
        let stored = repo.list_snapshots().unwrap();

        assert_eq!(stored, vec![snapshot.clone()]);
        assert_eq!(
            repo.find_snapshot(snapshot.source, &snapshot.market_id, snapshot.observed_at)
                .unwrap(),
            Some(snapshot)
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn idempotent_upsert_does_not_duplicate_equivalent_snapshot() {
        let path = unique_path("snapshot-upsert-idempotent");
        let repo = FilesystemSnapshotRepository::new(&path).unwrap();
        let snapshot = sample_snapshot("market-1", 0.41, 0.43);

        assert!(repo.upsert_snapshot(&snapshot).unwrap());
        assert!(!repo.upsert_snapshot(&snapshot).unwrap());
        assert_eq!(repo.list_snapshots().unwrap().len(), 1);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn upsert_replaces_existing_snapshot_with_same_key() {
        let path = unique_path("snapshot-upsert-replace");
        let repo = FilesystemSnapshotRepository::new(&path).unwrap();
        let original = sample_snapshot("market-1", 0.41, 0.43);
        let updated = sample_snapshot("market-1", 0.44, 0.46);

        assert!(repo.upsert_snapshot(&original).unwrap());
        assert!(repo.upsert_snapshot(&updated).unwrap());

        let stored = repo.list_snapshots().unwrap();
        assert_eq!(stored, vec![updated]);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn write_all_uses_temporary_file_and_replaces_target_atomically() {
        let path = unique_path("snapshot-write-all-atomic");
        let repo = FilesystemSnapshotRepository::new(&path).unwrap();
        let snapshot = sample_snapshot("market-1", 0.41, 0.43);

        assert!(repo.upsert_snapshot(&snapshot).unwrap());
        assert!(path.exists());
        assert_eq!(repo.list_snapshots().unwrap(), vec![snapshot]);

        let temp_path = path.with_file_name(format!(
            "{}.tmp",
            path.file_name().unwrap().to_string_lossy()
        ));
        assert!(!temp_path.exists());

        fs::remove_file(path).unwrap();
    }

    fn sample_snapshot(market_id: &str, best_bid: f64, best_ask: f64) -> MarketSnapshot {
        let mut snapshot = MarketSnapshot::new(
            market_id,
            MarketSource::Polymarket,
            "Example market",
            MarketStatus::Open,
            Utc.with_ymd_and_hms(2026, 4, 12, 0, 0, 0).unwrap(),
        )
        .unwrap();
        snapshot.best_bid = Some(best_bid);
        snapshot.best_ask = Some(best_ask);
        snapshot.last_price = Some((best_bid + best_ask) / 2.0);
        snapshot.volume = Some(100.0);
        snapshot
    }

    fn unique_path(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{nanos}.jsonl"))
    }
}
