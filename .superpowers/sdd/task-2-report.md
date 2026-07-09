Status: DONE

Summary:
- Replaced the CLI command surface with `dataset`, `pack`, and `version`, removing daemon-facing subcommands from clap parsing.
- Added dataset handlers for `materialize sqlite`, `inspect`, and `query` using `kat-rs-datasource`.
- Kept `pack` commands as the explicit placeholder error required by the brief.
- Replaced the CLI command tests with short-lived command-surface expectations and an end-to-end SQLite dataset flow.

Files changed:
- `Cargo.lock`
- `crates/kat-rs-cli/Cargo.toml`
- `crates/kat-rs-cli/src/commands.rs`
- `crates/kat-rs-cli/tests/commands.rs`

Verification:
- `cargo test -p kat-rs-cli --test commands -- --nocapture`
- `cargo test -p kat-rs-cli`

Test results:
- `cargo test -p kat-rs-cli --test commands -- --nocapture`: 4 passed, 0 failed
- `cargo test -p kat-rs-cli`: 4 passed, 0 failed

Concerns:
- None.
