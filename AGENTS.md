# Repository Guidelines

## Structure

This repository contains only ssh-tunnel 0.1.0, built with Tauri 2, React,
TypeScript and Rust. Native commands and desktop lifecycle live in `src-tauri/`;
SSH, forwarding, configuration, imports and diagnostics live in `crates/tunnel-core/`.
The UI and component tests are in `frontend/src/`, with native WebView2 regression
coverage in `frontend/scripts/native-smoke.mjs`. Application icons are in `build/`.

## Build and Test

- `pnpm --dir frontend install --frozen-lockfile`
- `pnpm dev`: real Tauri desktop with Vite live updates; Vite alone has no native IPC.
- `cargo fmt --all --check`
- `cargo test --workspace --locked`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `pnpm test:frontend`
- `pnpm build --debug --no-bundle` then `pnpm test:native`
- `pnpm build:release` then `pnpm test:native:release`
- `pnpm test:native:config`: implicit configuration lookup in a synthetic home.
- `pnpm stage:windows`: copy the tested release EXE and checksum from a clean commit.

Native evidence stays in ignored `target/native-smoke/`. Inspect failure evidence,
not just compiler output. CI tests/builds Windows and macOS; desktop interactions
must not be claimed verified on a platform merely because it compiles.

## Coding and Review

Use Rustfmt, adjacent Rust regression tests, PascalCase React components and typed
IPC in `frontend/src/api.ts`. Keep Simplified Chinese and English labels in sync in
`locale.tsx`. Preserve compact layouts and check 390px and desktop widths.
Cover failure paths, cancellation, stale host-key approvals, cleanup, configuration
isolation and rolling traffic expiration. Use synthetic loopback SSH fixtures.

Use focused conventional commits. Do not revert unrelated user changes, amend commits,
rewrite history, publish releases or push to a remote unless requested. The initial
repository cleanup is local-only; GitHub synchronization requires a separate request.

## Security and Configuration

Never commit private keys, tokens, credentials or real runtime configurations.
Release defaults to `~/.ssh-tunnel/config.toml`, startup identity `ssh-tunnel`.
Debug keeps the existing isolated `.ssh-tunnel-migration` directory and startup name
for development-profile compatibility. Do not rename it or copy user data silently.
Test release binaries using explicit fixture paths or the synthetic-home harness,
never the real default home. Preserve user configuration and system SSH files.

`SSH_TUNNEL_CONFIG` and `--config` require absolute paths; `config.root` may redirect
the anchor. Exports contain plaintext credentials and require a warning. Startup
fixture overrides must stay in `ssh-tunnel-test-*`. Host trust must use an observed
backend challenge, reject stale/revoked keys and never silently disable verification.
Imported SSH commands are parsed, never executed.