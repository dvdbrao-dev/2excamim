mod decision_projection;
mod error;
mod signal_projection;
mod timeline;

pub use decision_projection::{build_decision_projections, DecisionProjection};
pub use error::ProjectionError;
pub use signal_projection::{build_signal_projections, SignalProjection};
pub use timeline::{
    timeline, timeline_for_correlation_id, timeline_for_decision_id, timeline_for_signal_id,
    TimelineEvent,
};
