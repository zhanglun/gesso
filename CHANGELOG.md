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
- Thumbnail pipeline: AVFoundation frame extraction (generated objc2 bindings + ImageIO PNG encode, background `autoreleasepool`), `ThumbScheduler` (30s scan, in-flight dedup, 3 attempts/entry), import schedules extraction asynchronously.
- Hover preview on library cards: paused still-frame by default, extracted-frame carousel on hover.
- Shader renderer (M4 part 1): WebGL2 host branch with Shadertoy-subset uniforms (`iTime/iResolution/iMouse/iTimeDelta/iFrame` + `iChannel0..3` bound to runtime-generated 256×256 noise textures), `code=` base64url source param（file:// 下 fetch/XHR 被拦）, Shadertoy `mainImage` auto-wrap, RAF-timestamp fps gating, red-screen diagnostics on compile/link failure; 3 built-in samples (plasma / aurora / noiseflow) with static + hover-carousel thumbnails.

**UI (manager window, gpui-kit)**

- Library page (filter segments, search, cards, status bar, drag-to-monitor assignment overlay), monitors page (desktop-sandbox canvas + detail strip, per-monitor controls), settings page (persisted, applied live).
- Unified engine bridge: `AppState` global + `EngineAction` queue; snapshots preserve UI-local state.

**Project / docs**

- Feasibility spikes with verified conclusions (`SPIKE-REPORT.md`): GPUI integration, macOS desktop layering, Xcode-free builds via `runtime_shaders`.
- Engine API contract (`crates/app/API.md`), engineering notes (architecture rules + pitfall ledger), architecture doc, roadmap, FAQ, security policy, contribution guide, dual MIT/Apache-2.0 licensing.
- Product icon set ("gesso ground" concept, DESIGN.md §品牌图形): `mac/Gesso.icns` (16→1024), `win/gesso.ico` (16→256), tray glyphs (macOS template @2x / Windows white); tray placeholder pixels in `main.rs` replaced by embedded PNGs (template-aware on macOS), runtime Dock icon via `NSImage`; regen pipeline `crates/app/assets/icons/tools/build.mjs` + brand spec sheet `docs/design/06-品牌图标.brand.html`.

### Fixed

- Random SIGSEGV in `objc_release` during GCD autorelease-pool pop (two crashes with identical fingerprint, 2026-10-03/04): the thumbnail extractor hand-rolled ObjC refcounting on background tasks and violated ownership twice — an alloc+init object claimed by two `Retained::from_raw`, and the autoreleased `representationUsingType:properties:` result treated as +1 (over-released, its multi-MB VM region unmapped; the pool's stale record then faulted on release). Rewrote `thumb.rs`: generated objc2 bindings for AVFoundation (ownership in the type system), ImageIO `CGImageDestination` for PNG (pure C, no autoreleases), the whole job wrapped in an `autoreleasepool`, atomic per-frame writes (`.tmp` + rename), and a regression test running the real AVFoundation pipeline.
- Thumbnail scheduling hardening: missing-frame entries were retried forever every 30s (a persistently failing entry became an unbounded FFI churn and crash amplifier) and a `timer(2s)` guess refreshed the UI; extraction also ran synchronously on the main thread during import (~1s freeze per video). Now a dedicated 30s scan drives jobs through `engine::ThumbScheduler` (in-flight dedup + 3 attempts per entry per session), import schedules extraction asynchronously, and completion (`ThumbsDone`) triggers the snapshot refresh.
- Webview initial-navigation poison: a failed custom-scheme first URL leaves WKWebView in a "URL updates but never paints" state — initial navigation is now `about:blank`.
- Percent-encode library paths (`Application Support` contains a space).
- Import no longer renames `.webm`/`.webp` to `index.mp4`/`index.gif` (webviews type media by extension); import/validity/host-spec now share one `main_asset_name`.
- Tray "pause all" toggle used `AtomicBool::swap(true)` (only ever pauses) → `fetch_xor`.
- Tray "管理窗口…" click went dead after closing the window via the red dot (cached handle activation failed on a closed window) → rebuild the window automatically on activation failure.
- Settings rows: label and select were not vertically aligned — the kit Select's `size_full` root fills a fixed-height row while its trigger sits top-aligned inside; a `select_slot` wrapper (full-width, height collapsing to the trigger) restores row-level centering.

### Known limitations (tracked in ROADMAP)

- Windows pinning not yet verified (needs a machine).
- `gesso://` custom scheme non-functional on the current webview stack; entries load via `file://`.
- HTML renderer (M4 part 2), system data bridge (fullscreen/battery pause detection) pending.
- `hovered` state is reset by snapshots (~150 ms) — in-window hover highlight may flicker.
