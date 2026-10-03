# Contributing to Gesso

Thanks for your interest! Gesso is a young project — the fastest way to help is picking an unchecked item from the [roadmap](docs/ROADMAP.md) and reading the rules below before your first PR. 中文交流完全没问题（issues / PR / 代码注释均可）。

## Getting started

```bash
git clone <repo> && cd gesso
cargo test -p gesso-core -p gesso-app        # must pass
cargo run -p gesso-app                        # tray + manager window + wallpaper
```

- **macOS**: Command Line Tools only — no Xcode needed.
- **Rust**: recent stable.

## Read these first (they exist to prevent rework)

1. **[docs/ENGINEERING-NOTES.md](docs/ENGINEERING-NOTES.md)** — 5 architecture rules + 15 verified pitfalls. The rules are binding; the pitfall table exists because each entry cost us a debugging session.
2. **[crates/app/API.md](crates/app/API.md)** — the only interface UI code may use to talk to the engine.
3. **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)** — what lives where and why.
4. **[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)** — local dev & debugging guide (logging, diagnostics, troubleshooting table).

The two rules that have caused the most rework historically:

- **Wallpaper windows are native windows (`pin/`), never GPUI windows.** GPUI's window coordinator resets desktop-level full-screen geometry; this was established by a dedicated spike and re-litigating it wastes everyone's time.
- **UI never touches the engine directly** — write actions go through the `EngineAction` queue; reads go through the snapshot, preserving UI-local state (tab/query/selection).

## Code map

```
crates/core    pure domain logic: ContentSpec, config, library, state machine (unit-tested, no platform deps)
crates/app     the application:
  ├─ pin/        platform wallpaper windows (macOS: AppKit; Windows: planned)
  ├─ protocol.rs asset paths & the gesso:// scheme (currently file:// self-contained entries)
  ├─ session.rs  SessionManager — the single orchestrator
  ├─ engine.rs   AppState global + EngineAction queue
  └─ ui/         manager window (gpui-kit): library / monitors / settings / first-run
```

## Pull request checklist

- [ ] `cargo check -p gesso-app` passes
- [ ] `cargo test -p gesso-core -p gesso-app` passes; new logic ships with a test (assert-based, no framework)
- [ ] `cargo clippy -p gesso-core --all-targets -- -D warnings` passes
- [ ] UI changes don't call anything outside `crates/app/API.md`
- [ ] UI **behavior/appearance** changes reference the frozen interaction spec (see below) — if you're changing behavior, say so in the PR and describe what the new contract is
- [ ] You've run the app and exercised what you changed

Commit style: short imperative subject, body explains *why* if non-obvious. One logical change per commit.

## Design changes

The UI's visual contract is a frozen interaction spec + prototype (tokens, six-state components, copy rules). If you don't have access to the design archive, describe the intended behavior in the issue/PR and a maintainer will confirm it against the spec.

## Reporting bugs

Use the issue templates. Performance reports should include the wallpaper type, resolution and Activity Monitor / Task Manager numbers for the GPU and window-server processes. If `screencapture` can't see the wallpaper window, check Screen Recording permission for your terminal first (see FAQ).

## Security

See [SECURITY.md](SECURITY.md) — do not open public issues for vulnerabilities.
