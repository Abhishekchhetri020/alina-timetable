#![deny(clippy::unwrap_used)]
//! Serializable data; application mutations belong to engine commands.
use alina_foundation::{
    Availability, CardId, ClassId, LessonId, RoomId, SubjectId, TeacherId, TimeSlot,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Teacher {
    pub id: TeacherId,
    pub name: String,
    pub availability: Availability,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Class {
    pub id: ClassId,
    pub name: String,
    pub size: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Room {
    pub id: RoomId,
    pub name: String,
    pub capacity: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Subject {
    pub id: SubjectId,
    pub name: String,
}
/// A lesson is a requirement; cards are its individual occurrences.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Lesson {
    pub id: LessonId,
    pub subject: SubjectId,
    pub teachers: Vec<TeacherId>,
    pub classes: Vec<ClassId>,
    pub duration: u8,
    pub occurrences: u16,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Placement {
    pub card: CardId,
    pub lesson: LessonId,
    pub start: TimeSlot,
    pub room: Option<RoomId>,
}
/// Bootstrap schema, not yet a stable interchange format.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimetableDoc {
    pub schema_version: u32,
    pub days: u8,
    pub periods_per_day: u8,
    pub teachers: BTreeMap<TeacherId, Teacher>,
    pub classes: BTreeMap<ClassId, Class>,
    pub rooms: BTreeMap<RoomId, Room>,
    pub subjects: BTreeMap<SubjectId, Subject>,
    pub lessons: BTreeMap<LessonId, Lesson>,
    pub placements: Arc<BTreeMap<CardId, Placement>>,
    pub locked_slots: BTreeSet<TimeSlot>,
}
impl Default for TimetableDoc {
    fn default() -> Self {
        Self {
            schema_version: 1,
            days: 5,
            periods_per_day: 6,
            teachers: BTreeMap::new(),
            classes: BTreeMap::new(),
            rooms: BTreeMap::new(),
            subjects: BTreeMap::new(),
            lessons: BTreeMap::new(),
            placements: Arc::new(BTreeMap::new()),
            locked_slots: BTreeSet::new(),
        }
    }
}
