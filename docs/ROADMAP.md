# Bootstrap roadmap

1. Finalize foundation and document invariants, individual cards and split groups; test serialization and invalid input.
2. Implement atomic `card.place`, `card.unplace`, `card.swap` and a command registry with 200+ undo/redo entries. Test failure atomicity, replay and history isolation.
3. Implement an independent deterministic oracle for teacher, class/division and room conflicts across every occupied period, capacity, soft penalties and consequences. Validate on anonymized realistic fixtures.
4. Implement stable codecs and a headless CLI, followed by an MCP server with explicit session/concurrency and persistence boundaries.
5. Integrate egui/eframe and wgpu for native and web targets, virtualized dense grids, keyboard and pointer commands. Verify actual desktop and browser builds.
6. Implement and benchmark the Rust solver against independently verified, anonymized school datasets; document hardware, seeds, time budgets and constraint coverage. Measure performance rather than assuming it.
