use crate::{codecs::RehydratedEvent, events::EventType, events::Linkage, store::StoredEvent};

pub trait TimelineEvent {
    fn event_type(&self) -> EventType;
    fn linkage(&self) -> &Linkage;
}

impl TimelineEvent for StoredEvent {
    fn event_type(&self) -> EventType {
        self.event_type
    }

    fn linkage(&self) -> &Linkage {
        &self.linkage
    }
}

impl TimelineEvent for RehydratedEvent {
    fn event_type(&self) -> EventType {
        self.event_type()
    }

    fn linkage(&self) -> &Linkage {
        match self {
            Self::HypothesisGenerated(event) => &event.linkage,
            Self::SignalGenerated(event) => &event.linkage,
            Self::SignalConfirmed(event) => &event.linkage,
            Self::VetoRaised(event) => &event.linkage,
            Self::DecisionFormed(event) => &event.linkage,
            Self::OrderRegistered(event) => &event.linkage,
            Self::OrderSubmitted(event) => &event.linkage,
            Self::FillReceived(event) => &event.linkage,
        }
    }
}

pub fn timeline<E>(events: &[E]) -> Vec<&E>
where
    E: TimelineEvent,
{
    events.iter().collect()
}

pub fn timeline_for_correlation_id<'a, E>(events: &'a [E], correlation_id: &str) -> Vec<&'a E>
where
    E: TimelineEvent,
{
    events
        .iter()
        .filter(|event| event.linkage().correlation_id.as_deref() == Some(correlation_id))
        .collect()
}

pub fn timeline_for_signal_id<'a, E>(events: &'a [E], signal_id: &str) -> Vec<&'a E>
where
    E: TimelineEvent,
{
    events
        .iter()
        .filter(|event| event.linkage().signal_id.as_deref() == Some(signal_id))
        .collect()
}

pub fn timeline_for_decision_id<'a, E>(events: &'a [E], decision_id: &str) -> Vec<&'a E>
where
    E: TimelineEvent,
{
    events
        .iter()
        .filter(|event| event.linkage().decision_id.as_deref() == Some(decision_id))
        .collect()
}
