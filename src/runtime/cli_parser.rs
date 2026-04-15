use std::path::PathBuf;

use chrono::{DateTime, Utc};

use crate::events::FillSide;

use super::{
    Command, Config, ParseOutcome, RuntimeError, DEFAULT_SNAPSHOTS_PATH, DEFAULT_STORE_PATH,
};

pub(crate) fn parse_args(args: Vec<String>) -> Result<ParseOutcome, RuntimeError> {
    let mut args = args.into_iter();
    let _program_name = args.next();

    let mut positionals = Vec::new();
    let mut store_path = PathBuf::from(DEFAULT_STORE_PATH);
    let mut snapshots_path = PathBuf::from(DEFAULT_SNAPSHOTS_PATH);
    let mut format = super::OutputFormat::Text;
    let mut dry_run = false;
    let mut horizon_seconds = 3600_i64;
    let mut delta_threshold = 0.02_f64;
    let mut horizons = vec![3600_i64];
    let mut confidence_thresholds = vec![0.6_f64];
    let mut eras = 3_usize;
    let mut window_size_seconds = None;
    let mut policy_file = None;
    let mut output_path = None;
    let mut fill_id = None;
    let mut order_id = None;
    let mut decision_id = None;
    let mut instrument = None;
    let mut side = None;
    let mut quantity = None;
    let mut price = None;
    let mut venue = None;
    let mut executed_at = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--store" => {
                let path = args.next().ok_or_else(|| {
                    RuntimeError::Usage("missing value for --store\n\n".to_string() + &usage())
                })?;
                store_path = PathBuf::from(path);
            }
            "--json" => {
                format = super::OutputFormat::Json;
            }
            "--snapshots" => {
                let path = args.next().ok_or_else(|| {
                    RuntimeError::Usage("missing value for --snapshots\n\n".to_string() + &usage())
                })?;
                snapshots_path = PathBuf::from(path);
            }
            "--horizon-seconds" => {
                horizon_seconds = required_flag_value(&mut args, "--horizon-seconds")?
                    .parse::<i64>()
                    .map_err(|_| {
                        RuntimeError::Usage(format!(
                            "invalid numeric value for --horizon-seconds\n\n{}",
                            usage()
                        ))
                    })?;
            }
            "--delta-threshold" => {
                delta_threshold = required_flag_value(&mut args, "--delta-threshold")?
                    .parse::<f64>()
                    .map_err(|_| {
                        RuntimeError::Usage(format!(
                            "invalid numeric value for --delta-threshold\n\n{}",
                            usage()
                        ))
                    })?;
            }
            "--horizons" => {
                horizons =
                    parse_csv_i64(&required_flag_value(&mut args, "--horizons")?, "--horizons")?;
            }
            "--confidence-thresholds" => {
                confidence_thresholds = parse_csv_f64(
                    &required_flag_value(&mut args, "--confidence-thresholds")?,
                    "--confidence-thresholds",
                )?;
            }
            "--eras" => {
                eras = required_flag_value(&mut args, "--eras")?
                    .parse::<usize>()
                    .map_err(|_| {
                        RuntimeError::Usage(format!(
                            "invalid numeric value for --eras\n\n{}",
                            usage()
                        ))
                    })?;
            }
            "--window-size" => {
                window_size_seconds = Some(
                    required_flag_value(&mut args, "--window-size")?
                        .parse::<i64>()
                        .map_err(|_| {
                            RuntimeError::Usage(format!(
                                "invalid numeric value for --window-size\n\n{}",
                                usage()
                            ))
                        })?,
                );
            }
            "--policy-file" => {
                policy_file = Some(PathBuf::from(required_flag_value(
                    &mut args,
                    "--policy-file",
                )?));
            }
            "--output" => {
                output_path = Some(PathBuf::from(required_flag_value(&mut args, "--output")?));
            }
            "--dry-run" => {
                dry_run = true;
            }
            "--research-signals" => {
                positionals.push(arg);
                let path = args.next().ok_or_else(|| {
                    RuntimeError::Usage(
                        "missing value for --research-signals\n\n".to_string() + &usage(),
                    )
                })?;
                positionals.push(path);
            }
            "--fill-id" => fill_id = Some(required_flag_value(&mut args, "--fill-id")?),
            "--order-id" => order_id = Some(required_flag_value(&mut args, "--order-id")?),
            "--decision-id" => decision_id = Some(required_flag_value(&mut args, "--decision-id")?),
            "--instrument" => instrument = Some(required_flag_value(&mut args, "--instrument")?),
            "--side" => side = Some(required_flag_value(&mut args, "--side")?),
            "--quantity" => quantity = Some(required_flag_value(&mut args, "--quantity")?),
            "--price" => price = Some(required_flag_value(&mut args, "--price")?),
            "--venue" => venue = Some(required_flag_value(&mut args, "--venue")?),
            "--executed-at" => executed_at = Some(required_flag_value(&mut args, "--executed-at")?),
            "-h" | "--help" => {
                return Ok(ParseOutcome::Help);
            }
            _ if arg.starts_with('-') => {
                return Err(RuntimeError::Usage(format!(
                    "unknown flag: {arg}\n\n{}",
                    usage()
                )));
            }
            _ => positionals.push(arg),
        }
    }

    let command = match positionals.as_slice() {
        [] => Command::Summary,
        [single] if single == "summary" => Command::Summary,
        [single, id] if single == "signal" => Command::Signal {
            signal_id: id.clone(),
        },
        [single, id] if single == "decision" => Command::Decision {
            decision_id: id.clone(),
        },
        [single, id] if single == "order" => Command::Order {
            order_id: id.clone(),
        },
        [single, id] if single == "fill" => Command::Fill {
            fill_id: id.clone(),
        },
        [observe, entity] if observe == "observe" && entity == "fill" => Command::ObserveFill {
            request: crate::materialization::FillObservationRequest {
                fill_id: required_value(fill_id, "--fill-id")?,
                order_id: required_value(order_id, "--order-id")?,
                decision_id,
                instrument,
                side: parse_fill_side(&required_value(side, "--side")?)?,
                quantity: parse_f64_flag(&required_value(quantity, "--quantity")?, "--quantity")?,
                price: parse_f64_flag(&required_value(price, "--price")?, "--price")?,
                venue,
                executed_at: parse_timestamp_flag(
                    &required_value(executed_at, "--executed-at")?,
                    "--executed-at",
                )?,
            },
        },
        [inspect, entity, id] if inspect == "inspect" && entity == "signal" => Command::Signal {
            signal_id: id.clone(),
        },
        [inspect, entity, id] if inspect == "inspect" && entity == "decision" => {
            Command::Decision {
                decision_id: id.clone(),
            }
        }
        [inspect, entity, id] if inspect == "inspect" && entity == "order" => Command::Order {
            order_id: id.clone(),
        },
        [inspect, entity, id] if inspect == "inspect" && entity == "fill" => Command::Fill {
            fill_id: id.clone(),
        },
        [policy, entity, id] if policy == "policy" && entity == "signal" => Command::PolicySignal {
            signal_id: id.clone(),
        },
        [policy, entity, id] if policy == "policy" && entity == "decision" => {
            Command::PolicyDecision {
                decision_id: id.clone(),
            }
        }
        [policy, entity, id] if policy == "policy" && entity == "order" => Command::PolicyOrder {
            order_id: id.clone(),
        },
        [ingest, kind, path] if ingest == "ingest" && kind == "research-signals" => {
            Command::IngestResearchSignals {
                input_path: PathBuf::from(path),
            }
        }
        [confirm, entity] if confirm == "confirm" && entity == "signals" => Command::ConfirmSignals,
        [single] if single == "confirm-signals" => Command::ConfirmSignals,
        [single] if single == "measure-confirmation-outcomes" => {
            Command::MeasureConfirmationOutcomes
        }
        [single] if single == "evaluate-confirmation-policy" => Command::EvaluateConfirmationPolicy,
        [single] if single == "walkforward-confirmation-policy" => {
            Command::WalkForwardConfirmationPolicy
        }
        [single] if single == "propose-confirmation-policy" => Command::ProposeConfirmationPolicy,
        [materialize, entity] if materialize == "materialize" && entity == "decisions" => {
            Command::MaterializeDecisions
        }
        [materialize, entity] if materialize == "materialize" && entity == "orders" => {
            Command::MaterializeOrders
        }
        [submit, entity] if submit == "submit" && entity == "orders" => Command::SubmitOrders,
        [run, batch, flag, path]
            if run == "run" && batch == "batch" && flag == "--research-signals" =>
        {
            Command::RunBatch {
                research_signals_path: PathBuf::from(path),
            }
        }
        _ => {
            return Err(RuntimeError::Usage(format!(
                "invalid command\n\n{}",
                usage()
            )));
        }
    };

    Ok(ParseOutcome::Config(Config {
        command,
        store_path,
        snapshots_path,
        format,
        dry_run,
        horizon_seconds,
        delta_threshold,
        horizons,
        confidence_thresholds,
        eras,
        window_size_seconds,
        policy_file,
        output_path,
    }))
}

