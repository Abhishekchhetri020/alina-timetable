# Chronexa parity inventory

Chronexa repository inspected at `ef92770` on 2026-11-19. Alina is a separate Rust rewrite. Features are targets, not claims of implementation. Chronexa's local working tree had unrelated edits; treat its source as reference and do not alter it.

| Area | Chronexa reference | Alina status |
| --- | --- | --- |
| School setup, teachers, subjects, classes, rooms, lessons, groups, divisions, bells, day/week/term definitions | `js/ui/entities/`, `js/ui/wizard/` | Initial data structures only; groups, bells and patterns missing |
| aSc XML import/export and round trip | `js/xml/`, `js/ui/` export | Not implemented |
| Chronexa school JSON import | `js/solver/csp_solver.js` | Strict experimental subset: entities, occurrence expansion, room choices, teacher time-off, class bells, day patterns, split-group scopes, fixed starts and locked rooms. Unknown fields and unsupported rules are rejected; real Chronexa files typically contain additional fields and are not yet supported. |
| Timetable editor, unplaced tray, drag, swap, undo, class/teacher/room views | `js/ui/editor/`, `js/ui/grid_view.js` | Not implemented |
| Verification and hard/soft constraints | `js/solver/constraints.js`, `constraint_keys.js`, `csp_solver.js` | Initial subset oracle: clashes, availability, capacities, daily limits, period restrictions and explicit pairwise relations; teacher gaps and undesirable periods are reported as unweighted indicators |
| Generation, improve, advanced relations and diagnostics | `js/solver/csp_solver.js`, `backend/solver_cpsat.py`, `js/solver/wasm/` | Reference backtracking solver and explicit card-pair relations only; scoped relations and soft optimization not implemented |
| Substitution planning, ranking, fairness and outputs | `js/ui/substitution/` | Not implemented |
| Print layouts, reports, preview, booklet and saved styles | `js/ui/print_preview/` | Not implemented |
| Viewer, saved school, offline/PWA, import/export and onboarding | `js/viewer/`, `js/ui/` | Not implemented |

## Acceptance approach

For each feature, derive a behavior specification and synthetic fixture from the existing application, implement it in Rust, and compare observable output with Chronexa. Cover real anonymized school-sized data locally without committing it. Track differences in constraint semantics rather than treating all checks as equivalent. Verify every period of multi-period cards; represent splits and group overlaps before using class conflict checks for production. Test user workflows as well as engine functions, including saved-file round trips. Benchmark identical inputs and budgets; do not claim parity or speed until measured.

The oracle supports conservative split-group conflict checks and subject parallel caps. Explicit group participant counts can be supplied; a room assignment for a split group with unknown size reports unknown capacity rather than claiming it fits. Full division validation remains incomplete. It reports required and unplaced occurrence counts by lesson, but does not identify missing cards. Its existing findings do not constitute comprehensive validation.

The reference Rust solver is deliberately bounded and validates candidates through the oracle. It is not optimized for school scale, does not implement Chronexa parity, and cannot yet model all required constraints.

The command session supports atomic place, unplace, swap, slot locking and snapshot undo/redo. Commands validate the resulting document with the oracle; command replay persistence, richer editing commands and collaborative synchronization remain unimplemented. The importer expands a subset of Chronexa scoped relations (n_0, n_1, n_2, n_8, n_9, n_10, n_12 and n_13) to card pairs. For n_12/n_13, periods must match only when both cards are on the same day, matching Chronexa’s solver. Other scoped relations remain unsupported. The importer rejects n_0 when matched lessons span multiple periods: direct Chronexa solver checks show order-dependent behavior for doubles, so a deterministic pairwise rule cannot reproduce both placement orders.

The CLI can run `cargo run -p alina-cli -- solve-chronexa tests/fixtures/chronexa-minimal.json /tmp/alina-result.json` and `cargo run -p alina-cli -- verify /tmp/alina-result.json`. This is a synthetic smoke test, not a Chronexa parity or scale benchmark. The importer rejects unrecognized fields and known unsupported scheduling data. Verification reports supported hard violations and occurrence counts; it does not establish comprehensive constraint coverage. Output is provisional Alina JSON, not an aSc or Chronexa round trip.

The Chronexa JSON adapter imports group membership for class-collision checks. It imports explicit group participant counts and rejects division records and lessons combining groups from different divisions until their semantics are modeled. Room assignments for split groups with unknown participant counts remain unverifiable and are reported by the oracle.

Chronexa n_16 first/last placement relations are imported with per-class bell masks and full lesson durations.
