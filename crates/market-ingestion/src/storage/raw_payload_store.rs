use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

use crate::{
    ports::StoreRawPayloads,
    storage::{RawPayloadBatch, RawPayloadRecord, StorageError},
};

/// Storage abstraction for captured raw payloads.
pub trait RawPayloadStore {
    /// Appends a raw payload record.
    fn append(&self, record: &RawPayloadRecord) -> Result<(), StorageError>;

    /// Reads all raw payload records in storage order.
    fn read_all(&self) -> Result<Vec<RawPayloadRecord>, StorageError>;
}

/// Filesystem-backed JSONL raw payload store.
#[derive(Debug, Clone)]
pub struct FilesystemRawPayloadStore {
    path: PathBuf,
}

impl FilesystemRawPayloadStore {
    /// Creates a new raw payload store at the given path.
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, StorageError> {
        let path = path.into();
        ensure_store_file(&path)?;
        Ok(Self { path })
    }

    /// Returns the underlying store path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl RawPayloadStore for FilesystemRawPayloadStore {
    fn append(&self, record: &RawPayloadRecord) -> Result<(), StorageError> {
        record.validate()?;

        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.path)?;
        serde_json::to_writer(&mut file, record)?;
        file.write_all(b"\n")?;
        file.flush()?;
        Ok(())
    }

    fn read_all(&self) -> Result<Vec<RawPayloadRecord>, StorageError> {
        ensure_store_file(&self.path)?;

        let file = File::open(&self.path)?;
        let reader = BufReader::new(file);
        let mut records = Vec::new();

        for (index, line) in reader.lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let record: RawPayloadRecord = serde_json::from_str(&line).map_err(|error| {
                StorageError::invalid_data(format!(
                    "failed to parse JSONL line {}: {error}",
                    index + 1
                ))
            })?;
            record.validate().map_err(|error| {
                StorageError::invalid_data(format!(
                    "invalid raw payload at line {}: {error}",
                    index + 1
                ))
            })?;
            records.push(record);
        }

        Ok(records)
    }
}

impl StoreRawPayloads for FilesystemRawPayloadStore {
    type Error = StorageError;

    fn store_raw_payloads(&self, batch: &RawPayloadBatch) -> Result<(), Self::Error> {
        for record in &batch.records {
            self.append(record)?;
        }

        Ok(())
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
    use market_domain::MarketSource;

    use super::{FilesystemRawPayloadStore, RawPayloadStore};
    use crate::storage::RawPayloadRecord;

    #[test]
    fn writes_and_reads_raw_payload_records() {
        let path = unique_path("raw-payload-store");
        let store = FilesystemRawPayloadStore::new(&path).unwrap();

        let record = RawPayloadRecord {
            source: MarketSource::Polymarket,
            source_event_id: Some("evt-1".into()),
            payload_kind: "polymarket.discovery".into(),
            payload: br#"{"markets":[]}"#.to_vec(),
            captured_at: Utc::now(),
        };

        store.append(&record).unwrap();
        let stored = store.read_all().unwrap();

        assert_eq!(stored, vec![record]);
        fs::remove_file(path).unwrap();
    }

    fn unique_path(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{nanos}.jsonl"))
    }
}
