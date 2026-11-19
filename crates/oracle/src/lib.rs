#![deny(clippy::unwrap_used)]
//! Independent deterministic checks for the supported document subset.
//! This is not yet a complete Chronexa constraint oracle.
use alina_doc::TimetableDoc;
use alina_foundation::{CardId, ClassId, RoomId, TeacherId, TimeSlot};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Violation {
    InvalidWeek,
    InvalidPlacement {
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

/// Checks the supported constraints in stable ID and slot order. Counts are
/// placements only; an exact unplaced count requires explicit card identities.
pub fn verify(doc: &TimetableDoc) -> Report {
    let mut violations = Vec::new();
    let mut teachers: BTreeMap<(TeacherId, TimeSlot), Vec<CardId>> = BTreeMap::new();
    let mut classes: BTreeMap<(ClassId, TimeSlot), Vec<CardId>> = BTreeMap::new();
    let mut rooms: BTreeMap<(RoomId, TimeSlot), Vec<CardId>> = BTreeMap::new();
    let week_ok = doc.days > 0
        && doc.periods_per_day > 0
        && u16::from(doc.days) * u16::from(doc.periods_per_day) <= 64;
    if !week_ok {
        violations.push(Violation::InvalidWeek);
    }
    for (&key, placement) in doc.placements.iter() {
        if key != placement.card {
            violations.push(Violation::InvalidPlacement { card: key });
        }
        let Some(lesson) = doc.lessons.get(&placement.lesson) else {
            violations.push(Violation::MissingLesson { card: key });
            continue;
        };
        let end = u16::from(placement.start.period) + u16::from(lesson.duration);
        if !week_ok
            || lesson.duration == 0
            || placement.start.day >= doc.days
            || end > u16::from(doc.periods_per_day)
        {
            violations.push(Violation::InvalidPlacement { card: key });
            continue;
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
        if let Some(room) = placement.room {
            match doc.rooms.get(&room) {
                None => violations.push(Violation::MissingRoom { card: key, room }),
                Some(r) if size > u64::from(r.capacity) => {
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
                    classes.entry((*class, slot)).or_default().push(key);
                }
            }
            if let Some(room) = placement.room {
                if doc.rooms.contains_key(&room) {
                    rooms.entry((room, slot)).or_default().push(key);
                }
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
    for ((class, slot), mut cards) in classes {
        cards.sort();
        cards.dedup();
        if cards.len() > 1 {
            violations.push(Violation::ClassClash { class, slot, cards });
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
    use alina_doc::{Class, Lesson, Placement, Room, Teacher};
    use alina_foundation::{Availability, LessonId, SubjectId};
    use std::sync::Arc;
    #[test]
    fn detects_clashes_in_second_period_and_capacity() {
        let mut d = TimetableDoc::default();
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
        d.lessons.insert(
            LessonId(1),
            Lesson {
                id: LessonId(1),
                subject: SubjectId(1),
                teachers: vec![],
                classes: vec![],
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
}
