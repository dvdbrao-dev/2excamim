use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::UNIX_EPOCH,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    events::{EventEnvelope, Linkage, Provenance, SignalGenerated, SignalSide, SourceKind},
    store::{JsonlEventStore, StoredEvent},
};

use super::HandoffError;

const RESEARCH_TIMEFRAME: &str = "research_snapshot";
const DECODER_SCRIPT_PATH: &str = "research_prediction_markets/export_signals_json.py";
const RESEARCH_ACTOR: &str = "research_prediction_markets";
pub const RESEARCH_SIGNAL_HANDOFF_SCHEMA_VERSION: &str = "research-signals.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ResearchSignalIngestOptions {
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResearchSignalInputRecord {
    pub handoff_schema_version: Option<String>,
    pub row_number: usize,
    pub market_id: Option<String>,
    pub timestamp: Option<String>,
    pub signal_name: Option<String>,
    pub strength: Option<f64>,
    pub direction: Option<String>,
    pub probability: Option<f64>,
    pub spread_tight: Option<f64>,
    pub volume_spike_24h: Option<f64>,
    pub price_deviation_vwap_1h: Option<f64>,
    pub source: Option<String>,
    pub metadata: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResearchSignalIngestRejection {
    pub row_number: usize,
    pub market_id: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestedResearchSignal {
    pub row_number: usize,
    pub signal_id: String,
    pub event_type: String,
    pub deduplicated: bool,
    pub event_written: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResearchSignalRejectedReason {
    pub reason: String,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResearchSignalIngestReport {
    pub handoff_schema_version: String,
    pub input_path: PathBuf,
    pub dry_run: bool,
    pub batch_trace_id: String,
    pub input_file_size_bytes: u64,
    pub rows_read: usize,
    pub rows_valid: usize,
    pub rows_invalid: usize,
    pub events_written: usize,
    pub duplicates: usize,
    pub generated_event_types: Vec<String>,
    pub rejected_reasons: Vec<ResearchSignalRejectedReason>,
    pub ingested_signals: Vec<IngestedResearchSignal>,
    pub rejections: Vec<ResearchSignalIngestRejection>,
}

pub fn ingest_research_signals_file(
    store: &JsonlEventStore,
    input_path: impl AsRef<Path>,
    options: ResearchSignalIngestOptions,
) -> Result<ResearchSignalIngestReport, HandoffError> {
    let input_path = input_path.as_ref().to_path_buf();
    let records = decode_research_signal_records(&input_path)?;
    let input_file_size_bytes = fs::metadata(&input_path)?.len();
    let batch_trace_id = build_batch_trace_id(&input_path)?;

    let mut rows_valid = 0usize;
    let mut events_written = 0usize;
    let mut duplicates = 0usize;
    let mut ingested_signals = Vec::new();
    let mut rejections = Vec::new();
    let mut rejection_reason_counts = BTreeMap::new();

    for record in records.iter().cloned() {
        match translate_record_to_signal_event(&input_path, &batch_trace_id, &record) {
            Ok(envelope) => {
                rows_valid += 1;
                let signal_id = envelope.payload.signal_id.clone();
                let stored = StoredEvent::try_from(envelope)?;
                let appended = if options.dry_run {
                    false
                } else {
                    store.append_event(&stored)?
                };

                if appended {
                    events_written += 1;
                } else if !options.dry_run {
                    duplicates += 1;
                }

                ingested_signals.push(IngestedResearchSignal {
                    row_number: record.row_number,
                    signal_id,
                    event_type: stored.event_type.as_str().to_string(),
                    deduplicated: !options.dry_run && !appended,
                    event_written: appended,
                });
            }
            Err(error) => {
                let reason = error.to_string();
                *rejection_reason_counts
                    .entry(reason.clone())
                    .or_insert(0usize) += 1;
                rejections.push(ResearchSignalIngestRejection {
                    row_number: record.row_number,
                    market_id: record.market_id.clone(),
                    reason,
                });
            }
        }
    }

    Ok(ResearchSignalIngestReport {
        handoff_schema_version: RESEARCH_SIGNAL_HANDOFF_SCHEMA_VERSION.to_string(),
        input_path,
        dry_run: options.dry_run,
        batch_trace_id,
        input_file_size_bytes,
        rows_read: records.len(),
        rows_valid,
        rows_invalid: rejections.len(),
        events_written,
        duplicates,
        generated_event_types: vec!["signal.generated".to_string()],
        rejected_reasons: rejection_reason_counts
            .into_iter()
            .map(|(reason, count)| ResearchSignalRejectedReason { reason, count })
            .collect(),
        ingested_signals,
        rejections,
    })
}

fn decode_research_signal_records(
    input_path: &Path,
) -> Result<Vec<ResearchSignalInputRecord>, HandoffError> {
    if !input_path.exists() {
        return Err(HandoffError::invalid_input(format!(
            "input file {} does not exist",
            input_path.display()
        )));
    }

    match input_path.extension().and_then(|ext| ext.to_str()) {
        Some("parquet") => decode_parquet_via_python(input_path),
        _ => Err(HandoffError::unsupported_format(
            input_path,
            "v1 only accepts research Parquet output",
        )),
    }
}

fn decode_parquet_via_python(
    input_path: &Path,
) -> Result<Vec<ResearchSignalInputRecord>, HandoffError> {
    let script_path = Path::new(DECODER_SCRIPT_PATH);
    if !script_path.exists() {
        return Err(HandoffError::invalid_input(format!(
            "decoder script {} is missing",
            script_path.display()
        )));
    }

    let interpreter = resolve_python_interpreter()?;
    let output = Command::new(&interpreter)
        .arg(script_path)
        .arg(input_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8(output.stderr)?;
        let message = stderr.trim();
        return Err(HandoffError::DecoderFailed(if message.is_empty() {
            format!(
                "python decoder failed while reading {}",
                input_path.display()
            )
        } else {
            message.to_string()
        }));
    }

    let stdout = String::from_utf8(output.stdout)?;
    let mut records = Vec::new();
    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        records.push(serde_json::from_str(line)?);
    }
    Ok(records)
}

fn resolve_python_interpreter() -> Result<PathBuf, HandoffError> {
    let venv_python = PathBuf::from("research_prediction_markets/.venv/bin/python");
    if venv_python.exists() {
        return Ok(venv_python);
    }

    let output = Command::new("python3").arg("--version").output();
    match output {
        Ok(result) if result.status.success() => Ok(PathBuf::from("python3")),
        Ok(_) | Err(_) => Err(HandoffError::invalid_input(
            "no Python interpreter available for Parquet decoding; expected research_prediction_markets/.venv/bin/python or python3",
        )),
    }
}

fn translate_record_to_signal_event(
    input_path: &Path,
    batch_trace_id: &str,
    record: &ResearchSignalInputRecord,
) -> Result<EventEnvelope<SignalGenerated>, HandoffError> {
    validate_handoff_schema_version(record.handoff_schema_version.as_deref())?;
    let market_id = required_string(record.market_id.as_deref(), "market_id")?;
    let timestamp_raw = required_string(record.timestamp.as_deref(), "timestamp")?;
    let signal_name = required_string(record.signal_name.as_deref(), "signal_name")?;
    let direction = required_string(record.direction.as_deref(), "direction")?;
    let source = required_string(record.source.as_deref(), "source")?;
    let strength = record
        .strength
        .ok_or_else(|| HandoffError::invalid_input("missing strength"))?;

    validate_probability(strength, "strength")?;
    validate_probability_optional(record.probability, "probability")?;
    validate_json_object_string(record.metadata.as_deref())?;

    let timestamp = parse_timestamp(timestamp_raw)?;
    let side = map_direction_to_side(direction)?;
    let signal_id = build_signal_id(source, market_id, signal_name, &timestamp, direction);
    let aggregate_key = Some(build_instrument(source, market_id));
    let rationale = Some(build_rationale(signal_name, direction, record.probability));
    let provenance_notes = build_provenance_notes(record, batch_trace_id)?;

    Ok(EventEnvelope::new_signal_generated(
        "runtime.handoff.research_signals",
        aggregate_key,
        Linkage {
            hypothesis_id: None,
            signal_id: Some(signal_id.clone()),
            decision_id: None,
            order_id: None,
            position_id: None,
            parent_event_id: None,
            correlation_id: Some(signal_id.clone()),
        },
        Provenance {
            source_kind: SourceKind::Research,
            source_ref: Some(format!(
                "research-signals://{}#row={}",
                input_path.display(),
                record.row_number
            )),
            producer_run_id: Some(batch_trace_id.to_string()),
            actor: Some(RESEARCH_ACTOR.to_string()),
            trace_id: Some(format!("{batch_trace_id}-row-{}", record.row_number)),
            notes: Some(provenance_notes),
        },
        SignalGenerated {
            signal_id,
            hypothesis_id: None,
            instrument: build_instrument(source, market_id),
            timeframe: RESEARCH_TIMEFRAME.to_string(),
            side,
            strength,
            rationale,
        },
    )?)
}

fn validate_handoff_schema_version(value: Option<&str>) -> Result<(), HandoffError> {
    match value {
        Some(version) if version == RESEARCH_SIGNAL_HANDOFF_SCHEMA_VERSION => Ok(()),
        Some(version) => Err(HandoffError::invalid_input(format!(
            "handoff_schema_version must be {RESEARCH_SIGNAL_HANDOFF_SCHEMA_VERSION}; got {version}"
        ))),
        None => Err(HandoffError::invalid_input(
            "missing handoff_schema_version",
        )),
    }
}

fn required_string<'a>(value: Option<&'a str>, field: &str) -> Result<&'a str, HandoffError> {
    let value = value.ok_or_else(|| HandoffError::invalid_input(format!("missing {field}")))?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(HandoffError::invalid_input(format!(
            "{field} cannot be blank"
        )));
    }
    Ok(trimmed)
}

