# Changelog

All notable changes to Gesso are documented here. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versioning: [SemVer](https://semver.org/spec/v2.0.0.html) once 0.1 ships.

## [Unreleased]

### Added — 0.1.0 development milestones

**Engine**

- Dual-crate workspace: `gesso-core` (pure domain logic, 14 unit tests) + `gesso-app`.
- macOS wallpaper pinning: native `NSWindow` at `level = -2147483604`, click-through, all-spaces, transparent fallback to the system desktop (verified by a dedicated spike; GPUI windows are *not* used for wallpaper — see ARCHITECTURE).
- Wallpaper host pipeline: self-contained `file://` entries (host page copied per entry, relative media paths), `ContentSpec` query contract, `__gesso.pause/resume` JS API.
- Session manager: per-monitor sessions driven by the core state machine (`Idle→Loading→Playing→Paused…`), monitor diff sync, config + library persistence, single-instance lock.
- Import flow: extension validation (`mp4/webm/gif/webp/glsl/html`; `mkv`/HEVC rejected with actionable copy), source-extension preservation, random unguessable entry IDs.
- Tray: full menu (pause-all / cycle / manager / autostart / quit).
- First-run wizard wired to the real library; autostart via `auto-launch`.

**UI (manager window, gpui-kit)**

- Library page (filter segments, search, cards, status bar, drag-to-monitor assignment overlay), monitors page (topology, per-monitor controls), settings page (persisted, applied live).
- Unified engine bridge: `AppState` global + `EngineAction` queue; snapshots preserve UI-local state.

**Project / docs**

- Feasibility spikes with verified conclusions (`SPIKE-REPORT.md`): GPUI integration, macOS desktop layering, Xcode-free builds via `runtime_shaders`.
- Engine API contract (`crates/app/API.md`), engineering notes (architecture rules + pitfall ledger), architecture doc, roadmap, FAQ, security policy, contribution guide, dual MIT/Apache-2.0 licensing.
- Product icon set ("gesso ground" concept, DESIGN.md §品牌图形): `mac/Gesso.icns` (16→1024), `win/gesso.ico` (16→256), tray glyphs (macOS template @2x / Windows white); tray placeholder pixels in `main.rs` replaced by embedded PNGs (template-aware on macOS), runtime Dock icon via `NSImage`; regen pipeline `crates/app/assets/icons/tools/build.mjs` + brand spec sheet `docs/design/06-品牌图标.brand.html`.

### Fixed

- Webview initial-navigation poison: a failed custom-scheme first URL leaves WKWebView in a "URL updates but never paints" state — initial navigation is now `about:blank`.
- Percent-encode library paths (`Application Support` contains a space).
- Import no longer renames `.webm`/`.webp` to `index.mp4`/`index.gif` (webviews type media by extension); import/validity/host-spec now share one `main_asset_name`.
- Tray "pause all" toggle used `AtomicBool::swap(true)` (only ever pauses) → `fetch_xor`.

### Known limitations (tracked in ROADMAP)

- Windows pinning not yet verified (needs a machine).
- `gesso://` custom scheme non-functional on the current webview stack; entries load via `file://`.
- Shader/HTML renderers, system data bridge (fullscreen/battery pause detection) pending.
- `hovered` state is reset by snapshots (~150 ms) — in-window hover highlight may flicker.
