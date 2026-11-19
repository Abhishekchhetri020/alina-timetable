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
/// A class may be split into disjoint groups within a division. Different
/// divisions may overlap and therefore conflict conservatively.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClassScope {
    Whole,
    Groups {
        division: u32,
        groups: BTreeSet<u32>,
    },
}
/// A lesson is a requirement; cards are its individual occurrences.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Lesson {
    pub id: LessonId,
    pub subject: SubjectId,
    pub teachers: Vec<TeacherId>,
    pub classes: Vec<ClassId>,
    /// Missing entries mean the whole class. Group capacity is not yet modeled.
    #[serde(default)]
    pub class_scopes: BTreeMap<ClassId, ClassScope>,
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
/// Optional hard limits. Absence means unrestricted.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HardLimits {
    pub teacher_periods_per_day: BTreeMap<TeacherId, u8>,
    pub class_periods_per_day: BTreeMap<ClassId, u8>,
    pub subject_periods_per_day: BTreeMap<SubjectId, u8>,
    /// Bit per allowed day. Missing entries permit every day.
    pub lesson_allowed_days: BTreeMap<LessonId, u64>,
    pub lesson_fixed_start: BTreeMap<LessonId, TimeSlot>,
    /// Absolute weekly slot masks: set means unavailable.
    #[serde(default)]
    pub class_blocked_slots: BTreeMap<ClassId, u64>,
    #[serde(default)]
    pub room_blocked_slots: BTreeMap<RoomId, u64>,
    /// Period-of-day masks (bit zero is the first period).
    #[serde(default)]
    pub lesson_allowed_starts: BTreeMap<LessonId, u64>,
    #[serde(default)]
    pub subject_banned_starts: BTreeMap<SubjectId, u64>,
    #[serde(default)]
    pub subject_banned_ends: BTreeMap<SubjectId, u64>,
    /// Only listed rooms are allowed for this lesson; an empty list permits none.
    #[serde(default)]
    pub lesson_allowed_rooms: BTreeMap<LessonId, BTreeSet<RoomId>>,
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
    #[serde(default)]
    pub hard_limits: HardLimits,
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
            hard_limits: HardLimits::default(),
        }
    }
}
