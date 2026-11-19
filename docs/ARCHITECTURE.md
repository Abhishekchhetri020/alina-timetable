# Architecture

Alina is one Rust workspace targeting native desktop and browser WebAssembly. The intended presentation technology is egui/eframe with wgpu; neither is integrated yet. GitHub Pages can host the eventual browser build, but no deployable app exists today. Browser support for the selected renderer must be verified, and a fallback renderer may be needed.

`foundation` defines typed IDs and time primitives. `doc` holds serializable school data. `oracle` will be the authoritative, deterministic source of counts, hard violations, soft penalties and placement consequences. `solver` will propose changes but must be independently checked by the oracle. `engine` owns the session and dispatches serializable commands, transactions and history. `automation` and `ui-egui` consume engine APIs; apps compose those layers. No UI may mutate a live document directly.

## Open design decisions

- `TimetableDoc` is a provisional transport structure with public fields; it does **not** enforce the UI mutation boundary. The future `Session` must encapsulate it and expose immutable views and command dispatch only.
- The requested `Command` trait has no per-invocation undo token. A reusable stateless command cannot reliably undo independent invocations with that signature alone. The history layer must store per-invocation inverse data or snapshots, and transactionally restore state on errors. The trait declaration does not promise undo/redo today.
- Card identity is separate from lesson identity: a lesson can have multiple occurrences. The model must explicitly represent every unplaced card before exact counts or command IDs can be finalized.
- Split divisions, slot locks with class scope, substitution dates, soft weights, bell breaks, card requirements, teacher availability validation, room assignment and group overlap semantics need explicit modeling. A 64-bit mask supports only week shapes with at most 64 slots. Validate all bounds and disjoint availability masks at boundaries.
- Do not claim determinism or a performance target until input ordering, random seeds, platform behavior, algorithms and benchmark fixtures are specified and tested. Conflict reporting must remain reproducible.
- Chronexa is the authorized feature and behavior reference. Document behavior and use synthetic fixtures for reproducible compatibility checks; do not commit private school data.
