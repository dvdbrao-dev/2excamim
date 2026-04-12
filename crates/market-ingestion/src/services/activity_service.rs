use std::collections::HashSet;

use crate::{
    adapters::polymarket::{FetchPolymarketActivityPayload, PolymarketAdapterError},
    dto::polymarket::PolymarketActivityDto,
    mappers::polymarket::PolymarketActivityMapper,
    storage::CanonicalActivityBatch,
};

/// Summary returned by a paginated activity read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivityCollectionSummary {
    /// Number of provider activity items fetched across all pages.
    pub fetched: usize,
    /// Number of duplicate provider items skipped.
    pub duplicates: usize,
    /// Number of provider items that failed canonical mapping.
    pub failed: usize,
}

/// Result returned by the Polymarket activity service.
#[derive(Debug, Clone, PartialEq)]
pub struct PolymarketActivityCollection {
    /// Canonical mapped activity.
    pub activity: CanonicalActivityBatch,
    /// Small operational summary.
    pub summary: ActivityCollectionSummary,
}

/// Fatal error for Polymarket activity collection.
#[derive(Debug)]
pub enum PolymarketActivityError {
    Adapter(PolymarketAdapterError),
}

impl core::fmt::Display for PolymarketActivityError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Adapter(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for PolymarketActivityError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Adapter(err) => Some(err),
        }
    }
}

/// Read-only paginated Polymarket activity service.
pub struct PolymarketActivityService<A> {
    adapter: A,
    mapper: PolymarketActivityMapper,
}

impl<A> PolymarketActivityService<A> {
    /// Creates a new Polymarket activity service.
    pub fn new(adapter: A) -> Self {
        Self {
            adapter,
            mapper: PolymarketActivityMapper,
        }
    }
}

impl<A> PolymarketActivityService<A>
where
    A: FetchPolymarketActivityPayload,
{
    /// Collects paginated Polymarket activity and returns deduplicated canonical activity.
    pub fn collect_activity(
        &self,
    ) -> Result<PolymarketActivityCollection, PolymarketActivityError> {
        let mut cursor: Option<String> = None;
        let mut fetched = 0usize;
        let mut duplicates = 0usize;
        let mut unique_items = Vec::new();
        let mut seen = HashSet::new();

        loop {
            let payload = self
                .adapter
                .fetch_activity_payload(cursor.as_deref())
                .map_err(PolymarketActivityError::Adapter)?;

            for item in payload.page.activities {
                fetched += 1;
                let key = activity_key(&item);
                if !seen.insert(key) {
                    duplicates += 1;
                    continue;
                }
                unique_items.push(item);
            }

            match payload.page.next_cursor {
                Some(next_cursor) if !next_cursor.trim().is_empty() => {
                    cursor = Some(next_cursor);
                }
                _ => break,
            }
        }

        let (activity, mapping_errors) = self.mapper.map_activity_lossy(&unique_items);

        Ok(PolymarketActivityCollection {
            activity: CanonicalActivityBatch { activity },
            summary: ActivityCollectionSummary {
                fetched,
                duplicates,
                failed: mapping_errors.len(),
            },
        })
    }
}

fn activity_key(item: &PolymarketActivityDto) -> String {
    match item.activity_id.as_deref() {
        Some(activity_id) if !activity_id.trim().is_empty() => {
            format!("id:{activity_id}")
        }
        _ => format!(
            "fallback:{}:{}:{}:{}:{}",
            item.condition_id,
            item.activity_type,
            item.timestamp,
            item.price
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".into()),
            item.quantity
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".into())
        ),
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::PolymarketActivityService;
    use crate::{
        adapters::polymarket::{
            FetchPolymarketActivityPayload, PolymarketActivityPayload, PolymarketAdapterError,
        },
        dto::polymarket::{PolymarketActivityDto, PolymarketActivityPageDto},
    };

    struct FakePolymarketActivityPayload {
        pages: std::cell::RefCell<std::collections::VecDeque<PolymarketActivityPayload>>,
    }

    impl FetchPolymarketActivityPayload for FakePolymarketActivityPayload {
        fn fetch_activity_payload(
            &self,
            _cursor: Option<&str>,
        ) -> Result<PolymarketActivityPayload, PolymarketAdapterError> {
            self.pages
                .borrow_mut()
                .pop_front()
                .ok_or(PolymarketAdapterError::UnexpectedStatus(404))
        }
    }

    #[test]
    fn activity_service_collects_paginated_activity_and_deduplicates() {
        let service = PolymarketActivityService::new(FakePolymarketActivityPayload {
            pages: std::cell::RefCell::new(std::collections::VecDeque::from(vec![
                PolymarketActivityPayload {
                    raw_body: "page-1".into(),
                    page: PolymarketActivityPageDto {
                        activities: vec![
                            PolymarketActivityDto {
                                activity_id: Some("trade-1".into()),
                                condition_id: "0xabc".into(),
                                activity_type: "trade".into(),
                                price: Some(0.51),
                                quantity: Some(10.0),
                                timestamp: "2026-04-12T00:00:00Z".into(),
                            },
                            PolymarketActivityDto {
                                activity_id: Some("trade-1".into()),
                                condition_id: "0xabc".into(),
                                activity_type: "trade".into(),
                                price: Some(0.51),
                                quantity: Some(10.0),
                                timestamp: "2026-04-12T00:00:00Z".into(),
                            },
                        ],
                        next_cursor: Some("page-2".into()),
                    },
                    fetched_at: Utc::now(),
                },
                PolymarketActivityPayload {
                    raw_body: "page-2".into(),
                    page: PolymarketActivityPageDto {
                        activities: vec![PolymarketActivityDto {
                            activity_id: Some("trade-2".into()),
                            condition_id: "0xabc".into(),
                            activity_type: "trade".into(),
                            price: Some(0.52),
                            quantity: Some(11.0),
                            timestamp: "2026-04-12T00:01:00Z".into(),
                        }],
                        next_cursor: None,
                    },
                    fetched_at: Utc::now(),
                },
            ])),
        });

        let result = service.collect_activity().unwrap();
        assert_eq!(result.summary.fetched, 3);
        assert_eq!(result.summary.duplicates, 1);
        assert_eq!(result.summary.failed, 0);
        assert_eq!(result.activity.activity.len(), 2);
    }

    #[test]
    fn activity_service_counts_malformed_activity_rows() {
        let service = PolymarketActivityService::new(FakePolymarketActivityPayload {
            pages: std::cell::RefCell::new(std::collections::VecDeque::from(vec![
                PolymarketActivityPayload {
                    raw_body: "page-1".into(),
                    page: PolymarketActivityPageDto {
                        activities: vec![
                            PolymarketActivityDto {
                                activity_id: Some("trade-1".into()),
                                condition_id: "0xabc".into(),
                                activity_type: "trade".into(),
                                price: Some(0.51),
                                quantity: Some(10.0),
                                timestamp: "2026-04-12T00:00:00Z".into(),
                            },
                            PolymarketActivityDto {
                                activity_id: Some("bad-1".into()),
                                condition_id: "0xabc".into(),
                                activity_type: "mystery".into(),
                                price: None,
                                quantity: None,
                                timestamp: "not-a-timestamp".into(),
                            },
                        ],
                        next_cursor: None,
                    },
                    fetched_at: Utc::now(),
                },
            ])),
        });

        let result = service.collect_activity().unwrap();
        assert_eq!(result.summary.fetched, 2);
        assert_eq!(result.summary.duplicates, 0);
        assert_eq!(result.summary.failed, 1);
        assert_eq!(result.activity.activity.len(), 1);
    }
}
