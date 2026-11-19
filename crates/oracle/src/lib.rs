#![deny(clippy::unwrap_used)]
//! Independent deterministic checks for the supported document subset.
//! This is not yet a complete Chronexa constraint oracle.
use alina_doc::{ClassScope, Placement, TimetableDoc};
use alina_foundation::{CardId, ClassId, RoomId, TeacherId, TimeSlot};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Violation {
    InvalidWeek,
    InvalidAvailability {
        teacher: TeacherId,
    },
    OccurrenceOverflow {
        lesson: alina_foundation::LessonId,
        actual: usize,
        limit: u16,
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
    pub violations: Vec<Violation>,
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
        if has_groups && placement.room.is_some() {
            violations.push(Violation::GroupCapacityUnknown { card: key });
        }
        if let Some(room) = placement.room {
            match doc.rooms.get(&room) {
                None => violations.push(Violation::MissingRoom { card: key, room }),
                Some(r) if !has_groups && size > u64::from(r.capacity) => {
                    violations.push(Violation::RoomCapacity {
                        card: key,
                        room,
                        required: size,
                        capacity: r.capacity,
                    })
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
                    rooms.entry((room, slot)).or_default().push(key);
                }
            }
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
        if let Some(&limit) = doc.hard_limits.subject_periods_per_day.get(&subject) {
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
}
