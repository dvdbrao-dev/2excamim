pub mod confirmation_agent;
pub mod confirmation_comparison;
pub mod confirmation_eras;
pub mod confirmation_outcome_runner;
pub mod confirmation_outcomes;
pub mod confirmation_policy;
pub mod confirmation_policy_advisory;
pub mod confirmation_policy_proposal;
pub mod confirmation_policy_sweep;
pub mod confirmation_runner;
pub mod confirmation_scorecard;
pub mod confirmation_walkforward;

pub use confirmation_agent::{
    ConfirmationAgent, ConfirmationAgentConfig, ConfirmationDecision, CoreEvent,
};
pub use confirmation_comparison::{
    compare_confirmation_quality, load_confirmed_signal_ids_from_store,
    load_generated_signals_from_store, ComparisonBreakdownRow, ConfirmationComparisonReport,
    GeneratedSignalContext,
};
pub use confirmation_eras::{
    split_generated_signals_into_eras, ConfirmationEra, ConfirmationEraSplit, ConfirmationEraWindow,
};
pub use confirmation_outcome_runner::{ConfirmationOutcomeRunReport, ConfirmationOutcomeRunner};
pub use confirmation_outcomes::{
    evaluate_confirmation_outcome, evaluate_outcome, ConfirmationOutcomeConfig,
    ConfirmationOutcomeLabel, ConfirmationOutcomeRecord, ConfirmationOutcomeScorecard,
    ConfirmedSignalContext, DirectionOutcomeStats, OutcomeSignalContext,
};
pub use confirmation_policy::{
    ConfirmationPolicy, ConfirmationPolicyLoadError, ConfirmationPolicyMetadata, SignalPolicyRule,
    SignalPolicyStatus,
};
pub use confirmation_policy_advisory::{
    advise_signal_families, AdvisorySignalFamilyMetrics, ConfirmationPolicyAdvisory,
    ConfirmationPolicyAdvisoryClassification, ConfirmationPolicyAdvisoryConfig,
};
pub use confirmation_policy_proposal::{
    propose_confirmation_policy, write_confirmation_policy_proposal, ConfirmationPolicyProposal,
    ConfirmationPolicyProposalConfig, ConfirmationPolicyProposalSummary,
};
pub use confirmation_policy_sweep::{
    sweep_confirmation_policy, PolicySweepBreakdownRow, PolicySweepResultRow, PolicySweepSummary,
};
pub use confirmation_runner::{
    ConfirmationDisposition, ConfirmationRunItem, ConfirmationRunReport, ConfirmationRunner,
};
pub use confirmation_scorecard::{ConfirmationDirectionStats, ConfirmationScorecard};
pub use confirmation_walkforward::{
    load_snapshots_jsonl, run_confirmation_walkforward, run_confirmation_walkforward_from_store,
    AdvisoryStabilityRow, ConfirmationWalkForwardConfig, ConfirmationWalkForwardReport,
    ConfirmationWalkForwardStep, ConfirmationWalkForwardSummary, PolicyChoiceFrequency,
    WalkForwardPolicyChoice,
};
