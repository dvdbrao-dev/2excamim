//! Application-level market watch observation runner.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::{
    adapters::polymarket::{PolymarketHttpAdapter, UreqHttpClient},
    services::{
        PolymarketActivityError, PolymarketActivityService, PolymarketSnapshotRefreshService,
        SnapshotRefreshError, SnapshotRefreshSummary,
    },
    storage::{
        ActivityRepository, FilesystemActivityRepository, FilesystemRawPayloadStore,
        FilesystemSnapshotRepository, StorageError,
    },
};

const ACTOR: &str = "market-watch";

/// Configuration for one market watch observation cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketWatchConfig {
    /// Provider base URL.
    pub base_url: String,
    /// Local state directory used for append-only storage.
    pub state_dir: PathBuf,
    /// Whether to collect and persist activity in addition to snapshots.
    pub with_activity: bool,
}

/// Snapshot observation summary returned to the caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnapshotObservationSummary {
    /// Number of provider markets fetched.
    pub fetched: usize,
    /// Number of canonical snapshots stored or updated.
    pub stored: usize,
    /// Number of provider rows that failed canonical mapping.
    pub failed: usize,
}

/// Activity observation summary returned to the caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActivityObservationSummary {
    /// Number of provider activity items fetched.
    pub fetched: usize,
    /// Number of canonical activity records stored or updated.
    pub stored: usize,
    /// Number of duplicate provider items skipped.
    pub duplicates: usize,
    /// Number of provider rows that failed canonical mapping.
    pub failed: usize,
}

/// Structured summary for one market watch observation cycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MarketWatchObservationSummary {
    /// Stable actor identifier.
    pub actor: String,
    /// Provider base URL used for the run.
    pub base_url: String,
    /// Local state directory used for the run.
    pub state_dir: String,
    /// Snapshot refresh summary.
    pub snapshots: SnapshotObservationSummary,
    /// Optional activity refresh summary.
    pub activity: Option<ActivityObservationSummary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ActivityCycleSummary {
    fetched: usize,
    stored: usize,
    duplicates: usize,
    failed: usize,
}

trait SnapshotCycle {
    fn run_snapshot_cycle(&self) -> Result<SnapshotRefreshSummary, MarketWatchError>;
}

trait ActivityCycle {
    fn run_activity_cycle(&self) -> Result<ActivityCycleSummary, MarketWatchError>;
}

struct SnapshotRefreshRunner<'a> {
    config: &'a MarketWatchConfig,
}

struct ActivityRefreshRunner<'a> {
    config: &'a MarketWatchConfig,
}

/// Typed failure for the market watch application runner.
#[derive(Debug)]
pub enum MarketWatchError {
    /// Failure while preparing or using local storage.
    Storage(StorageError),
    /// Failure in snapshot refresh.
    SnapshotRefresh(SnapshotRefreshError),
    /// Failure in activity collection.
    ActivityRefresh(PolymarketActivityError),
}

impl core::fmt::Display for MarketWatchError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Storage(error) => write!(f, "{error}"),
            Self::SnapshotRefresh(error) => write!(f, "{error}"),
            Self::ActivityRefresh(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for MarketWatchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error),
            Self::SnapshotRefresh(error) => Some(error),
            Self::ActivityRefresh(error) => Some(error),
        }
    }
}

/// Small application runner that performs one read-only market observation cycle.
#[derive(Debug, Clone)]
pub struct MarketWatchRunner {
    config: MarketWatchConfig,
}

impl MarketWatchRunner {
    /// Creates a new market watch runner.
    pub fn new(config: MarketWatchConfig) -> Self {
        Self { config }
    }

    /// Executes one market watch observation cycle.
    pub fn run(&self) -> Result<MarketWatchObservationSummary, MarketWatchError> {
        let snapshot_cycle = SnapshotRefreshRunner {
            config: &self.config,
        };
        let activity_cycle = ActivityRefreshRunner {
            config: &self.config,
        };

        run_cycle(
            &self.config,
            &snapshot_cycle,
            if self.config.with_activity {
                Some(&activity_cycle)
            } else {
                None
            },
        )
    }
}

impl SnapshotCycle for SnapshotRefreshRunner<'_> {
    fn run_snapshot_cycle(&self) -> Result<SnapshotRefreshSummary, MarketWatchError> {
        let adapter = PolymarketHttpAdapter::new(self.config.base_url.clone(), UreqHttpClient);
        let raw_store = FilesystemRawPayloadStore::new(
            self.config.state_dir.join("raw/polymarket-discovery.jsonl"),
        )
        .map_err(MarketWatchError::Storage)?;
        let snapshot_repo =
            FilesystemSnapshotRepository::new(self.config.state_dir.join("snapshots.jsonl"))
                .map_err(MarketWatchError::Storage)?;
        let service = PolymarketSnapshotRefreshService::new(adapter, raw_store, snapshot_repo);

        service.refresh().map_err(MarketWatchError::SnapshotRefresh)
    }
}

