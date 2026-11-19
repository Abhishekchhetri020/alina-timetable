# Chronexa parity inventory

Chronexa repository inspected at `ef92770` on 2026-11-19. Alina is a separate Rust rewrite. Features are targets, not claims of implementation. Chronexa's local working tree had unrelated edits; treat its source as reference and do not alter it.

| Area | Chronexa reference | Alina status |
| --- | --- | --- |
| School setup, teachers, subjects, classes, rooms, lessons, groups, divisions, bells, day/week/term definitions | `js/ui/entities/`, `js/ui/wizard/` | Initial data structures only; groups, bells and patterns missing |
| aSc XML import/export and round trip | `js/xml/`, `js/ui/` export | Not implemented |
| Timetable editor, unplaced tray, drag, swap, undo, class/teacher/room views | `js/ui/editor/`, `js/ui/grid_view.js` | Not implemented |
| Verification and hard/soft constraints | `js/solver/constraints.js`, `constraint_keys.js`, `csp_solver.js` | Initial subset oracle: teacher/class/room clashes, blocked teacher slots, room capacity and span bounds. No split-group semantics; no soft scores |
| Generation, improve, advanced relations and diagnostics | `js/solver/csp_solver.js`, `backend/solver_cpsat.py`, `js/solver/wasm/` | Not implemented |
| Substitution planning, ranking, fairness and outputs | `js/ui/substitution/` | Not implemented |
| Print layouts, reports, preview, booklet and saved styles | `js/ui/print_preview/` | Not implemented |
| Viewer, saved school, offline/PWA, import/export and onboarding | `js/viewer/`, `js/ui/` | Not implemented |

## Acceptance approach

For each feature, derive a behavior specification and synthetic fixture from the existing application, implement it in Rust, and compare observable output with Chronexa. Cover real anonymized school-sized data locally without committing it. Track differences in constraint semantics rather than treating all checks as equivalent. Verify every period of multi-period cards; represent splits and group overlaps before using class conflict checks for production. Test user workflows as well as engine functions, including saved-file round trips. Benchmark identical inputs and budgets; do not claim parity or speed until measured.

The current oracle assumes each lesson's classes are whole classes and sums their sizes for room capacity. It must not be used to validate split classes or merged groups until those models are implemented. It counts stored placements, not all valid or unplaced cards. Its existing findings do not constitute comprehensive validation.
