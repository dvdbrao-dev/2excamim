mod paper_decision_runner;
mod paper_ledger;
mod paper_risk_guard;
mod polymarket_paper_adapter;

pub use paper_decision_runner::{
    run_paper_decisions, PaperDecisionRunConfig, PaperDecisionRunDisposition, PaperDecisionRunItem,
    PaperDecisionRunReport, PaperDecisionRunnerError,
};
pub use paper_ledger::{
    project_paper_ledger, PaperExposureView, PaperFillView, PaperLedgerProjection,
    PaperLedgerSummary, PaperOrderStatus, PaperOrderView, PaperPositionLifecycle,
    PaperPositionView,
};
pub use paper_risk_guard::{
    evaluate_paper_risk, PaperRiskGuardConfig, PaperRiskGuardDecision, PaperRiskGuardOutcome,
};
pub use polymarket_paper_adapter::{
    submit_paper_order_and_map_fill, CliPolymarketPaperBackend, FixturePolymarketPaperBackend,
    PaperExecutionImportDisposition, PaperExecutionImportReport, PaperExecutionRequest,
    PaperExecutionResult, PolymarketPaperAdapterError, PolymarketPaperBackend,
    PolymarketPaperTrade, DEFAULT_POLYMARKET_PAPER_DATA_DIR, POLYMARKET_PAPER_VENUE,
};
