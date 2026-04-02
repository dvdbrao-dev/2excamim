use core::fmt;

use serde_json::Error as SerdeJsonError;

use crate::events::{EventError, EventType};

#[derive(Debug)]
pub enum CodecError {
    UnknownEventType(EventType),
    PayloadDecode {
        event_type: EventType,
        source: SerdeJsonError,
    },
    Validation(EventError),
}

impl CodecError {
    pub fn unknown_event_type(event_type: EventType) -> Self {
        Self::UnknownEventType(event_type)
    }

    pub fn payload_decode(event_type: EventType, source: SerdeJsonError) -> Self {
        Self::PayloadDecode { event_type, source }
    }

    pub fn validation(source: EventError) -> Self {
        Self::Validation(source)
    }
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownEventType(event_type) => {
                write!(f, "unknown event type: {}", event_type.as_str())
            }
            Self::PayloadDecode { event_type, source } => {
                write!(
                    f,
                    "payload decode error for {}: {source}",
                    event_type.as_str()
                )
            }
            Self::Validation(source) => write!(f, "{source}"),
        }
    }
}

impl std::error::Error for CodecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::PayloadDecode { source, .. } => Some(source),
            Self::Validation(source) => Some(source),
            Self::UnknownEventType(_) => None,
        }
    }
}
