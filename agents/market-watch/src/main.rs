use std::path::PathBuf;

use market_ingestion::{MarketWatchConfig, MarketWatchRunner};

const DEFAULT_BASE_URL: &str = "https://gamma-api.polymarket.com";
const DEFAULT_STATE_DIR: &str = "./var/market-watch";

fn parse_args() -> Result<MarketWatchConfig, String> {
    let mut base_url = DEFAULT_BASE_URL.to_string();
    let mut state_dir = PathBuf::from(DEFAULT_STATE_DIR);
    let mut with_activity = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--base-url" => {
                base_url = args
                    .next()
                    .ok_or_else(|| "--base-url requires a value".to_string())?;
            }
            "--state-dir" => {
                state_dir = PathBuf::from(
                    args.next()
                        .ok_or_else(|| "--state-dir requires a value".to_string())?,
                );
            }
            "--with-activity" => with_activity = true,
            "--help" | "-h" => {
                println!(
                    "usage: market-watch [--base-url URL] [--state-dir PATH] [--with-activity]"
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    Ok(MarketWatchConfig {
        base_url,
        state_dir,
        with_activity,
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config =
        parse_args().map_err(|message| format!("market-watch argument error: {message}"))?;
    let summary = MarketWatchRunner::new(config)
        .run()
        .map_err(|message| format!("market-watch observation failed: {message}"))?;

    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}
