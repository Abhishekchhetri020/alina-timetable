# Alina

Alina is an open-source, local-first school timetabling project, inspired by Chronexa and intended to run as a native Rust application and in browsers using WebAssembly. This repository is an **architecture bootstrap**: it does not yet generate or verify timetables, run a GUI, or expose a CLI/MCP service. Do not use it for school scheduling yet.

## Workspace

| Layer | Crates | Responsibility |
| --- | --- | --- |
| 0 | `foundation` | IDs, coordinates and primitives |
| 1 | `doc` | Timetable document and entities |
| 2 | `codecs` | Future import/export |
| 3 | `oracle`, `solver` | Future authoritative validation and scheduling |
| 4 | `engine` | Future command registry, session and history |
| 5 | `automation`, `ui-egui` | Future MCP/CLI integration and egui interface |
| 6 | `apps/alina-*` | Desktop, CLI and browser entry points |

Dependencies must point down the layers; engine, oracle and solver must never depend on egui. The placeholder crates and apps are not implemented features. See [architecture](docs/ARCHITECTURE.md) and [roadmap](docs/ROADMAP.md).

## Development

Install Rust stable and `rustup target add wasm32-unknown-unknown`.

```sh
cargo fmt --all --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo check --workspace --target wasm32-unknown-unknown --locked
```

Never commit real school data, credentials, or private fixtures. The project schema is provisional and must not be treated as a stable file format.