fn parse_timestamp(value: &str) -> Result<DateTime<Utc>, HandoffError> {
    let parsed = DateTime::parse_from_rfc3339(value).map_err(|error| {
        HandoffError::invalid_input(format!("timestamp must be RFC3339: {error}"))
    })?;
    Ok(parsed.with_timezone(&Utc))
}

fn map_direction_to_side(value: &str) -> Result<SignalSide, HandoffError> {
    match value {
        "long_yes" | "short_no" => Ok(SignalSide::Long),
        "long_no" | "short_yes" => Ok(SignalSide::Short),
        _ => Err(HandoffError::invalid_input(format!(
            "direction must be one of long_yes, long_no, short_yes, short_no; got {value}"
        ))),
    }
}

fn build_signal_id(
    source: &str,
    market_id: &str,
    signal_name: &str,
    timestamp: &DateTime<Utc>,
    direction: &str,
) -> String {
    sanitize_id_component(&format!(
        "research-{}-{}-{}-{}-{}",
        source,
        market_id,
        signal_name,
        timestamp.format("%Y%m%dT%H%M%S%.3fZ"),
        direction
    ))
}

fn build_instrument(source: &str, market_id: &str) -> String {
    format!("{source}:{market_id}")
}

fn build_rationale(signal_name: &str, direction: &str, probability: Option<f64>) -> String {
    match probability {
        Some(probability) => format!(
            "research signal_name={signal_name} direction={direction} probability={probability}"
        ),
        None => format!("research signal_name={signal_name} direction={direction}"),
    }
}

