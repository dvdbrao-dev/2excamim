use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use market_domain::{MarketActivity, MarketActivityKind, MarketSource, Validate};

use crate::{
    ports::StoreCanonicalActivity,
    storage::{CanonicalActivityBatch, StorageError},
};

/// Repository abstraction for canonical market activity.
pub trait ActivityRepository {
    /// Upserts one canonical activity record. Returns `true` when storage changed.
    fn upsert_activity(&self, activity: &MarketActivity) -> Result<bool, StorageError>;

    /// Returns all stored activity records.
    fn list_activity(&self) -> Result<Vec<MarketActivity>, StorageError>;

    /// Finds one activity record by stable dedup key parts.
    fn find_activity(
        &self,
        source: MarketSource,
        market_id: &str,
        kind: MarketActivityKind,
        observed_at: DateTime<Utc>,
        price: Option<f64>,
        quantity: Option<f64>,
    ) -> Result<Option<MarketActivity>, StorageError>;
}

/// Filesystem-backed JSONL activity repository with idempotent upsert.
#[derive(Debug, Clone)]
pub struct FilesystemActivityRepository {
    path: PathBuf,
}

impl FilesystemActivityRepository {
    /// Creates a new filesystem activity repository.
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, StorageError> {
        let path = path.into();
        ensure_store_file(&path)?;
        Ok(Self { path })
    }

    /// Returns the underlying repository path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn write_all(&self, activity: &[MarketActivity]) -> Result<(), StorageError> {
        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(&self.path)?;

        for item in activity {
            serde_json::to_writer(&mut file, item)?;
            file.write_all(b"\n")?;
        }

        file.flush()?;
        Ok(())
    }
}

impl ActivityRepository for FilesystemActivityRepository {
    fn upsert_activity(&self, activity: &MarketActivity) -> Result<bool, StorageError> {
        activity
            .validate()
            .map_err(|error| StorageError::invalid_data(error.to_string()))?;

        let mut records = self.list_activity()?;
        let key = activity_key(
            activity.source,
            &activity.market_id,
            activity.kind,
            activity.observed_at,
            activity.price,
            activity.quantity,
        );

        if let Some(existing) = records.iter_mut().find(|stored| {
            activity_key(
                stored.source,
                &stored.market_id,
                stored.kind,
                stored.observed_at,
                stored.price,
                stored.quantity,
            ) == key
        }) {
            if existing == activity {
                return Ok(false);
            }

            *existing = activity.clone();
            self.write_all(&records)?;
            return Ok(true);
        }

        records.push(activity.clone());
        self.write_all(&records)?;
        Ok(true)
    }

    fn list_activity(&self) -> Result<Vec<MarketActivity>, StorageError> {
        ensure_store_file(&self.path)?;

        let file = File::open(&self.path)?;
        let reader = BufReader::new(file);
        let mut records = Vec::new();

        for (index, line) in reader.lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let activity: MarketActivity = serde_json::from_str(&line).map_err(|error| {
                StorageError::invalid_data(format!(
                    "failed to parse JSONL line {}: {error}",
                    index + 1
                ))
            })?;
            activity.validate().map_err(|error| {
                StorageError::invalid_data(format!(
                    "invalid activity at line {}: {error}",
                    index + 1
                ))
            })?;
            records.push(activity);
        }

        Ok(records)
    }

    fn find_activity(
        &self,
        source: MarketSource,
        market_id: &str,
        kind: MarketActivityKind,
        observed_at: DateTime<Utc>,
        price: Option<f64>,
        quantity: Option<f64>,
    ) -> Result<Option<MarketActivity>, StorageError> {
        let target = activity_key(source, market_id, kind, observed_at, price, quantity);

        Ok(self.list_activity()?.into_iter().find(|item| {
            activity_key(
                item.source,
                &item.market_id,
                item.kind,
                item.observed_at,
                item.price,
                item.quantity,
            ) == target
        }))
    }
}

impl StoreCanonicalActivity for FilesystemActivityRepository {
    type Error = StorageError;

    fn store_canonical_activity(&self, batch: &CanonicalActivityBatch) -> Result<(), Self::Error> {
        for item in &batch.activity {
            self.upsert_activity(item)?;
        }

        Ok(())
    }
}

fn activity_key(
    source: MarketSource,
    market_id: &str,
    kind: MarketActivityKind,
    observed_at: DateTime<Utc>,
    price: Option<f64>,
    quantity: Option<f64>,
) -> String {
    format!(
        "{source:?}:{market_id}:{kind:?}:{}:{}:{}",
        observed_at.to_rfc3339(),
        format_option_f64(price),
        format_option_f64(quantity)
    )
}

fn format_option_f64(value: Option<f64>) -> String {
    match value {
        Some(value) => value.to_string(),
        None => "none".into(),
    }
}

fn ensure_store_file(path: &Path) -> Result<(), StorageError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    OpenOptions::new().create(true).append(true).open(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use chrono::Utc;
    use market_domain::{MarketActivity, MarketActivityKind, MarketSource};

    use super::{ActivityRepository, FilesystemActivityRepository};

    #[test]
    fn writes_and_reads_activity() {
        let path = unique_path("activity-repository");
        let repo = FilesystemActivityRepository::new(&path).unwrap();
        let activity = sample_trade("market-1", "2026-04-12T00:00:00Z", 0.51, 10.0);

        assert!(repo.upsert_activity(&activity).unwrap());
        let stored = repo.list_activity().unwrap();

        assert_eq!(stored, vec![activity.clone()]);
        assert_eq!(
            repo.find_activity(
                activity.source,
                &activity.market_id,
                activity.kind,
                activity.observed_at,
                activity.price,
                activity.quantity
            )
            .unwrap(),
            Some(activity)
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn duplicate_provider_payload_does_not_duplicate_activity() {
        let path = unique_path("activity-repository-duplicate");
        let repo = FilesystemActivityRepository::new(&path).unwrap();
        let activity = sample_trade("market-1", "2026-04-12T00:00:00Z", 0.51, 10.0);

        assert!(repo.upsert_activity(&activity).unwrap());
        assert!(!repo.upsert_activity(&activity).unwrap());
        assert_eq!(repo.list_activity().unwrap().len(), 1);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn replayed_ingestion_batch_remains_idempotent() {
        let path = unique_path("activity-repository-replay");
        let repo = FilesystemActivityRepository::new(&path).unwrap();
        let first = sample_trade("market-1", "2026-04-12T00:00:00Z", 0.51, 10.0);
        let second = sample_trade("market-1", "2026-04-12T00:01:00Z", 0.52, 11.0);

        assert!(repo.upsert_activity(&first).unwrap());
        assert!(repo.upsert_activity(&second).unwrap());
        assert!(!repo.upsert_activity(&first).unwrap());
        assert!(!repo.upsert_activity(&second).unwrap());

        let stored = repo.list_activity().unwrap();
        assert_eq!(stored.len(), 2);
        fs::remove_file(path).unwrap();
    }

    fn sample_trade(
        market_id: &str,
        observed_at: &str,
        price: f64,
        quantity: f64,
    ) -> MarketActivity {
        MarketActivity {
            market_id: market_id.into(),
            source: MarketSource::Polymarket,
            kind: MarketActivityKind::Trade,
            price: Some(price),
            quantity: Some(quantity),
            observed_at: chrono::DateTime::parse_from_rfc3339(observed_at)
                .unwrap()
                .with_timezone(&Utc),
        }
    }

    fn unique_path(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{nanos}.jsonl"))
    }
}
