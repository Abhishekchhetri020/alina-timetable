# Alina contributor instructions

Read README.md, docs/ARCHITECTURE.md and docs/ROADMAP.md before changing behavior. This is a bootstrap, not a working scheduler. Maintain the one-way dependency graph. Engine, solver and oracle must never depend on the UI. All live document changes must eventually pass through atomic engine commands; do not implement separate UI or automation mutation paths.

Add focused tests for constraints, multi-period boundaries, command atomicity, undo/redo and deterministic results as those features are built. The oracle is the independent source for counts and violations, not a solver-owned cache. Never claim compatibility, speed or production readiness without evidence. Never commit private school data or credentials. Preserve unrelated changes and coordinate shared edits. Run the four checks in README before handing off changes.