pub(crate) fn usage() -> String {
    format!(
        "2EXCAMIM Runtime CLI\n\nUsage:\n  twoexcamim summary [--store PATH] [--json]\n  twoexcamim inspect <signal|decision|order|fill> <id> [--store PATH] [--json]\n  twoexcamim policy <signal|decision|order> <id> [--store PATH] [--json]\n  twoexcamim ingest research-signals <input.parquet> [--store PATH] [--dry-run] [--json]\n  twoexcamim confirm signals [--store PATH] [--policy-file PATH] [--json]\n  twoexcamim confirm-signals [--store PATH] [--policy-file PATH] [--json]\n  twoexcamim measure-confirmation-outcomes [--store PATH] [--snapshots PATH] [--horizon-seconds N] [--delta-threshold N] [--json]\n  twoexcamim evaluate-confirmation-policy [--store PATH] [--snapshots PATH] [--horizons CSV] [--confidence-thresholds CSV] [--delta-threshold N] [--json]\n  twoexcamim walkforward-confirmation-policy [--store PATH] [--snapshots PATH] [--eras N | --window-size SECONDS] [--horizons CSV] [--confidence-thresholds CSV] [--delta-threshold N] [--json]\n  twoexcamim propose-confirmation-policy [--store PATH] [--snapshots PATH] [--eras N | --window-size SECONDS] [--horizons CSV] [--confidence-thresholds CSV] [--delta-threshold N] [--output PATH] [--json]\n  twoexcamim materialize decisions [--store PATH] [--dry-run] [--json]\n  twoexcamim materialize orders [--store PATH] [--dry-run] [--json]\n  twoexcamim submit orders [--store PATH] [--dry-run] [--json]\n  twoexcamim observe fill --fill-id ID --order-id ID --side <buy|sell> --quantity N --price N --executed-at RFC3339 [--decision-id ID] [--instrument VALUE] [--venue VALUE] [--store PATH] [--dry-run] [--json]\n  twoexcamim run batch --research-signals <input.parquet> [--store PATH] [--dry-run] [--json]\n\nAliases:\n  twoexcamim signal <id>\n  twoexcamim decision <id>\n  twoexcamim order <id>\n  twoexcamim fill <id>\n\nExamples:\n  twoexcamim summary\n  twoexcamim inspect signal sig-1 --store ./var/events.jsonl\n  twoexcamim policy signal sig-1 --json\n  twoexcamim ingest research-signals research_prediction_markets/output/signals/latest_signals.parquet --dry-run\n  twoexcamim confirm-signals --store ./var/events.jsonl --policy-file ./policies/confirmation_policy.json\n  twoexcamim measure-confirmation-outcomes --store ./var/events.jsonl --snapshots ./var/market_snapshots.jsonl --json\n  twoexcamim evaluate-confirmation-policy --store ./var/events.jsonl --snapshots ./var/market_snapshots.jsonl --horizons 3600,7200 --confidence-thresholds 0.5,0.6,0.7 --json\n  twoexcamim walkforward-confirmation-policy --store ./var/events.jsonl --snapshots ./var/market_snapshots.jsonl --eras 4 --horizons 3600,7200 --confidence-thresholds 0.5,0.6,0.7 --json\n  twoexcamim propose-confirmation-policy --store ./var/events.jsonl --snapshots ./var/market_snapshots.jsonl --eras 4 --confidence-thresholds 0.5,0.6,0.7 --output ./policies/proposed_confirmation_policy.json\n  twoexcamim materialize decisions --dry-run --json\n  twoexcamim materialize orders --dry-run --json\n  twoexcamim submit orders --dry-run --json\n  twoexcamim observe fill --fill-id fill-1 --order-id ord-1 --side buy --quantity 1 --price 0.54 --executed-at 2026-04-07T00:00:00Z --dry-run\n  twoexcamim run batch --research-signals research_prediction_markets/output/signals/latest_signals.parquet --store ./var/events.jsonl --dry-run\n\nDefault store path: {DEFAULT_STORE_PATH}\nDefault snapshots path: {DEFAULT_SNAPSHOTS_PATH}"
    )
}

