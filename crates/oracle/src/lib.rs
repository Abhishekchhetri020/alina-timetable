#![deny(clippy::unwrap_used)]
//! Independent deterministic checks for the supported document subset.
//! This is not yet a complete Chronexa constraint oracle.
use alina_doc::{CardRelationKind, ClassScope, Placement, TimetableDoc};
use alina_foundation::{CardId, ClassId, RoomId, TeacherId, TimeSlot};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Violation {
    InvalidWeek,
    CardRelation {
        index: usize,
        a: CardId,
        b: CardId,
    },
    InvalidAvailability {
        teacher: TeacherId,
    },
    OccurrenceOverflow {
        lesson: alina_foundation::LessonId,
        actual: usize,
        limit: u16,
    },
    LessonStartNotAllowed {
        card: CardId,
    },
    SubjectStartBanned {
        card: CardId,
    },
    SubjectEndBanned {
        card: CardId,
    },
    RoomNotAllowed {
        card: CardId,
        room: Option<RoomId>,
    },
    ClassUnavailable {
        card: CardId,
        class: ClassId,
        slot: TimeSlot,
    },
    RoomUnavailable {
        card: CardId,
        room: RoomId,
        slot: TimeSlot,
    },
    DayNotAllowed {
        card: CardId,
    },
    FixedStartMismatch {
        card: CardId,
    },
    TeacherDailyLimit {
        teacher: TeacherId,
        day: u8,
        actual: usize,
        limit: u8,
    },
    ClassDailyLimit {
        class: ClassId,
        day: u8,
        actual: usize,
        limit: u8,
    },
    SubjectDailyLimit {
        class: ClassId,
        subject: alina_foundation::SubjectId,
        day: u8,
        actual: usize,
        limit: u8,
    },
    SubjectParallelLimit {
        subject: alina_foundation::SubjectId,
        slot: TimeSlot,
        actual: usize,
        limit: u16,
    },
    InvalidPlacement {
        card: CardId,
    },
    MissingSubject {
        card: CardId,
    },
    MissingLesson {
        card: CardId,
    },
    MissingTeacher {
        card: CardId,
        teacher: TeacherId,
    },
    MissingClass {
        card: CardId,
        class: ClassId,
    },
    MissingRoom {
        card: CardId,
        room: RoomId,
    },
    TeacherUnavailable {
        card: CardId,
        teacher: TeacherId,
        slot: TimeSlot,
    },
    TeacherClash {
        teacher: TeacherId,
        slot: TimeSlot,
        cards: Vec<CardId>,
    },
    ClassClash {
        class: ClassId,
        slot: TimeSlot,
        cards: Vec<CardId>,
    },
    RoomClash {
        room: RoomId,
        slot: TimeSlot,
        cards: Vec<CardId>,
    },
    GroupCapacityUnknown {
        card: CardId,
    },
    RoomCapacity {
        card: CardId,
        room: RoomId,
        required: u64,
        capacity: u32,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub placed: usize,
    /// Number of required occurrences, independent of placement validity.
    pub required: usize,
    /// Missing occurrences by lesson count. This does not identify card IDs.
    pub unplaced: usize,
    pub violations: Vec<Violation>,
}

/// Unweighted quality indicators. These are separate from hard violations;
/// applications may choose their own weights without changing feasibility.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SoftIndicators {
    pub teacher_gap_periods: usize,
    pub teacher_undesirable_periods: usize,
}

/// Count teacher windows and occupied undesirable slots deterministically.
/// Only valid, in-bounds lesson spans contribute to the indicators.
pub fn soft_indicators(doc: &TimetableDoc) -> SoftIndicators {
    use std::collections::BTreeSet;
    let mut occupied: BTreeMap<(TeacherId, u8), BTreeSet<u8>> = BTreeMap::new();
    if doc.days == 0
        || doc.periods_per_day == 0
        || u16::from(doc.days) * u16::from(doc.periods_per_day) > 64
    {
        return SoftIndicators::default();
    }
    for placement in doc.placements.values() {
        let Some(lesson) = doc.lessons.get(&placement.lesson) else {
            continue;
        };
        let end = u16::from(placement.start.period) + u16::from(lesson.duration);
        if lesson.duration == 0
            || placement.start.day >= doc.days
            || end > u16::from(doc.periods_per_day)
        {
            continue;
        }
        for teacher in &lesson.teachers {
            if doc.teachers.contains_key(teacher) {
                for period in placement.start.period..end as u8 {
                    occupied
                        .entry((*teacher, placement.start.day))
                        .or_default()
                        .insert(period);
                }
            }
        }
    }
    let mut result = SoftIndicators::default();
    for ((teacher, day), periods) in occupied {
        if let (Some(first), Some(last)) = (periods.first(), periods.last()) {
            result.teacher_gap_periods += usize::from(last - first) + 1 - periods.len();
        }
        if let Some(data) = doc.teachers.get(&teacher) {
            for period in periods {
                let bit = u32::from(day) * u32::from(doc.periods_per_day) + u32::from(period);
                result.teacher_undesirable_periods +=
                    usize::from(data.availability.undesirable & (1u64 << bit) != 0);
            }
        }
    }
    result
}