fn build_provenance_notes(
    record: &ResearchSignalInputRecord,
    batch_trace_id: &str,
) -> Result<String, HandoffError> {
    let metadata_value = match record.metadata.as_deref() {
        Some(raw) => serde_json::from_str::<serde_json::Value>(raw)?,
        None => serde_json::Value::Null,
    };

    Ok(serde_json::to_string(&json!({
        "handoff_schema_version": record.handoff_schema_version,
        "batch_trace_id": batch_trace_id,
        "row_number": record.row_number,
        "market_id": record.market_id,
        "signal_name": record.signal_name,
        "direction": record.direction,
        "probability": record.probability,
        "spread_tight": record.spread_tight,
        "volume_spike_24h": record.volume_spike_24h,
        "price_deviation_vwap_1h": record.price_deviation_vwap_1h,
        "source": record.source,
        "metadata": metadata_value,
    }))?)
}

fn build_batch_trace_id(input_path: &Path) -> Result<String, HandoffError> {
    let metadata = fs::metadata(input_path)?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_secs())
        .unwrap_or(0);
    let file_name = input_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("input");

    Ok(sanitize_id_component(&format!(
        "research-batch-{}-{}-{}-{}",
        RESEARCH_SIGNAL_HANDOFF_SCHEMA_VERSION,
        file_name,
        metadata.len(),
        modified
    )))
}