fn required_flag_value(
    args: &mut std::vec::IntoIter<String>,
    flag: &str,
) -> Result<String, RuntimeError> {
    args.next()
        .ok_or_else(|| RuntimeError::Usage(format!("missing value for {flag}\n\n{}", usage())))
}

fn required_value(value: Option<String>, flag: &str) -> Result<String, RuntimeError> {
    value.ok_or_else(|| RuntimeError::Usage(format!("missing value for {flag}\n\n{}", usage())))
}

fn parse_f64_flag(value: &str, flag: &str) -> Result<f64, RuntimeError> {
    value.parse::<f64>().map_err(|_| {
        RuntimeError::Usage(format!(
            "invalid numeric value for {flag}: {value}\n\n{}",
            usage()
        ))
    })
}

fn parse_csv_i64(value: &str, flag: &str) -> Result<Vec<i64>, RuntimeError> {
    let parsed = value
        .split(',')
        .filter(|item| !item.trim().is_empty())
        .map(|item| {
            item.trim().parse::<i64>().map_err(|_| {
                RuntimeError::Usage(format!(
                    "invalid numeric CSV for {flag}: {value}\n\n{}",
                    usage()
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if parsed.is_empty() {
        return Err(RuntimeError::Usage(format!(
            "empty CSV for {flag}\n\n{}",
            usage()
        )));
    }
    Ok(parsed)
}

fn parse_csv_f64(value: &str, flag: &str) -> Result<Vec<f64>, RuntimeError> {
    let parsed = value
        .split(',')
        .filter(|item| !item.trim().is_empty())
        .map(|item| {
            item.trim().parse::<f64>().map_err(|_| {
                RuntimeError::Usage(format!(
                    "invalid numeric CSV for {flag}: {value}\n\n{}",
                    usage()
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if parsed.is_empty() {
        return Err(RuntimeError::Usage(format!(
            "empty CSV for {flag}\n\n{}",
            usage()
        )));
    }
    Ok(parsed)
}

fn parse_timestamp_flag(value: &str, flag: &str) -> Result<DateTime<Utc>, RuntimeError> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| {
            RuntimeError::Usage(format!(
                "invalid RFC3339 timestamp for {flag}: {value}\n\n{}",
                usage()
            ))
        })
}

fn parse_fill_side(value: &str) -> Result<FillSide, RuntimeError> {
    match value.to_ascii_lowercase().as_str() {
        "buy" => Ok(FillSide::Buy),
        "sell" => Ok(FillSide::Sell),
        _ => Err(RuntimeError::Usage(format!(
            "invalid fill side: {value}\n\n{}",
            usage()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_args;
    use crate::runtime::{Command, ParseOutcome};

    #[test]
    fn parses_signal_alias() {
        let parsed =
            parse_args(vec!["twoexcamim".into(), "signal".into(), "sig-1".into()]).unwrap();
        let ParseOutcome::Config(config) = parsed else {
            panic!("expected config");
        };
        assert_eq!(
            config.command,
            Command::Signal {
                signal_id: "sig-1".into()
            }
        );
    }

    #[test]
    fn parses_walkforward_command_with_window_size() {
        let parsed = parse_args(vec![
            "twoexcamim".into(),
            "walkforward-confirmation-policy".into(),
            "--window-size".into(),
            "3600".into(),
        ])
        .unwrap();
        let ParseOutcome::Config(config) = parsed else {
            panic!("expected config");
        };
        assert_eq!(config.command, Command::WalkForwardConfirmationPolicy);
        assert_eq!(config.window_size_seconds, Some(3600));
    }

    #[test]
    fn parses_policy_file_for_confirm_signals() {
        let parsed = parse_args(vec![
            "twoexcamim".into(),
            "confirm-signals".into(),
            "--policy-file".into(),
            "policies/confirmation_policy.json".into(),
        ])
        .unwrap();
        let ParseOutcome::Config(config) = parsed else {
            panic!("expected config");
        };
        assert_eq!(config.command, Command::ConfirmSignals);
        assert_eq!(
            config.policy_file,
            Some(std::path::PathBuf::from(
                "policies/confirmation_policy.json"
            ))
        );
    }

    #[test]
    fn parses_output_for_proposal_command() {
        let parsed = parse_args(vec![
            "twoexcamim".into(),
            "propose-confirmation-policy".into(),
            "--output".into(),
            "policies/proposed_confirmation_policy.json".into(),
        ])
        .unwrap();
        let ParseOutcome::Config(config) = parsed else {
            panic!("expected config");
        };
        assert_eq!(config.command, Command::ProposeConfirmationPolicy);
        assert_eq!(
            config.output_path,
            Some(std::path::PathBuf::from(
                "policies/proposed_confirmation_policy.json"
            ))
        );
    }

    #[test]
    fn rejects_unknown_flag() {
        let error = parse_args(vec!["twoexcamim".into(), "--nope".into()]).unwrap_err();
        assert!(error.to_string().contains("unknown flag"));
    }
}