fn scopes_conflict(a: &ClassScope, b: &ClassScope) -> bool {
    match (a, b) {
        (
            ClassScope::Groups {
                division: ad,
                groups: ag,
            },
            ClassScope::Groups {
                division: bd,
                groups: bg,
            },
        ) => {
            // An empty group set is invalid/unknown: fail conservatively.
            ag.is_empty() || bg.is_empty() || ad != bd || !ag.is_disjoint(bg)
        }
        _ => true,
    }
}

/// Preview a candidate without changing the document. The full resulting report
/// is returned, including existing conflicts, so callers must not mistake it
/// for a delta or an authorization to place a card.
pub fn preview(doc: &TimetableDoc, candidate: Placement) -> Report {
    let mut copy = doc.clone();
    std::sync::Arc::make_mut(&mut copy.placements).insert(candidate.card, candidate);
    verify(&copy)
}

/// Difference between the current timetable and the proposed placement.
/// Existing violations are not presented as consequences of the move.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Forecast {
    pub added: Vec<Violation>,
    pub resolved: Vec<Violation>,
    pub resulting: Report,
}

/// Forecast a placement without mutating the document. Differences compare
/// complete violation values, including affected cards and counts; a changed
/// count therefore appears as one resolved and one added violation.
pub fn forecast(doc: &TimetableDoc, candidate: Placement) -> Forecast {
    let before = verify(doc);
    let resulting = preview(doc, candidate);
    // Match as multisets rather than sets in case two distinct checks have
    // identical reports. Preserve the deterministic oracle order.
    let mut matched = vec![false; resulting.violations.len()];
    let mut resolved = Vec::new();
    for violation in &before.violations {
        if let Some(index) = resulting
            .violations
            .iter()
            .enumerate()
            .position(|(i, v)| !matched[i] && v == violation)
        {
            matched[index] = true;
        } else {
            resolved.push(violation.clone());
        }
    }
    let added = resulting
        .violations
        .iter()
        .enumerate()
        .filter(|(i, _)| !matched[*i])
        .map(|(_, v)| v.clone())
        .collect();
    Forecast {
        added,
        resolved,
        resulting,
    }
}

