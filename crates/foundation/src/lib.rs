#![deny(clippy::unwrap_used)]
//! Stable identifiers and timetable coordinates.
use serde::{Deserialize, Serialize};
macro_rules! id { ($($name:ident),+) => { $(
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
    #[serde(transparent)] pub struct $name(pub u64);
)+ }; }
id!(TeacherId, ClassId, RoomId, LessonId, CardId, SubjectId);
/// Zero-based coordinates, checked against the document's week shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TimeSlot {
    pub day: u8,
    pub period: u8,
}
/// Slot index is day * periods_per_day + period, limited to 64 slots.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Availability {
    pub blocked: u64,
    pub undesirable: u64,
}