fn validate_probability(value: f64, field: &str) -> Result<(), HandoffError> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(HandoffError::invalid_input(format!(
            "{field} must be in [0,1]"
        )));
    }
    Ok(())
}

fn validate_probability_optional(value: Option<f64>, field: &str) -> Result<(), HandoffError> {
    if let Some(value) = value {
        validate_probability(value, field)?;
    }
    Ok(())
}

fn validate_json_object_string(value: Option<&str>) -> Result<(), HandoffError> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.trim().is_empty() {
        return Err(HandoffError::invalid_input(
            "metadata cannot be blank when provided",
        ));
    }
    let parsed: serde_json::Value = serde_json::from_str(value)?;
    if !parsed.is_object() {
        return Err(HandoffError::invalid_input(
            "metadata must be a JSON object string",
        ));
    }
    Ok(())
}

fn sanitize_id_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
            _ => '_',
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_store_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("twoexcamim-handoff-{name}-{nanos}.jsonl"))
    }

    fn valid_record() -> ResearchSignalInputRecord {
        ResearchSignalInputRecord {
            handoff_schema_version: Some(RESEARCH_SIGNAL_HANDOFF_SCHEMA_VERSION.into()),
            row_number: 1,
            market_id: Some("market-1".into()),
            timestamp: Some("2026-04-03T17:57:41.047Z".into()),
            signal_name: Some("vwap_reversion".into()),
            strength: Some(0.8),
            direction: Some("long_yes".into()),
            probability: Some(0.2),
            spread_tight: Some(0.9),
            volume_spike_24h: Some(1.0),
            price_deviation_vwap_1h: Some(-12.0),
            source: Some("polymarket".into()),
            metadata: Some("{\"foo\":\"bar\"}".into()),
        }
    }

    #[test]
    fn translates_valid_record_to_signal_generated() {
        let envelope = translate_record_to_signal_event(
            Path::new("signals.parquet"),
            "batch-1",
            &valid_record(),
        )
        .unwrap();

        assert_eq!(envelope.payload.instrument, "polymarket:market-1");
        assert_eq!(envelope.payload.timeframe, RESEARCH_TIMEFRAME);
        assert_eq!(envelope.payload.side, SignalSide::Long);
        assert_eq!(envelope.payload.hypothesis_id, None);
        assert_eq!(
            envelope.linkage.correlation_id,
            Some(envelope.payload.signal_id.clone())
        );
        assert_eq!(
            envelope.provenance.producer_run_id.as_deref(),
            Some("batch-1")
        );
    }

    #[test]
    fn rejects_invalid_direction() {
        let mut record = valid_record();
        record.direction = Some("sideways".into());

        let error =
            translate_record_to_signal_event(Path::new("signals.parquet"), "batch-1", &record)
                .unwrap_err();

        assert!(error
            .to_string()
            .contains("direction must be one of long_yes, long_no, short_yes, short_no"));
    }

    #[test]
    fn ingests_and_deduplicates_records() {
        let path = temp_store_path("dedupe");
        let store = JsonlEventStore::new(&path).unwrap();
        let input_path = Path::new("signals.parquet");
        let record = valid_record();

        let first_event = translate_record_to_signal_event(input_path, "batch-1", &record).unwrap();
        let second_event =
            translate_record_to_signal_event(input_path, "batch-1", &record).unwrap();

        let appended_first = store
            .append_event(&StoredEvent::try_from(first_event).unwrap())
            .unwrap();
        let appended_second = store
            .append_event(&StoredEvent::try_from(second_event).unwrap())
            .unwrap();

        assert!(appended_first);
        assert!(!appended_second);

        let _ = fs::remove_file(path);
    }
}