/// Checks the supported constraints in stable ID and slot order. Counts are
/// placements only; an exact unplaced count requires explicit card identities.
pub fn verify(doc: &TimetableDoc) -> Report {
    let mut violations = Vec::new();
    let mut teachers: BTreeMap<(TeacherId, TimeSlot), Vec<CardId>> = BTreeMap::new();
    let mut classes: BTreeMap<(ClassId, TimeSlot), Vec<(CardId, ClassScope)>> = BTreeMap::new();
    let mut subject_slots: BTreeMap<
        (ClassId, alina_foundation::SubjectId, u8),
        std::collections::BTreeSet<u8>,
    > = BTreeMap::new();
    let mut rooms: BTreeMap<(RoomId, TimeSlot), Vec<CardId>> = BTreeMap::new();
    let mut subject_parallel: BTreeMap<(alina_foundation::SubjectId, TimeSlot), usize> =
        BTreeMap::new();
    let week_ok = doc.days > 0
        && doc.periods_per_day > 0
        && u16::from(doc.days) * u16::from(doc.periods_per_day) <= 64;
    if !week_ok {
        violations.push(Violation::InvalidWeek);
    }
    for (&teacher, data) in &doc.teachers {
        if data.availability.blocked & data.availability.undesirable != 0 {
            violations.push(Violation::InvalidAvailability { teacher });
        }
    }
    let mut lesson_counts = BTreeMap::new();
    for placement in doc.placements.values() {
        *lesson_counts.entry(placement.lesson).or_insert(0usize) += 1;
    }
    let required = doc
        .lessons
        .values()
        .map(|l| usize::from(l.occurrences))
        .sum();
    let unplaced = doc
        .lessons
        .iter()
        .map(|(id, lesson)| {
            usize::from(lesson.occurrences).saturating_sub(*lesson_counts.get(id).unwrap_or(&0))
        })
        .sum();
    for (lesson, actual) in lesson_counts {
        if let Some(def) = doc.lessons.get(&lesson) {
            if actual > usize::from(def.occurrences) {
                violations.push(Violation::OccurrenceOverflow {
                    lesson,
                    actual,
                    limit: def.occurrences,
                });
            }
        }
    }
    for (&key, placement) in doc.placements.iter() {
        if key != placement.card {
            violations.push(Violation::InvalidPlacement { card: key });
        }
        let Some(lesson) = doc.lessons.get(&placement.lesson) else {
            violations.push(Violation::MissingLesson { card: key });
            continue;
        };
        if !doc.subjects.contains_key(&lesson.subject) {
            violations.push(Violation::MissingSubject { card: key });
        }
        let end = u16::from(placement.start.period) + u16::from(lesson.duration);
        if !week_ok
            || lesson.duration == 0
            || placement.start.day >= doc.days
            || end > u16::from(doc.periods_per_day)
        {
            violations.push(Violation::InvalidPlacement { card: key });
            continue;
        }
        if let Some(mask) = doc.hard_limits.lesson_allowed_days.get(&placement.lesson) {
            if mask & (1u64 << placement.start.day) == 0 {
                violations.push(Violation::DayNotAllowed { card: key });
            }
        }
        if let Some(fixed) = doc.hard_limits.lesson_fixed_start.get(&placement.lesson) {
            if *fixed != placement.start {
                violations.push(Violation::FixedStartMismatch { card: key });
            }
        }
        if doc
            .hard_limits
            .card_fixed_start
            .get(&key)
            .is_some_and(|fixed| *fixed != placement.start)
        {
            violations.push(Violation::FixedStartMismatch { card: key });
        }
        let start_bit = 1u64 << placement.start.period;
        let end_bit = 1u64 << (end - 1);
        if doc
            .hard_limits
            .lesson_allowed_starts
            .get(&placement.lesson)
            .is_some_and(|mask| mask & start_bit == 0)
        {
            violations.push(Violation::LessonStartNotAllowed { card: key });
        }
        if doc
            .hard_limits
            .subject_banned_starts
            .get(&lesson.subject)
            .is_some_and(|mask| mask & start_bit != 0)
        {
            violations.push(Violation::SubjectStartBanned { card: key });
        }
        if doc
            .hard_limits
            .subject_banned_ends
            .get(&lesson.subject)
            .is_some_and(|mask| mask & end_bit != 0)
        {
            violations.push(Violation::SubjectEndBanned { card: key });
        }
        if doc
            .hard_limits
            .lesson_allowed_rooms
            .get(&placement.lesson)
            .is_some_and(|allowed| placement.room.is_none_or(|room| !allowed.contains(&room)))
        {
            violations.push(Violation::RoomNotAllowed {
                card: key,
                room: placement.room,
            });
        }
        let mut size = 0u64;
        for class in &lesson.classes {
            match doc.classes.get(class) {
                Some(c) => size += u64::from(c.size),
                None => violations.push(Violation::MissingClass {
                    card: key,
                    class: *class,
                }),
            }
        }
        for teacher in &lesson.teachers {
            if !doc.teachers.contains_key(teacher) {
                violations.push(Violation::MissingTeacher {
                    card: key,
                    teacher: *teacher,
                });
            }
        }
        let has_groups = lesson
            .classes
            .iter()
            .any(|c| matches!(lesson.class_scopes.get(c), Some(ClassScope::Groups { .. })));
        let required_size = lesson.student_count.map_or(size, u64::from);
        if has_groups && lesson.student_count.is_none() && placement.room.is_some() {
            violations.push(Violation::GroupCapacityUnknown { card: key });
        }
        if let Some(room) = placement.room {
            match doc.rooms.get(&room) {
                None => violations.push(Violation::MissingRoom { card: key, room }),
                Some(r)
                    if (!has_groups || lesson.student_count.is_some())
                        && required_size > u64::from(r.capacity) =>
                {
                    violations.push(Violation::RoomCapacity {
                        card: key,
                        room,
                        required: required_size,
                        capacity: r.capacity,
                    });
                }
                _ => {}
            }
        }
        for period in placement.start.period..(end as u8) {
            let slot = TimeSlot {
                day: placement.start.day,
                period,
            };
            let bit = u32::from(slot.day) * u32::from(doc.periods_per_day) + u32::from(slot.period);
            *subject_parallel.entry((lesson.subject, slot)).or_default() += 1;
            for teacher in &lesson.teachers {
                if let Some(t) = doc.teachers.get(teacher) {
                    teachers.entry((*teacher, slot)).or_default().push(key);
                    if t.availability.blocked & (1u64 << bit) != 0 {
                        violations.push(Violation::TeacherUnavailable {
                            card: key,
                            teacher: *teacher,
                            slot,
                        });
                    }
                }
            }
            for class in &lesson.classes {
                if doc.classes.contains_key(class) {
                    if doc
                        .hard_limits
                        .class_blocked_slots
                        .get(class)
                        .is_some_and(|mask| mask & (1u64 << bit) != 0)
                    {
                        violations.push(Violation::ClassUnavailable {
                            card: key,
                            class: *class,
                            slot,
                        });
                    }
                    subject_slots
                        .entry((*class, lesson.subject, slot.day))
                        .or_default()
                        .insert(slot.period);
                    classes.entry((*class, slot)).or_default().push((
                        key,
                        lesson
                            .class_scopes
                            .get(class)
                            .cloned()
                            .unwrap_or(ClassScope::Whole),
                    ));
                }
            }
            if let Some(room) = placement.room {
                if doc.rooms.contains_key(&room) {
                    if doc
                        .hard_limits
                        .room_blocked_slots
                        .get(&room)
                        .is_some_and(|mask| mask & (1u64 << bit) != 0)
                    {
                        violations.push(Violation::RoomUnavailable {
                            card: key,
                            room,
                            slot,
                        });
                    }
                    rooms.entry((room, slot)).or_default().push(key);
                }
            }
        }
    }
    for ((subject, slot), actual) in subject_parallel {
        if let Some(&limit) = doc.hard_limits.subject_max_parallel.get(&subject) {
            if actual > usize::from(limit) {
                violations.push(Violation::SubjectParallelLimit {
                    subject,
                    slot,
                    actual,
                    limit,
                });
            }
        }
    }
    for (index, relation) in doc.hard_limits.card_relations.iter().enumerate() {
        let (Some(a), Some(b)) = (
            doc.placements.get(&relation.a),
            doc.placements.get(&relation.b),
        ) else {
            continue;
        };
        let (Some(al), Some(bl)) = (doc.lessons.get(&a.lesson), doc.lessons.get(&b.lesson)) else {
            continue;
        };
        let a_end = u16::from(a.start.period) + u16::from(al.duration);
        let b_end = u16::from(b.start.period) + u16::from(bl.duration);
        let same_day = a.start.day == b.start.day;
        let bad = match relation.kind {
            CardRelationKind::DifferentDays => same_day,
            CardRelationKind::NoOverlap => {
                same_day && u16::from(a.start.period) < b_end && u16::from(b.start.period) < a_end
            }
            CardRelationKind::NotAdjacent => {
                same_day
                    && (a_end == u16::from(b.start.period) || b_end == u16::from(a.start.period))
            }
            CardRelationKind::SameDay => !same_day,
            CardRelationKind::SameStart => a.start != b.start,
            CardRelationKind::Adjacent => {
                !same_day
                    || (a_end != u16::from(b.start.period) && b_end != u16::from(a.start.period))
            }
            CardRelationKind::OrderedAdjacent => !same_day || a_end != u16::from(b.start.period),
            CardRelationKind::SameDayOrdered => !same_day || a_end > u16::from(b.start.period),
        };
        if bad {
            violations.push(Violation::CardRelation {
                index,
                a: relation.a,
                b: relation.b,
            });
        }
    }
    let mut teacher_days: BTreeMap<(TeacherId, u8), usize> = BTreeMap::new();
    for (teacher, slot) in teachers.keys() {
        *teacher_days.entry((*teacher, slot.day)).or_default() += 1;
    }
    for ((teacher, day), actual) in teacher_days {
        if let Some(&limit) = doc.hard_limits.teacher_periods_per_day.get(&teacher) {
            if actual > usize::from(limit) {
                violations.push(Violation::TeacherDailyLimit {
                    teacher,
                    day,
                    actual,
                    limit,
                });
            }
        }
    }
    let mut class_days: BTreeMap<(ClassId, u8), usize> = BTreeMap::new();
    for (class, slot) in classes.keys() {
        *class_days.entry((*class, slot.day)).or_default() += 1;
    }
    for ((class, day), actual) in class_days {
        if let Some(&limit) = doc.hard_limits.class_periods_per_day.get(&class) {
            if actual > usize::from(limit) {
                violations.push(Violation::ClassDailyLimit {
                    class,
                    day,
                    actual,
                    limit,
                });
            }
        }
    }
    for ((class, subject, day), periods) in subject_slots {
        let limit = match doc
            .hard_limits
            .class_subject_periods_per_day
            .get(&class)
            .and_then(|m| m.get(&subject))
        {
            Some(value) => *value,
            None => doc
                .hard_limits
                .subject_periods_per_day
                .get(&subject)
                .copied(),
        };
        if let Some(limit) = limit {
            if periods.len() > usize::from(limit) {
                violations.push(Violation::SubjectDailyLimit {
                    class,
                    subject,
                    day,
                    actual: periods.len(),
                    limit,
                });
            }
        }
    }
    for ((teacher, slot), mut cards) in teachers {
        cards.sort();
        cards.dedup();
        if cards.len() > 1 {
            violations.push(Violation::TeacherClash {
                teacher,
                slot,
                cards,
            });
        }
    }
    for ((class, slot), entries) in classes {
        // Pairwise reporting avoids incorrectly identifying disjoint groups as
        // conflicting members of a larger simultaneous slot.
        for i in 0..entries.len() {
            for j in (i + 1)..entries.len() {
                if entries[i].0 != entries[j].0 && scopes_conflict(&entries[i].1, &entries[j].1) {
                    violations.push(Violation::ClassClash {
                        class,
                        slot,
                        cards: vec![entries[i].0, entries[j].0],
                    });
                }
            }
        }
    }
    for ((room, slot), mut cards) in rooms {
        cards.sort();
        cards.dedup();
        if cards.len() > 1 {
            violations.push(Violation::RoomClash { room, slot, cards });
        }
    }
    Report {
        placed: doc.placements.len(),
        required,
        unplaced,
        violations,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use alina_doc::{Class, Lesson, Placement, Room, Subject, Teacher};
    use alina_foundation::{Availability, LessonId, SubjectId};
    use std::sync::Arc;
    #[test]
    fn soft_indicators_count_occupied_periods_and_gaps() {
        let mut d = TimetableDoc::default();
        d.teachers.insert(
            TeacherId(1),
            Teacher {
                id: TeacherId(1),
                name: "T".into(),
                availability: alina_foundation::Availability {
                    blocked: 0,
                    undesirable: 1 << 3,
                },
            },
        );
        d.lessons.insert(
            LessonId(1),
            Lesson {
                id: LessonId(1),
                subject: SubjectId(1),
                teachers: vec![TeacherId(1)],
                classes: vec![],
                class_scopes: BTreeMap::new(),
                student_count: None,
                duration: 1,
                occurrences: 2,
            },
        );
        for (id, period) in [(1, 0), (2, 3)] {
            Arc::make_mut(&mut d.placements).insert(
                CardId(id),
                Placement {
                    card: CardId(id),
                    lesson: LessonId(1),
                    start: TimeSlot { day: 0, period },
                    room: None,
                },
            );
        }
        assert_eq!(
            soft_indicators(&d),
            SoftIndicators {
                teacher_gap_periods: 2,
                teacher_undesirable_periods: 1
            }
        );
    }
    #[test]
    fn subject_parallel_limit_counts_each_period_of_a_double() {
        let mut d = TimetableDoc::default();
        d.subjects.insert(
            SubjectId(1),
            Subject {
                id: SubjectId(1),
                name: "S".into(),
            },
        );
        d.lessons.insert(
            LessonId(1),
            Lesson {
                id: LessonId(1),
                subject: SubjectId(1),
                teachers: vec![],
                classes: vec![],
                class_scopes: BTreeMap::new(),
                student_count: None,
                duration: 2,
                occurrences: 2,
            },
        );
        d.hard_limits.subject_max_parallel.insert(SubjectId(1), 1);
        for (id, period) in [(1, 0), (2, 1)] {
            Arc::make_mut(&mut d.placements).insert(
                CardId(id),
                Placement {
                    card: CardId(id),
                    lesson: LessonId(1),
                    start: TimeSlot { day: 0, period },
                    room: None,
                },
            );
        }
        assert!(verify(&d)
            .violations
            .contains(&Violation::SubjectParallelLimit {
                subject: SubjectId(1),
                slot: TimeSlot { day: 0, period: 1 },
                actual: 2,
                limit: 1
            }));
    }
    #[test]
    fn counts_unplaced_occurrences_per_lesson() {
        let mut d = TimetableDoc::default();
        d.lessons.insert(
            LessonId(1),
            Lesson {
                id: LessonId(1),
                subject: SubjectId(1),
                teachers: vec![],
                classes: vec![],
                class_scopes: BTreeMap::new(),
                student_count: None,
                duration: 1,
                occurrences: 3,
            },
        );
        let r = verify(&d);
        assert_eq!((r.required, r.placed, r.unplaced), (3, 0, 3));
        Arc::make_mut(&mut d.placements).insert(
            CardId(1),
            Placement {
                card: CardId(1),
                lesson: LessonId(1),
                start: TimeSlot { day: 0, period: 0 },
                room: None,
            },
        );
        let r = verify(&d);
        assert_eq!((r.required, r.placed, r.unplaced), (3, 1, 2));
    }
    #[test]
    fn forecast_only_reports_changes_and_does_not_mutate() {
        let mut d = TimetableDoc::default();
        d.subjects.insert(
            SubjectId(1),
            Subject {
                id: SubjectId(1),
                name: "S".into(),
            },
        );
        d.teachers.insert(
            TeacherId(1),
            Teacher {
                id: TeacherId(1),
                name: "T".into(),
                availability: alina_foundation::Availability::default(),
            },
        );
        d.lessons.insert(
            LessonId(1),
            Lesson {
                id: LessonId(1),
                subject: SubjectId(1),
                teachers: vec![TeacherId(1)],
                classes: vec![],
                class_scopes: BTreeMap::new(),
                student_count: None,
                duration: 1,
                occurrences: 3,
            },
        );
        for id in [1, 2] {
            Arc::make_mut(&mut d.placements).insert(
                CardId(id),
                Placement {
                    card: CardId(id),
                    lesson: LessonId(1),
                    start: TimeSlot { day: 0, period: 0 },
                    room: None,
                },
            );
        }
        let moved = Placement {
            card: CardId(2),
            lesson: LessonId(1),
            start: TimeSlot { day: 0, period: 1 },
            room: None,
        };
        let result = forecast(&d, moved);
        assert!(result.added.is_empty());
        assert!(result
            .resolved
            .iter()
            .any(|v| matches!(v, Violation::TeacherClash { .. })));
        assert!(result.resulting.violations.is_empty());
        assert_eq!(d.placements[&CardId(2)].start.period, 0);
        let unchanged = forecast(
            &d,
            Placement {
                card: CardId(3),
                lesson: LessonId(1),
                start: TimeSlot { day: 0, period: 2 },
                room: None,
            },
        );
        assert!(unchanged.added.is_empty());
        assert!(unchanged.resolved.is_empty());
    }
    #[test]
    fn class_subject_override_takes_precedence_over_subject_limit() {
        let mut d = TimetableDoc::default();
        d.classes.insert(
            ClassId(1),
            Class {
                id: ClassId(1),
                name: "C".into(),
                size: 10,
            },
        );
        d.subjects.insert(
            SubjectId(1),
            Subject {
                id: SubjectId(1),
                name: "S".into(),
            },
        );
        d.lessons.insert(
            LessonId(1),
            Lesson {
                id: LessonId(1),
                subject: SubjectId(1),
                teachers: vec![],
                classes: vec![ClassId(1)],
                class_scopes: BTreeMap::new(),
                student_count: None,
                duration: 2,
                occurrences: 1,
            },
        );
        Arc::make_mut(&mut d.placements).insert(
            CardId(1),
            Placement {
                card: CardId(1),
                lesson: LessonId(1),
                start: TimeSlot { day: 0, period: 0 },
                room: None,
            },
        );
        d.hard_limits
            .subject_periods_per_day
            .insert(SubjectId(1), 1);
        assert!(verify(&d)
            .violations
            .iter()
            .any(|v| matches!(v, Violation::SubjectDailyLimit { .. })));
        d.hard_limits
            .class_subject_periods_per_day
            .entry(ClassId(1))
            .or_default()
            .insert(SubjectId(1), None);
        assert!(!verify(&d)
            .violations
            .iter()
            .any(|v| matches!(v, Violation::SubjectDailyLimit { .. })));
        d.hard_limits
            .class_subject_periods_per_day
            .entry(ClassId(1))
            .or_default()
            .insert(SubjectId(1), Some(1));
        assert!(verify(&d)
            .violations
            .iter()
            .any(|v| matches!(v, Violation::SubjectDailyLimit { .. })));
    }
    #[test]
    fn detects_clashes_in_second_period_and_capacity() {
        let mut d = TimetableDoc::default();
        d.subjects.insert(
            SubjectId(1),
            Subject {
                id: SubjectId(1),
                name: "S".into(),
            },
        );
        d.teachers.insert(
            TeacherId(1),
            Teacher {
                id: TeacherId(1),
                name: "T".into(),
                availability: Availability {
                    blocked: 2,
                    undesirable: 0,
                },
            },
        );
        d.classes.insert(
            ClassId(1),
            Class {
                id: ClassId(1),
                name: "C".into(),
                size: 30,
            },
        );
        d.rooms.insert(
            RoomId(1),
            Room {
                id: RoomId(1),
                name: "R".into(),
                capacity: 20,
            },
        );
        for n in 1..=2 {
            d.lessons.insert(
                LessonId(n),
                Lesson {
                    id: LessonId(n),
                    subject: SubjectId(1),
                    teachers: vec![TeacherId(1)],
                    classes: vec![ClassId(1)],
                    class_scopes: BTreeMap::new(),
                    student_count: None,
                    duration: if n == 1 { 2 } else { 1 },
                    occurrences: 1,
                },
            );
            Arc::make_mut(&mut d.placements).insert(
                CardId(n),
                Placement {
                    card: CardId(n),
                    lesson: LessonId(n),
                    start: TimeSlot {
                        day: 0,
                        period: if n == 1 { 0 } else { 1 },
                    },
                    room: Some(RoomId(1)),
                },
            );
        }
        let r = verify(&d);
        assert_eq!(r.placed, 2);
        assert_eq!(r.violations.len(), 7);
        assert!(r.violations.contains(&Violation::TeacherClash {
            teacher: TeacherId(1),
            slot: TimeSlot { day: 0, period: 1 },
            cards: vec![CardId(1), CardId(2)]
        }));
        assert_eq!(r, verify(&d));
    }
    #[test]
    fn rejects_cross_day_double() {
        let mut d = TimetableDoc::default();
        d.subjects.insert(
            SubjectId(1),
            Subject {
                id: SubjectId(1),
                name: "S".into(),
            },
        );
        d.lessons.insert(
            LessonId(1),
            Lesson {
                id: LessonId(1),
                subject: SubjectId(1),
                teachers: vec![],
                classes: vec![],
                class_scopes: BTreeMap::new(),
                student_count: None,
                duration: 2,
                occurrences: 1,
            },
        );
        Arc::make_mut(&mut d.placements).insert(
            CardId(1),
            Placement {
                card: CardId(1),
                lesson: LessonId(1),
                start: TimeSlot { day: 0, period: 5 },
                room: None,
            },
        );
        assert_eq!(
            verify(&d).violations,
            vec![Violation::InvalidPlacement { card: CardId(1) }]
        );
    }
    #[test]
    fn split_groups_follow_division_rules() {
        use std::collections::BTreeSet;
        let group = |division, ids: &[u32]| ClassScope::Groups {
            division,
            groups: ids.iter().copied().collect::<BTreeSet<_>>(),
        };
        assert!(!scopes_conflict(&group(1, &[1]), &group(1, &[2])));
        assert!(scopes_conflict(&group(1, &[1]), &group(1, &[1, 2])));
        assert!(scopes_conflict(&group(1, &[1]), &group(2, &[2])));
        assert!(scopes_conflict(&ClassScope::Whole, &group(1, &[2])));
        assert!(scopes_conflict(&group(1, &[]), &group(1, &[2])));
    }
    #[test]
    fn preview_does_not_mutate_original() {
        let original = TimetableDoc::default();
        let candidate = Placement {
            card: CardId(1),
            lesson: LessonId(99),
            start: TimeSlot { day: 0, period: 0 },
            room: None,
        };
        let report = preview(&original, candidate);
        assert!(report
            .violations
            .contains(&Violation::MissingLesson { card: CardId(1) }));
        assert!(original.placements.is_empty());
    }
    #[test]
    fn daily_limits_and_allowed_days_include_double_tail() {
        let mut d = TimetableDoc::default();
        d.classes.insert(
            ClassId(1),
            Class {
                id: ClassId(1),
                name: "C".into(),
                size: 20,
            },
        );
        d.teachers.insert(
            TeacherId(1),
            Teacher {
                id: TeacherId(1),
                name: "T".into(),
                availability: Availability::default(),
            },
        );
        d.lessons.insert(
            LessonId(1),
            Lesson {
                id: LessonId(1),
                subject: SubjectId(1),
                teachers: vec![TeacherId(1)],
                classes: vec![ClassId(1)],
                class_scopes: BTreeMap::new(),
                student_count: None,
                duration: 2,
                occurrences: 1,
            },
        );
        d.hard_limits
            .teacher_periods_per_day
            .insert(TeacherId(1), 1);
        d.hard_limits.class_periods_per_day.insert(ClassId(1), 1);
        d.hard_limits
            .subject_periods_per_day
            .insert(SubjectId(1), 1);
        d.hard_limits.lesson_allowed_days.insert(LessonId(1), 1);
        d.hard_limits
            .lesson_fixed_start
            .insert(LessonId(1), TimeSlot { day: 0, period: 0 });
        Arc::make_mut(&mut d.placements).insert(
            CardId(1),
            Placement {
                card: CardId(1),
                lesson: LessonId(1),
                start: TimeSlot { day: 1, period: 0 },
                room: None,
            },
        );
        let v = verify(&d).violations;
        assert!(v.contains(&Violation::DayNotAllowed { card: CardId(1) }));
        assert!(v.contains(&Violation::FixedStartMismatch { card: CardId(1) }));
        assert!(v.contains(&Violation::TeacherDailyLimit {
            teacher: TeacherId(1),
            day: 1,
            actual: 2,
            limit: 1
        }));
        assert!(v.contains(&Violation::ClassDailyLimit {
            class: ClassId(1),
            day: 1,
            actual: 2,
            limit: 1
        }));
        assert!(v.contains(&Violation::SubjectDailyLimit {
            class: ClassId(1),
            subject: SubjectId(1),
            day: 1,
            actual: 2,
            limit: 1
        }));
    }
    #[test]
    fn double_respects_availability_and_start_end_restrictions() {
        let mut d = TimetableDoc::default();
        d.subjects.insert(
            SubjectId(1),
            Subject {
                id: SubjectId(1),
                name: "S".into(),
            },
        );
        d.classes.insert(
            ClassId(1),
            Class {
                id: ClassId(1),
                name: "C".into(),
                size: 20,
            },
        );
        d.rooms.insert(
            RoomId(1),
            Room {
                id: RoomId(1),
                name: "R".into(),
                capacity: 30,
            },
        );
        d.lessons.insert(
            LessonId(1),
            Lesson {
                id: LessonId(1),
                subject: SubjectId(1),
                teachers: vec![],
                classes: vec![ClassId(1)],
                class_scopes: BTreeMap::new(),
                student_count: None,
                duration: 2,
                occurrences: 1,
            },
        );
        d.hard_limits.class_blocked_slots.insert(ClassId(1), 1 << 1);
        d.hard_limits.room_blocked_slots.insert(RoomId(1), 1 << 1);
        d.hard_limits
            .lesson_allowed_starts
            .insert(LessonId(1), 1 << 2);
        d.hard_limits.subject_banned_starts.insert(SubjectId(1), 1);
        d.hard_limits
            .subject_banned_ends
            .insert(SubjectId(1), 1 << 1);
        d.hard_limits
            .lesson_allowed_rooms
            .insert(LessonId(1), [RoomId(2)].into_iter().collect());
        Arc::make_mut(&mut d.placements).insert(
            CardId(1),
            Placement {
                card: CardId(1),
                lesson: LessonId(1),
                start: TimeSlot { day: 0, period: 0 },
                room: Some(RoomId(1)),
            },
        );
        let v = verify(&d).violations;
        assert!(v.contains(&Violation::LessonStartNotAllowed { card: CardId(1) }));
        assert!(v.contains(&Violation::SubjectStartBanned { card: CardId(1) }));
        assert!(v.contains(&Violation::SubjectEndBanned { card: CardId(1) }));
        assert!(v.contains(&Violation::RoomNotAllowed {
            card: CardId(1),
            room: Some(RoomId(1))
        }));
        assert!(v.contains(&Violation::ClassUnavailable {
            card: CardId(1),
            class: ClassId(1),
            slot: TimeSlot { day: 0, period: 1 }
        }));
        assert!(v.contains(&Violation::RoomUnavailable {
            card: CardId(1),
            room: RoomId(1),
            slot: TimeSlot { day: 0, period: 1 }
        }));
    }
    #[test]
    fn pair_relations_use_complete_spans_and_ignore_unplaced_partner() {
        use alina_doc::{CardRelation, CardRelationKind};
        let mut d = TimetableDoc::default();
        d.subjects.insert(
            SubjectId(1),
            Subject {
                id: SubjectId(1),
                name: "S".into(),
            },
        );
        d.lessons.insert(
            LessonId(1),
            Lesson {
                id: LessonId(1),
                subject: SubjectId(1),
                teachers: vec![],
                classes: vec![],
                class_scopes: BTreeMap::new(),
                student_count: None,
                duration: 2,
                occurrences: 2,
            },
        );
        let put = |d: &mut TimetableDoc, id, day, period| {
            Arc::make_mut(&mut d.placements).insert(
                CardId(id),
                Placement {
                    card: CardId(id),
                    lesson: LessonId(1),
                    start: TimeSlot { day, period },
                    room: None,
                },
            );
        };
        d.hard_limits.card_relations.push(CardRelation {
            a: CardId(1),
            b: CardId(2),
            kind: CardRelationKind::NoOverlap,
        });
        put(&mut d, 1, 0, 0);
        assert!(verify(&d).violations.is_empty());
        put(&mut d, 2, 0, 1);
        assert!(verify(&d).violations.contains(&Violation::CardRelation {
            index: 0,
            a: CardId(1),
            b: CardId(2)
        }));
        d.hard_limits.card_relations[0].kind = CardRelationKind::NotAdjacent;
        put(&mut d, 2, 0, 2);
        assert!(verify(&d).violations.contains(&Violation::CardRelation {
            index: 0,
            a: CardId(1),
            b: CardId(2)
        }));
        d.hard_limits.card_relations[0].kind = CardRelationKind::SameStart;
        assert!(verify(&d).violations.contains(&Violation::CardRelation {
            index: 0,
            a: CardId(1),
            b: CardId(2)
        }));
        put(&mut d, 2, 0, 0);
        assert!(verify(&d).violations.is_empty());
        d.hard_limits.card_relations[0].kind = CardRelationKind::DifferentDays;
        assert!(!verify(&d).violations.is_empty());
        d.hard_limits.card_relations[0].kind = CardRelationKind::SameDay;
        put(&mut d, 2, 1, 0);
        assert!(!verify(&d).violations.is_empty());
        d.hard_limits.card_relations[0].kind = CardRelationKind::OrderedAdjacent;
        put(&mut d, 2, 0, 2);
        assert!(verify(&d).violations.is_empty());
        put(&mut d, 2, 0, 3);
        assert!(!verify(&d).violations.is_empty());
        d.hard_limits.card_relations[0].kind = CardRelationKind::SameDayOrdered;
        assert!(verify(&d).violations.is_empty());
        put(&mut d, 2, 0, 1);
        assert!(!verify(&d).violations.is_empty());
        d.hard_limits.card_relations[0].kind = CardRelationKind::Adjacent;
        put(&mut d, 1, 0, 2);
        put(&mut d, 2, 0, 0);
        assert!(verify(&d).violations.is_empty());
    }
    #[test]
    fn split_group_capacity_requires_known_size() {
        let mut d = TimetableDoc::default();
        d.subjects.insert(
            SubjectId(1),
            Subject {
                id: SubjectId(1),
                name: "S".into(),
            },
        );
        d.classes.insert(
            ClassId(1),
            Class {
                id: ClassId(1),
                name: "C".into(),
                size: 40,
            },
        );
        d.rooms.insert(
            RoomId(1),
            Room {
                id: RoomId(1),
                name: "R".into(),
                capacity: 20,
            },
        );
        let mut scopes = BTreeMap::new();
        scopes.insert(
            ClassId(1),
            ClassScope::Groups {
                division: 1,
                groups: [1].into_iter().collect(),
            },
        );
        d.lessons.insert(
            LessonId(1),
            Lesson {
                id: LessonId(1),
                subject: SubjectId(1),
                teachers: vec![],
                classes: vec![ClassId(1)],
                class_scopes: scopes,
                student_count: None,
                duration: 1,
                occurrences: 1,
            },
        );
        Arc::make_mut(&mut d.placements).insert(
            CardId(1),
            Placement {
                card: CardId(1),
                lesson: LessonId(1),
                start: TimeSlot { day: 0, period: 0 },
                room: Some(RoomId(1)),
            },
        );
        assert!(verify(&d)
            .violations
            .contains(&Violation::GroupCapacityUnknown { card: CardId(1) }));
        if let Some(l) = d.lessons.get_mut(&LessonId(1)) {
            l.student_count = Some(15)
        }
        assert!(verify(&d).violations.is_empty());
        if let Some(l) = d.lessons.get_mut(&LessonId(1)) {
            l.student_count = Some(21)
        }
        assert!(verify(&d).violations.contains(&Violation::RoomCapacity {
            card: CardId(1),
            room: RoomId(1),
            required: 21,
            capacity: 20
        }));
    }
}