impl ActivityCycle for ActivityRefreshRunner<'_> {
    fn run_activity_cycle(&self) -> Result<ActivityCycleSummary, MarketWatchError> {
        let adapter = PolymarketHttpAdapter::new(self.config.base_url.clone(), UreqHttpClient);
        let service = PolymarketActivityService::new(adapter);
        let activity_repo =
            FilesystemActivityRepository::new(self.config.state_dir.join("activity.jsonl"))
                .map_err(MarketWatchError::Storage)?;
        let collection = service
            .collect_activity()
            .map_err(MarketWatchError::ActivityRefresh)?;

        let mut stored = 0usize;
        for activity in &collection.activity.activity {
            if activity_repo
                .upsert_activity(activity)
                .map_err(MarketWatchError::Storage)?
            {
                stored += 1;
            }
        }

        Ok(ActivityCycleSummary {
            fetched: collection.summary.fetched,
            stored,
            duplicates: collection.summary.duplicates,
            failed: collection.summary.failed,
        })
    }
}

fn run_cycle<S, A>(
    config: &MarketWatchConfig,
    snapshot_cycle: &S,
    activity_cycle: Option<&A>,
) -> Result<MarketWatchObservationSummary, MarketWatchError>
where
    S: SnapshotCycle,
    A: ActivityCycle,
{
    let snapshots = snapshot_cycle.run_snapshot_cycle()?;
    let activity = match activity_cycle {
        Some(activity_cycle) => {
            let summary = activity_cycle.run_activity_cycle()?;
            Some(ActivityObservationSummary {
                fetched: summary.fetched,
                stored: summary.stored,
                duplicates: summary.duplicates,
                failed: summary.failed,
            })
        }
        None => None,
    };

    Ok(MarketWatchObservationSummary {
        actor: ACTOR.into(),
        base_url: config.base_url.clone(),
        state_dir: display_path(&config.state_dir),
        snapshots: SnapshotObservationSummary {
            fetched: snapshots.fetched,
            stored: snapshots.stored,
            failed: snapshots.failed,
        },
        activity,
    })
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        run_cycle, ActivityCycle, ActivityCycleSummary, MarketWatchConfig,
        MarketWatchObservationSummary, SnapshotCycle, SnapshotObservationSummary,
    };
    use crate::services::SnapshotRefreshSummary;

    struct FakeSnapshotCycle {
        summary: SnapshotRefreshSummary,
    }

    impl SnapshotCycle for FakeSnapshotCycle {
        fn run_snapshot_cycle(&self) -> Result<SnapshotRefreshSummary, super::MarketWatchError> {
            Ok(self.summary)
        }
    }

    struct FakeActivityCycle {
        summary: ActivityCycleSummary,
    }

    impl ActivityCycle for FakeActivityCycle {
        fn run_activity_cycle(&self) -> Result<ActivityCycleSummary, super::MarketWatchError> {
            Ok(self.summary)
        }
    }

    #[test]
    fn observation_cycle_returns_snapshot_summary_only_when_activity_is_disabled() {
        let config = MarketWatchConfig {
            base_url: "https://example.test".into(),
            state_dir: PathBuf::from("./var/market-watch"),
            with_activity: false,
        };

        let summary = run_cycle::<_, FakeActivityCycle>(
            &config,
            &FakeSnapshotCycle {
                summary: SnapshotRefreshSummary {
                    fetched: 10,
                    stored: 8,
                    failed: 2,
                },
            },
            None,
        )
        .unwrap();

        assert_eq!(
            summary,
            MarketWatchObservationSummary {
                actor: "market-watch".into(),
                base_url: "https://example.test".into(),
                state_dir: "./var/market-watch".into(),
                snapshots: SnapshotObservationSummary {
                    fetched: 10,
                    stored: 8,
                    failed: 2,
                },
                activity: None,
            }
        );
    }

    #[test]
    fn observation_cycle_includes_activity_summary_when_enabled() {
        let config = MarketWatchConfig {
            base_url: "https://example.test".into(),
            state_dir: PathBuf::from("./var/market-watch"),
            with_activity: true,
        };

        let summary = run_cycle(
            &config,
            &FakeSnapshotCycle {
                summary: SnapshotRefreshSummary {
                    fetched: 10,
                    stored: 8,
                    failed: 2,
                },
            },
            Some(&FakeActivityCycle {
                summary: ActivityCycleSummary {
                    fetched: 5,
                    stored: 4,
                    duplicates: 1,
                    failed: 0,
                },
            }),
        )
        .unwrap();

        assert_eq!(summary.activity.as_ref().unwrap().stored, 4);
        assert_eq!(summary.activity.as_ref().unwrap().duplicates, 1);
    }
}
