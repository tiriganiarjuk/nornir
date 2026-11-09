# Building Nornir

Do NOT run `cargo build --release` or `maturin build` directly.
Use `nornir_deploy` instead.

## Why

1. **Dependency coherence.** When a shared crate changes (e.g., `datagram_io`, `schemas_embedded`), ALL consumers must rebuild together. Running `cargo build --release -p one_crate` leaves other binaries in the same category running stale code. Strange runtime bugs with no obvious cause.

2. **Deployment.** `cargo build` puts binaries in cargo's target dir — on the Derived volume, per `.cargo/config.toml`, not under this repo. They are not deployed until **copied** to `~/ai/tools/bin/` (copied, never symlinked: a deployed tool must keep working with that drive unplugged). The deploy tool handles build, copy, and verification of the deployed copy as one atomic operation.

3. **Gates require maturin.** PyO3 gate modules cannot be built with `cargo build`. They need `maturin build`, wheel extraction, and `.so` deployment to `~/ai/tools/lib/`. The deploy tool handles all of this.

4. **Schema embedding.** Schemas are embedded at compile time via `include_str!()`. A changed `.schema.json` has no effect until the consuming binary is rebuilt and redeployed.

## Commands

```
nornir_deploy --all                  # rebuild + deploy everything
nornir_deploy --non-pyo3             # all cargo crates, skip maturin gates
nornir_deploy --build hooks          # single category
nornir_deploy --build hooks,writers  # multiple categories
nornir_deploy --list                 # show categories and crate counts
```

Categories are defined in `deploy_categories.toml`.

## Adding a new crate

1. Add the crate to `deploy_categories.toml` under the appropriate category.
2. Run `nornir_deploy --build <category>`.

## What is safe to run directly

- `cargo check` — compilation checking, no deployment
- `cargo test` — run tests
- `cargo clippy` — lint checking
- `cargo build` (debug, no `--release`) — compilation checking, produces debug binaries only

A hook intercepts `cargo build --release` and `maturin build` to remind you.

## deploy_categories.toml Reference

Each category in `deploy_categories.toml` specifies a `verify` method that nornir_deploy runs after building to confirm the binary works.

| Method | What it does | When to use |
|--------|-------------|-------------|
| `help` | Runs `binary --help`, expects exit 0 | Binaries with clap or manual `--help` |
| `stdin` | Pipes `verify_input` JSON to stdin, expects exit 0 | Binaries that read JSON from stdin (hooks, converters, rewriters) |
| `exit_codes` | Runs binary with no args, accepts any code in `valid_exit_codes` | Binaries that exit non-zero on missing args but prove they load |
| `python_import` | Runs `python -c "import <module>"`, expects exit 0 | PyO3 `.so` modules (gates, interceptors) |

Additional fields:
- `verify_input` — JSON string piped to stdin (required when `verify = "stdin"`)
- `valid_exit_codes` — list of accepted exit codes (required when `verify = "exit_codes"`)
- `binary_names` — optional table mapping crate name to binary name when they differ (e.g., `saga_cli = "saga"`)
- `pyo3_crates` — crates built with maturin instead of cargo
- `python_version` — Python version for maturin builds (e.g., `"python3.13"`)
