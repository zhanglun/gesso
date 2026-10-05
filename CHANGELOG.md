# Changelog

All notable changes to Gesso are documented here. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versioning: [SemVer](https://semver.org/spec/v2.0.0.html) once 0.1 ships.

## [Unreleased]

### Added — 0.1.0 development milestones

**Engine**

- Dual-crate workspace: `gesso-core` (pure domain logic, 14 unit tests) + `gesso-app`.
- macOS wallpaper pinning: native `NSWindow` at `level = -2147483604`, click-through, all-spaces, transparent fallback to the system desktop (verified by a dedicated spike; GPUI windows are *not* used for wallpaper — see ARCHITECTURE).
- Wallpaper host pipeline: self-contained `file://` entries (host page copied per entry, relative media paths), `ContentSpec` query contract, `__gesso.pause/resume` JS API.
- Session manager: per-monitor sessions driven by the core state machine (`Idle→Loading→Playing→Paused…`), monitor diff sync, config + library persistence, single-instance lock.
- Cursor feed (M5): global mouse location routed to the display it is on → shader `iMouse` (30 Hz push, per-frame smoothing) and html `postMessage`; idle 5 min auto-downscale to 5 fps. No Input Monitoring permission needed — position via `NSEvent::mouseLocation`, buttons/idle via CoreGraphics HID source state (polling, not an event tap).
- Built-in `Cursor Glow` shader sample demonstrating mouse tracking.
- Import flow: extension validation (`mp4/webm/gif/webp/glsl/html`; `mkv`/HEVC rejected with actionable copy), source-extension preservation, random unguessable entry IDs.
- Tray: full menu (pause-all / cycle / manager / autostart / quit).
- First-run wizard wired to the real library; autostart via `auto-launch`.
- Thumbnail pipeline: AVFoundation frame extraction (generated objc2 bindings + ImageIO PNG encode, background `autoreleasepool`), `ThumbScheduler` (30s scan, in-flight dedup, 3 attempts/entry), import schedules extraction asynchronously.
- Hover preview on library cards: paused still-frame by default, extracted-frame carousel on hover.
- Shader renderer (M4 part 1): WebGL2 host branch with Shadertoy-subset uniforms (`iTime/iResolution/iMouse/iTimeDelta/iFrame` + `iChannel0..3` bound to runtime-generated 256×256 noise textures), `code=` base64url source param（file:// 下 fetch/XHR 被拦）, Shadertoy `mainImage` auto-wrap, RAF-timestamp fps gating, red-screen diagnostics on compile/link failure; 3 built-in samples (plasma / aurora / noiseflow) with static + hover-carousel thumbnails.
- HTML renderer (M4 part 2): the wallpaper's HTML (fixed at `wallpaper.html` — `index.html` stays the host page) loads in a sandboxed iframe (`sandbox="allow-scripts"` only, so the opaque origin rejects storage/IPC — hardware-verified DoD: `localStorage`/`sessionStorage`/`document.cookie`/`IndexedDB`/`window.top` all throw `SecurityError`, XHR is a `NetworkError`); pause/resume reach the frame via `postMessage {__gesso:"pause"|"resume"}`; built-in Clock sample as the contract reference.
- System data bridge (M5): fullscreen detection (`CGWindowList` layer-0 window covering a display frame, CG top-left ↔ AppKit bottom-left y-flip via primary display height) and battery state (IOKit `IOPSCopyPowerSourcesList`) drive the `Autopause` state machine per settings policy — pause / downscale-to-5fps (live `__gesso.setFps`: shader immediate, html advisory, video keeps playing) / ignore; user pause wins over auto-pause; hardware-verified (fullscreening the manager window pauses, exiting releases). Time feed: engine drives `__gesso.tick` at ~1 Hz so clock wallpapers tick from Rust events.
- Wallpaper Engine import I (M6): scans local Steam Workshop (`steamapps/workshop/content/431960/<id>/project.json`), imports `video` and `web` as **zero-copy read-only references** to the Steam source (`gesso://steam` route: video streamed via Range, main HTML injected with the WE API shim in-memory at the protocol layer — original files untouched, relative subresources same-origin). Project main file stored in `LibraryEntry.main_file`. **Multiple Steam libraries supported:** `steamapps/libraryfolders.vdf` is parsed so scanning and the `gesso://steam` route cover the main library plus every registered library (workshop content may live on different drives). Ordinary import also accepts `project.json`. Workshop entry hidden when Steam is absent (verified via a `STEAM_DIR` fixture). Legal boundary: reads only already-subscribed local content — no downloader, scraping, redistribution, or on-disk copy. `scene` rejected (M7), `application` never.
- Static image support: `jpg/jpeg/png/avif` added, new UI "Photo" category (still core `Image`; animated gif/webp distinguished via `is_animated_image_ext`).
- Content-type single source of truth (`gesso_core::content`): kind ↔ extensions ↔ MIME ↔ thumbnail strategy declared once in a `ContentType` table; import classification, main-asset discovery, protocol MIME and thumbnail dispatch all read it. `HostCommand` enum types the Rust→host JS commands (single `to_js` serialization, `WallpaperWindow::send`).

**UI (manager window, gpui-kit)**

- `gesso://` custom resource protocol restored to active (`bf3c294`): host page served from a single shared `gesso://host/index.html`, entry assets from `gesso://library/<id>/…`, cross-origin CORS, and HTTP Range(206) for video. Verified on all four kinds (image/video/html/shader).

- Library page (filter segments, search, cards, status bar, drag-to-monitor assignment overlay), monitors page (desktop-sandbox canvas + detail strip, per-monitor controls), settings page (persisted, applied live).
- Unified engine bridge: `AppState` global + `EngineAction` queue; snapshots preserve UI-local state.

**Project / docs**

- Feasibility spikes with verified conclusions (`SPIKE-REPORT.md`): GPUI integration, macOS desktop layering, Xcode-free builds via `runtime_shaders`.
- Engine API contract (`crates/app/API.md`), engineering notes (architecture rules + pitfall ledger), architecture doc, roadmap, FAQ, security policy, contribution guide, dual MIT/Apache-2.0 licensing.
- Product icon set ("gesso ground" concept, DESIGN.md §品牌图形): `mac/Gesso.icns` (16→1024), `win/gesso.ico` (16→256), tray glyphs (macOS template @2x / Windows white); tray placeholder pixels in `main.rs` replaced by embedded PNGs (template-aware on macOS), runtime Dock icon via `NSImage`; regen pipeline `crates/app/assets/icons/tools/build.mjs` + brand spec sheet `docs/design/06-品牌图标.brand.html`.

### Fixed

- Random SIGSEGV in `objc_release` during GCD autorelease-pool pop (two crashes with identical fingerprint, 2026-10-03/04): the thumbnail extractor hand-rolled ObjC refcounting on background tasks and violated ownership twice — an alloc+init object claimed by two `Retained::from_raw`, and the autoreleased `representationUsingType:properties:` result treated as +1 (over-released, its multi-MB VM region unmapped; the pool's stale record then faulted on release). Rewrote `thumb.rs`: generated objc2 bindings for AVFoundation (ownership in the type system), ImageIO `CGImageDestination` for PNG (pure C, no autoreleases), the whole job wrapped in an `autoreleasepool`, atomic per-frame writes (`.tmp` + rename), and a regression test running the real AVFoundation pipeline.
- Thumbnail scheduling hardening: missing-frame entries were retried forever every 30s (a persistently failing entry became an unbounded FFI churn and crash amplifier) and a `timer(2s)` guess refreshed the UI; extraction also ran synchronously on the main thread during import (~1s freeze per video). Now a dedicated 30s scan drives jobs through `engine::ThumbScheduler` (in-flight dedup + 3 attempts per entry per session), import schedules extraction asynchronously, and completion (`ThumbsDone`) triggers the snapshot refresh.
- Custom-scheme protocol restored: the M2 "zero callbacks / initial-navigation poison" conclusion no longer holds on lb-wry ≥0.53; the wallpaper webview now boots straight into a `gesso://host` URL (an `about:blank` first frame is no longer needed) and subresources resolve through the same handler.
- Static-image entry misidentified its host page as the main asset: an image directory contains both `index.html` (the host page, rewritten every launch) and `index.png`; `read_dir` could yield html first, and the old "any `index.*` fallback" in `main_asset_name` returned the host page → thumbnail/kind detection broke. Fixed at the root: `main_asset_name` only matches the kind's extension whitelist (`content_type(kind).extensions`), and html's main asset is fixed to `wallpaper.html`. The same table now backs import classification, validity and protocol MIME (previously six scattered extension lists).
- Rust↔host JS commands were hand-built strings (`__gesso&&__gesso.setFps(5)` across 8+ call sites, aligned by hand with the host page); replaced by the `HostCommand` enum with one serialization point and `WallpaperWindow::send`.
- Main loop decomposed (`apply_engine_action`/`merge_snapshot`/`focus_or_reopen_main`, named timing constants) and session stateless helpers moved to `encoding.rs`.
- Percent-encode library paths (`Application Support` contains a space).
- Import no longer renames `.webm`/`.webp` to `index.mp4`/`index.gif` (webviews type media by extension); import/validity/host-spec now share one `main_asset_name`.
- Tray "pause all" toggle used `AtomicBool::swap(true)` (only ever pauses) → `fetch_xor`.
- Tray "管理窗口…" click went dead after closing the window via the red dot (cached handle activation failed on a closed window) → rebuild the window automatically on activation failure.
- Settings rows: label and select were not vertically aligned — the kit Select's `size_full` root fills a fixed-height row while its trigger sits top-aligned inside; a `select_slot` wrapper (full-width, height collapsing to the trigger) restores row-level centering.
- Monitors-page FPS dropdown never took effect: its subscription updated only the UI-local projection and never enqueued `SetMonitorFps` (the action existed and was handled but had zero constructors; snapshots also hardcoded `fps: 60`). Now the dropdown enqueues the engine action (hot-reloads that session's host page, persists `monitor_fps`) and snapshots report the real per-display fps.
- UI hover state (`hovered` / `hover_frame`) was wiped by every engine snapshot回灌 — card hover highlight and preview carousel flickered on each refresh cycle; the merge now preserves them like the other UI-local browsing state.
- Library page could not scroll: the scroll container's `flex_1`/`min_h_0` sat under a non-flex wrapper, so its height was content-driven and never overflowed; the wrapper is now a flex container.
- Library grid now matches the prototype contract (`repeat(auto-fill, minmax(196px, 1fr))`): column count adapts to the window width and cards divide each row evenly instead of a fixed 208px width leaving a large trailing gap.
- Shader and HTML thumbnail capture: a persistent on-screen window (one level above the wallpaper, 2% alpha) runs the real host page and snapshots it via WKWebView — shader is frozen per frame via `__gessoSeek(t)`, html is sampled as live frames; captures run on one global serial queue and the capture window is never closed while a snapshot is pending (was an over-release SIGSEGV).
- Hover preview flicker went through three root causes: (1) the dark base color was attached to an `image_cache` element, which only forwards children and never paints its own style — moved to an outer div; (2) the `img` had no element id, so GPUI created no `ImgState` and the loading fallback was skipped entirely — fixed shared id; (3) the frame index advanced on a fixed timer while frames were still decoding (shader/html frames are large), so the preview alternated black/image and played catch-up at variable speed — hover is now two-phase: preload all 16 frames behind a spinner (`fetch_asset`, shared with the display cache), then play at a fixed 125 ms/frame.
- Library page scroll and responsive grid fixes (flex-container height chain; viewport-driven column count replacing the unavailable CSS `auto-fill`).
- Monitors-page redesign: desktop sandbox layout with a per-display detail bar.
- Compiler warnings in `gesso-app` cleared (32 → 0): mechanical fixes via `cargo fix`, `NSWindow::new`/`subviews`/`NSScreen::screens`/`setFrame_display` unsafe blocks adjusted to current objc2 bindings, deprecated `Retained::cast` → `downcast`, `NSColor::clearColor` safe call, dead leftover strings and helpers removed, the gesso:// protocol scaffolding and platform-window surface annotated as intentional (`#[allow(dead_code)]` with pointers to the protocol-fix todo / Windows M1).

### Known limitations (tracked in ROADMAP)

- Windows pinning not yet verified (needs a machine).
- WE video/web use zero-copy read-only references to the Steam source (`gesso://steam`); no on-disk copy is made. Older copied entries still resolve via `gesso://library`.

- iframe-internal navigation of HTML wallpapers is not allow-listed yet (`fps_cap` advisory for the html kind).
- Hover preview preload is per-process cache: the first hover shows a spinner briefly; subsequent hovers play immediately.
