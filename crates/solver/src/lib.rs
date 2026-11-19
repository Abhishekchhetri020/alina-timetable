#![deny(clippy::unwrap_used)]
//! Deterministic, bounded reference solver for the currently modeled subset.
//! It does not implement the full Chronexa constraint set or optimize soft scores.
use alina_doc::{Placement, TimetableDoc};
use alina_foundation::{CardId, LessonId, RoomId, TimeSlot};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// A card occurrence and its permissible rooms. An empty list means no room.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardRequest {
    pub card: CardId,
    pub lesson: LessonId,
    pub rooms: Vec<RoomId>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// All requested cards were placed and every lesson occurrence is present.
    Complete,
    /// All requested cards were placed, but the input omitted requirements.
    RequestComplete,
    Exhausted,
    BudgetReached,
}
#[derive(Clone, Debug)]
pub struct SolveResult {
    pub status: Status,
    pub placements: Vec<Placement>,
    pub unplaced: Vec<CardId>,
    pub nodes: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SolveError {
    InvalidBase,
    DuplicateCard(CardId),
    MissingLesson(LessonId),
    OccurrenceOverflow(LessonId),
    UnknownRelationCard(CardId),
    /// Final independent verification detected an internal solver error.
    InvalidResult,
    InvalidBudget,
}

/// Find a valid complete placement, or return the best partial placement found.
/// The base must pass the oracle. Existing placements are immutable. This is a
/// reference implementation: each candidate is independently verified.
pub fn solve(
    base: &TimetableDoc,
    requests: &[CardRequest],
    budget: u64,
) -> Result<SolveResult, SolveError> {
    if budget == 0 {
        return Err(SolveError::InvalidBudget);
    }
    if !alina_oracle::verify(base).violations.is_empty() {
        return Err(SolveError::InvalidBase);
    }
    let mut ids = BTreeSet::new();
    for request in requests {
        if !ids.insert(request.card) || base.placements.contains_key(&request.card) {
            return Err(SolveError::DuplicateCard(request.card));
        }
        if !base.lessons.contains_key(&request.lesson) {
            return Err(SolveError::MissingLesson(request.lesson));
        }
    }
    // A relation cannot be enforced if either endpoint is outside the solve.
    // Reject it instead of reporting a falsely complete solution.
    for relation in &base.hard_limits.card_relations {
        for card in [relation.a, relation.b] {
            if !ids.contains(&card) && !base.placements.contains_key(&card) {
                return Err(SolveError::UnknownRelationCard(card));
            }
        }
    }
    // Enforce the lesson occurrence count across fixed and requested cards.
    let mut counts = std::collections::BTreeMap::<LessonId, usize>::new();
    for p in base.placements.values() {
        *counts.entry(p.lesson).or_default() += 1;
    }
    for r in requests {
        *counts.entry(r.lesson).or_default() += 1;
    }
    for (id, count) in counts {
        if let Some(lesson) = base.lessons.get(&id) {
            if count > usize::from(lesson.occurrences) {
                return Err(SolveError::OccurrenceOverflow(id));
            }
        }
    }
    let mut ordered = requests.to_vec();
    ordered.sort_by_key(|r| r.card);
    let mut ctx = Search {
        doc: base.clone(),
        requests: &ordered,
        budget,
        nodes: 0,
        best: Vec::new(),
        current: Vec::new(),
        limited: false,
    };
    let complete = ctx.search(&ordered.iter().map(|r| r.card).collect::<Vec<_>>());
    let chosen = ctx.best;
    let mut final_doc = base.clone();
    for placement in &chosen {
        Arc::make_mut(&mut final_doc.placements).insert(placement.card, placement.clone());
    }
    let report = alina_oracle::verify(&final_doc);
    if !report.violations.is_empty() {
        return Err(SolveError::InvalidResult);
    }
    let assigned: BTreeSet<_> = chosen.iter().map(|p| p.card).collect();
    let unplaced = ordered
        .iter()
        .filter(|r| !assigned.contains(&r.card))
        .map(|r| r.card)
        .collect();
    Ok(SolveResult {
        status: if complete && report.unplaced == 0 {
            Status::Complete
        } else if complete {
            Status::RequestComplete
        } else if ctx.limited {
            Status::BudgetReached
        } else {
            Status::Exhausted
        },
        placements: chosen,
        unplaced,
        nodes: ctx.nodes,
    })
}
struct Search<'a> {
    doc: TimetableDoc,
    requests: &'a [CardRequest],
    budget: u64,
    nodes: u64,
    best: Vec<Placement>,
    current: Vec<Placement>,
    limited: bool,
}
/// Cheap rejection index. The oracle remains authoritative; this index must
/// only reject unavoidable clashes and is rebuilt at every search node so
/// backtracking cannot leave stale occupancy behind.
#[derive(Default)]
struct Occupancy {
    teachers: BTreeMap<alina_foundation::TeacherId, u64>,
    rooms: BTreeMap<RoomId, u64>,
}
impl Occupancy {
    fn from_doc(doc: &TimetableDoc) -> Self {
        let mut state = Self::default();
        for p in doc.placements.values() {
            if let Some(lesson) = doc.lessons.get(&p.lesson) {
                let mask = span_mask(doc, p.start, lesson.duration);
                for teacher in &lesson.teachers {
                    *state.teachers.entry(*teacher).or_default() |= mask;
                }
                if let Some(room) = p.room {
                    *state.rooms.entry(room).or_default() |= mask;
                }
            }
        }
        state
    }
    fn conflicts(&self, doc: &TimetableDoc, candidate: &Placement) -> bool {
        let Some(lesson) = doc.lessons.get(&candidate.lesson) else {
            return true;
        };
        let mask = span_mask(doc, candidate.start, lesson.duration);
        lesson
            .teachers
            .iter()
            .any(|id| self.teachers.get(id).is_some_and(|m| m & mask != 0))
            || candidate
                .room
                .is_some_and(|id| self.rooms.get(&id).is_some_and(|m| m & mask != 0))
    }
}
fn span_mask(doc: &TimetableDoc, start: TimeSlot, duration: u8) -> u64 {
    let mut mask = 0u64;
    for offset in 0..duration {
        let bit = u32::from(start.day) * u32::from(doc.periods_per_day)
            + u32::from(start.period)
            + u32::from(offset);
        if bit < 64 {
            mask |= 1u64 << bit;
        }
    }
    mask
}
impl Search<'_> {
    fn candidates(&self, r: &CardRequest, occupancy: &Occupancy) -> Vec<Placement> {
        let rooms: Vec<Option<RoomId>> = if r.rooms.is_empty() {
            vec![None]
        } else {
            let mut x = r.rooms.clone();
            x.sort();
            x.dedup();
            x.into_iter().map(Some).collect()
        };
        let mut candidates = Vec::new();
        for d in 0..self.doc.days {
            for p in 0..self.doc.periods_per_day {
                for room in &rooms {
                    // Locked cells are not available for new generated placements.
                    let Some(lesson) = self.doc.lessons.get(&r.lesson) else {
                        continue;
                    };
                    let end = u16::from(p) + u16::from(lesson.duration);
                    if end > u16::from(self.doc.periods_per_day)
                        || (u16::from(p)..end).any(|period| {
                            self.doc.locked_slots.contains(&TimeSlot {
                                day: d,
                                period: period as u8,
                            })
                        })
                    {
                        continue;
                    }
                    let candidate = Placement {
                        card: r.card,
                        lesson: r.lesson,
                        start: TimeSlot { day: d, period: p },
                        room: *room,
                    };
                    if occupancy.conflicts(&self.doc, &candidate) {
                        continue;
                    }
                    if alina_oracle::preview(&self.doc, candidate.clone())
                        .violations
                        .is_empty()
                    {
                        candidates.push(candidate);
                    }
                }
            }
        }
        candidates
    }
    fn search(&mut self, remaining: &[CardId]) -> bool {
        if self.nodes >= self.budget {
            self.limited = true;
            return false;
        }
        self.nodes += 1;
        if self.current.len() > self.best.len() {
            self.best = self.current.clone();
        }
        if remaining.is_empty() {
            return self.current.len() == self.requests.len();
        }
        let mut choice = None;
        let mut options = Vec::new();
        let occupancy = Occupancy::from_doc(&self.doc);
        for id in remaining {
            if let Some(r) = self.requests.iter().find(|r| r.card == *id) {
                let c = self.candidates(r, &occupancy);
                if choice.is_none() || c.len() < options.len() {
                    choice = Some(*id);
                    options = c;
                }
            }
        }
        let Some(id) = choice else { return false };
        let rest: Vec<_> = remaining.iter().copied().filter(|x| *x != id).collect();
        for placement in options {
            Arc::make_mut(&mut self.doc.placements).insert(id, placement.clone());
            self.current.push(placement);
            if self.search(&rest) {
                return true;
            }
            self.current.pop();
            Arc::make_mut(&mut self.doc.placements).remove(&id);
            if self.limited {
                return false;
            }
        }
        // Continue without this card to find the largest partial schedule.
        self.search(&rest)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use alina_doc::{Class, Lesson, Subject, Teacher};
    use alina_foundation::{Availability, ClassId, SubjectId, TeacherId};
    use std::collections::BTreeMap;
    fn fixture() -> TimetableDoc {
        let mut d = TimetableDoc {
            days: 1,
            periods_per_day: 2,
            ..Default::default()
        };
        d.teachers.insert(
            TeacherId(1),
            Teacher {
                id: TeacherId(1),
                name: "T".into(),
                availability: Availability::default(),
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
                teachers: vec![TeacherId(1)],
                classes: vec![ClassId(1)],
                class_scopes: BTreeMap::new(),
                student_count: None,
                duration: 1,
                occurrences: 3,
            },
        );
        d
    }
    #[test]
    fn complete_result_is_verified_and_repeatable() -> Result<(), SolveError> {
        let d = fixture();
        let req: Vec<_> = (1..=2)
            .map(|n| CardRequest {
                card: CardId(n),
                lesson: LessonId(1),
                rooms: vec![],
            })
            .collect();
        let a = solve(&d, &req, 100)?;
        let b = solve(&d, &req, 100)?;
        assert_eq!(a.status, Status::RequestComplete);
        assert_eq!(format!("{:?}", a.placements), format!("{:?}", b.placements));
        let mut result = d;
        for p in a.placements {
            Arc::make_mut(&mut result.placements).insert(p.card, p);
        }
        assert!(alina_oracle::verify(&result).violations.is_empty());
        Ok(())
    }
    #[test]
    fn excess_cards_produce_partial_and_not_false_complete() -> Result<(), SolveError> {
        let d = fixture();
        let req: Vec<_> = (1..=3)
            .map(|n| CardRequest {
                card: CardId(n),
                lesson: LessonId(1),
                rooms: vec![],
            })
            .collect();
        let r = solve(&d, &req, 1000)?;
        assert_eq!(r.status, Status::Exhausted);
        assert_eq!(r.unplaced.len(), 1);
        Ok(())
    }
    #[test]
    fn budget_is_explicit() -> Result<(), SolveError> {
        let d = fixture();
        let req = vec![CardRequest {
            card: CardId(1),
            lesson: LessonId(1),
            rooms: vec![],
        }];
        assert_eq!(solve(&d, &req, 1)?.status, Status::BudgetReached);
        Ok(())
    }
    #[test]
    fn complete_requires_all_lesson_occurrences() -> Result<(), SolveError> {
        let mut d = fixture();
        if let Some(lesson) = d.lessons.get_mut(&LessonId(1)) {
            lesson.occurrences = 2;
        }
        let req: Vec<_> = (1..=2)
            .map(|n| CardRequest {
                card: CardId(n),
                lesson: LessonId(1),
                rooms: vec![],
            })
            .collect();
        assert_eq!(solve(&d, &req, 100)?.status, Status::Complete);
        assert_eq!(solve(&d, &req[..1], 100)?.status, Status::RequestComplete);
        Ok(())
    }
    #[test]
    fn missing_relation_endpoint_is_rejected() {
        use alina_doc::{CardRelation, CardRelationKind};
        let mut doc = fixture();
        doc.hard_limits.card_relations.push(CardRelation {
            a: CardId(1),
            b: CardId(2),
            kind: CardRelationKind::SameDay,
        });
        let requests = [CardRequest {
            card: CardId(1),
            lesson: LessonId(1),
            rooms: vec![],
        }];
        assert!(matches!(
            solve(&doc, &requests, 100),
            Err(SolveError::UnknownRelationCard(CardId(2)))
        ));
    }
    #[test]
    fn occupancy_filter_matches_oracle_clashes_on_double_periods() {
        let mut doc = fixture();
        if let Some(lesson) = doc.lessons.get_mut(&LessonId(1)) {
            lesson.duration = 2;
        }
        let fixed = Placement {
            card: CardId(1),
            lesson: LessonId(1),
            start: TimeSlot { day: 0, period: 0 },
            room: None,
        };
        Arc::make_mut(&mut doc.placements).insert(fixed.card, fixed);
        let state = Occupancy::from_doc(&doc);
        let candidate = Placement {
            card: CardId(2),
            lesson: LessonId(1),
            start: TimeSlot { day: 0, period: 0 },
            room: None,
        };
        assert!(state.conflicts(&doc, &candidate));
        assert!(!alina_oracle::preview(&doc, candidate).violations.is_empty());
        Arc::make_mut(&mut doc.placements).clear();
        assert!(!Occupancy::from_doc(&doc).conflicts(
            &doc,
            &Placement {
                card: CardId(2),
                lesson: LessonId(1),
                start: TimeSlot { day: 0, period: 0 },
                room: None
            }
        ));
    }
}
