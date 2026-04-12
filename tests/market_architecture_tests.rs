use std::{
    fs,
    path::{Path, PathBuf},
};

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn read_file(relative: &str) -> String {
    fs::read_to_string(repo_path(relative)).unwrap()
}

fn rust_sources_under(relative_dir: &str) -> Vec<(PathBuf, String)> {
    let root = repo_path(relative_dir);
    let mut paths = Vec::new();
    collect_rust_sources(&root, &mut paths);
    paths.sort_by(|a, b| a.0.cmp(&b.0));
    paths
}

fn collect_rust_sources(dir: &Path, out: &mut Vec<(PathBuf, String)>) {
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.is_dir() {
            collect_rust_sources(&path, out);
            continue;
        }

        if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push((path.clone(), fs::read_to_string(path).unwrap()));
        }
    }
}

#[test]
fn canonical_domain_sources_do_not_reference_provider_dtos_or_adapters() {
    let cargo_toml = read_file("crates/market-domain/Cargo.toml");
    assert!(
        !cargo_toml.contains("market-ingestion"),
        "market-domain must not depend on market-ingestion",
    );
    assert!(
        !cargo_toml.contains("ureq"),
        "market-domain must not depend on HTTP clients",
    );

    let forbidden = [
        "dto::polymarket",
        "adapters::polymarket",
        "PolymarketMarketDto",
        "PolymarketActivityDto",
        "PolymarketHttpAdapter",
        "UreqHttpClient",
        "market_ingestion",
    ];

    for (path, source) in rust_sources_under("crates/market-domain/src") {
        for needle in forbidden {
            assert!(
                !source.contains(needle),
                "{} must stay provider-agnostic; found forbidden reference `{needle}`",
                path.display()
            );
        }
    }
}

#[test]
fn market_watch_agent_does_not_directly_import_provider_modules() {
    let cargo_toml = read_file("agents/market-watch/Cargo.toml");
    assert!(
        !cargo_toml.contains("ureq"),
        "market-watch must not depend on an HTTP client directly",
    );

    let source = read_file("agents/market-watch/src/main.rs");
    let forbidden = [
        "dto::polymarket",
        "adapters::polymarket",
        "PolymarketHttpAdapter",
        "UreqHttpClient",
        "PolymarketActivityService",
        "PolymarketSnapshotRefreshService",
    ];

    for needle in forbidden {
        assert!(
            !source.contains(needle),
            "market-watch must stay behind the application runner boundary; found `{needle}`",
        );
    }
}

#[test]
fn market_signals_stays_isolated_from_ingestion_crate() {
    let cargo_toml = read_file("crates/market-signals/Cargo.toml");
    assert!(
        !cargo_toml.contains("market-ingestion"),
        "market-signals must not depend on market-ingestion",
    );

    for (path, source) in rust_sources_under("crates/market-signals/src") {
        assert!(
            !source.contains("market_ingestion"),
            "{} must not import market_ingestion",
            path.display()
        );
    }
}
