use chrono::{DateTime, Utc};

use crate::GeneratedSignalContext;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmationEra {
    pub era_id: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmationEraWindow {
    pub era: ConfirmationEra,
    pub signals: Vec<GeneratedSignalContext>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationEraSplit {
    EraCount(usize),
    WindowSizeSeconds(i64),
}

pub fn split_generated_signals_into_eras(
    signals: &[GeneratedSignalContext],
    split: ConfirmationEraSplit,
) -> Vec<ConfirmationEraWindow> {
    let mut ordered = signals.to_vec();
    ordered.sort_by(|left, right| {
        left.generated_at
            .cmp(&right.generated_at)
            .then_with(|| left.signal_id.cmp(&right.signal_id))
    });

    match split {
        ConfirmationEraSplit::EraCount(era_count) => split_by_era_count(&ordered, era_count),
        ConfirmationEraSplit::WindowSizeSeconds(window_size_seconds) => {
            split_by_window_size(&ordered, window_size_seconds)
        }
    }
}

fn split_by_era_count(
    ordered: &[GeneratedSignalContext],
    era_count: usize,
) -> Vec<ConfirmationEraWindow> {
    if ordered.is_empty() || era_count == 0 {
        return Vec::new();
    }

    let target_eras = era_count.min(ordered.len());
    let chunk_size = ordered.len().div_ceil(target_eras);
    ordered
        .chunks(chunk_size.max(1))
        .enumerate()
        .map(|(index, chunk)| era_window(index, chunk.to_vec()))
        .collect()
}

fn split_by_window_size(
    ordered: &[GeneratedSignalContext],
    window_size_seconds: i64,
) -> Vec<ConfirmationEraWindow> {
    if ordered.is_empty() || window_size_seconds <= 0 {
        return Vec::new();
    }

    let anchor = ordered[0].generated_at;
    let mut windows = Vec::new();
    let mut current_bucket = None;
    let mut current_signals = Vec::new();

    for signal in ordered {
        let seconds_from_anchor = signal
            .generated_at
            .signed_duration_since(anchor)
            .num_seconds()
            .max(0);
        let bucket = seconds_from_anchor / window_size_seconds;

        if current_bucket != Some(bucket) && !current_signals.is_empty() {
            windows.push(current_signals);
            current_signals = Vec::new();
        }

        current_bucket = Some(bucket);
        current_signals.push(signal.clone());
    }

    if !current_signals.is_empty() {
        windows.push(current_signals);
    }

    windows
        .into_iter()
        .enumerate()
        .map(|(index, signals)| era_window(index, signals))
        .collect()
}

fn era_window(index: usize, signals: Vec<GeneratedSignalContext>) -> ConfirmationEraWindow {
    let start_time = signals[0].generated_at;
    let end_time = signals[signals.len() - 1].generated_at;
    ConfirmationEraWindow {
        era: ConfirmationEra {
            era_id: format!("era-{:03}", index + 1),
            start_time,
            end_time,
            label: None,
        },
        signals,
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone, Utc};
    use market_domain::{MarketSignalDirection, MarketSource};

    use super::{split_generated_signals_into_eras, ConfirmationEraSplit};
    use crate::GeneratedSignalContext;

    fn signal(id: &str, minutes_after: i64) -> GeneratedSignalContext {
        GeneratedSignalContext {
            signal_id: id.into(),
            market_id: format!("market-{id}"),
            signal_name: "odds_jump".into(),
            direction: MarketSignalDirection::Yes,
            source: MarketSource::Synthetic,
            confidence: 0.6,
            generated_at: Utc.with_ymd_and_hms(2026, 4, 13, 12, 0, 0).unwrap()
                + Duration::minutes(minutes_after),
        }
    }

    #[test]
    fn splits_into_deterministic_eras_by_count() {
        let signals = vec![
            signal("3", 30),
            signal("1", 0),
            signal("4", 45),
            signal("2", 15),
        ];

        let first = split_generated_signals_into_eras(&signals, ConfirmationEraSplit::EraCount(2));
        let second = split_generated_signals_into_eras(&signals, ConfirmationEraSplit::EraCount(2));

        assert_eq!(first, second);
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].era.era_id, "era-001");
        assert_eq!(
            first[0]
                .signals
                .iter()
                .map(|signal| signal.signal_id.as_str())
                .collect::<Vec<_>>(),
            vec!["1", "2"]
        );
        assert_eq!(
            first[1]
                .signals
                .iter()
                .map(|signal| signal.signal_id.as_str())
                .collect::<Vec<_>>(),
            vec!["3", "4"]
        );
    }

    #[test]
    fn splits_into_deterministic_eras_by_window_size() {
        let signals = vec![
            signal("1", 0),
            signal("2", 20),
            signal("3", 61),
            signal("4", 130),
        ];

        let windows = split_generated_signals_into_eras(
            &signals,
            ConfirmationEraSplit::WindowSizeSeconds(3600),
        );

        assert_eq!(windows.len(), 3);
        assert_eq!(
            windows[0]
                .signals
                .iter()
                .map(|signal| signal.signal_id.as_str())
                .collect::<Vec<_>>(),
            vec!["1", "2"]
        );
        assert_eq!(
            windows[1]
                .signals
                .iter()
                .map(|signal| signal.signal_id.as_str())
                .collect::<Vec<_>>(),
            vec!["3"]
        );
        assert_eq!(
            windows[2]
                .signals
                .iter()
                .map(|signal| signal.signal_id.as_str())
                .collect::<Vec<_>>(),
            vec!["4"]
        );
    }
}
