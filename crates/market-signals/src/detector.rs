/// Error returned by deterministic market signal detectors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectorError {
    InvalidConfig(String),
    InvalidInput(String),
}

impl DetectorError {
    /// Creates an invalid-config error.
    pub fn invalid_config(message: impl Into<String>) -> Self {
        Self::InvalidConfig(message.into())
    }

    /// Creates an invalid-input error.
    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::InvalidInput(message.into())
    }
}

impl core::fmt::Display for DetectorError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidConfig(message) => write!(f, "detector config error: {message}"),
            Self::InvalidInput(message) => write!(f, "detector input error: {message}"),
        }
    }
}

impl std::error::Error for DetectorError {}
