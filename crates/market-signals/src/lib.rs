//! Deterministic prediction-market signal detectors over canonical market history.

mod activity_spike;
mod detector;
mod odds_jump;

pub use activity_spike::{ActivitySpikeDetector, ActivitySpikeDetectorConfig};
pub use detector::DetectorError;
pub use odds_jump::{OddsJumpDetector, OddsJumpDetectorConfig};
