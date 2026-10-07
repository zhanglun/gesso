# Architecture

This document describes how Gesso actually works. Everything below was verified on macOS; items marked *planned* are not implemented yet.

## 1. Layers

```
┌─ UI layer (gpui-kit, GPUI) ──────────────────────────────┐
│  manager window (library / monitors / settings) + dialogs │
│  tray menus                                               │
├─ engine bridge ──────────────────────────────────────────┤
│  AppState global · EngineAction queue · snapshot_ui       │
├─ core services ──────────────────────────────────────────┤
│  SessionManager (state machine, per-monitor sessions)     │
│  content library · config persistence                     │
├─ platform layer (pin/) ──────────────────────────────────┤
│  macOS: AppKit NSWindow    Windows: Win32 + WorkerW        │
└─ content rendering ──────────────────────────────────────┘
   wallpaper window + wry webview (host page, sandboxed)
```

- `gesso-core` is **pure domain logic** (ContentSpec, config diffing, library manifest, state machine) — no platform or UI dependencies, fully unit-tested.
- `gesso-app` contains everything else. The **UI never touches engine internals** ([crates/app/API.md](../crates/app/API.md)): writes are queued as `EngineAction`s, reads come from `snapshot_ui`, and the snapshot merge preserves UI-local state (tab/query/selection/filter).

## 2. Window model

**Wallpaper windows are native windows owned by the pin layer, not GPUI windows.**

On macOS the wallpaper window is a plain `NSWindow` created with:

| Property | Value |
|---|---|
| style mask | borderless |
| level | `-2147483604` — strictly between the desktop picture and the icon layer (verified; values within the band behave identically, the desktop-picture level itself has non-deterministic ordering) |
| collection behavior | `CanJoinAllSpaces \| Stationary \| FullScreenAuxiliary \| IgnoresCycle` |
| mouse | `ignoresMouseEvents = true` (click-through to icons) |
| background | non-opaque + clear color → hiding the webview reveals the system desktop (this is how "fall back to the system wallpaper" works) |
| geometry | full screen frame including the menu-bar band |

The webview (`lb-wry`, the gpui-kit ecosystem fork of wry) is attached as a child of the window's content view and sized by us (no layout system involved).

**Why not GPUI windows?** GPUI's window coordinator resets desktop-level full-screen geometry (it pushed `origin.y` to `-menuBarHeight` on every notification cycle), and its layout insets content by safe areas. Fighting it from outside loses — verified by a dedicated spike (`SPIKE-REPORT.md`, M1.5). Windows mirrors this (M1, real-machine verified): our own Win32 window (`WS_POPUP`, toolwindow/no-activate, click-through via `WM_NCHITTEST → HTTRANSPARENT`) + WebView2 child, `SetParent` onto the spawned `WorkerW` (Progman `0x052C` ladder, Progman fallback when desktop icons are hidden, HWND_BOTTOM until explorer is ready). **Creation order is load-bearing: mount first, create the WebView2 second** — the DComp visual tree binds to the host hierarchy at controller creation, and the reverse order renders nothing (window tree looks healthy; verified by detaching the window mid-run, which instantly restores rendering). Explorer restarts destroy cross-process children, so `TaskbarCreated` (received by a hidden listener window) triggers a full window rebuild per session, not a re-parent. The process declares `PerMonitorV2` itself in `main()` (neither GPUI nor wry sets it) — physical pixels all the way.

GPUI owns the manager window and the tray. Both run in the same process/event loop; AppKit windows created by `pin/` coexist with the GPUI app (creation order matters: after `gpui_kit::init`, before GPUI windows — otherwise tray-icon init panics on `NSApplication`).

## 3. Content pipeline

```
LibraryEntry (library/<random-id>/index.<ext>)
   → ContentSpec { kind, source, fit, fps_cap, audio, meta }
   → host page URL  gesso://host/index.html?spec=<urlencoded JSON>
   → wallpaper webview   (source → gesso://library/<id>/<asset>)
```

- Each local library entry is **self-contained** on disk; the host page is a single shared copy served from `gesso://host/index.html` (no per-entry duplication) and entry media is referenced relatively.
- **Remote web entries** (`origin="url"`): the `https://` URL itself is stored in `LibraryEntry.source_url` (no local copy); the host page loads it directly in the sandboxed iframe. Thumbnails use the live-capture path (same as html).
- The **source extension is preserved on import** (`webm` stays `webm`): webviews type media by extension, so normalizing to `index.mp4` breaks playback. Import, validity checking and host-spec generation all call this `main_asset_name` helper — keep it that way.
- The host page exposes `window.__gesso`; commands are typed on the Rust side (`HostCommand` — Pause/Resume/SetFps/Tick/Mouse), serialized in exactly one place (`host_cmd.rs`) and delivered via `evaluate_script`. Pause is a **JS-level frame stop**; the window and webview stay resident.
- Renderer coverage today: video, image (gif/webp/jpg/jpeg/png/avif — animated gif/webp distinguished from static photos in the UI), shader (WebGL2 + Shadertoy subset, source via `code=` base64url), html (user page fixed at `wallpaper.html`, loaded in a sandboxed iframe `allow-scripts allow-same-origin` — same-scheme subresources load, storage of cross-origin frames throws `SecurityError`; pause/resume delivered via `postMessage {__gesso:"pause"|"resume"}`).
- The type ↔ extension ↔ MIME ↔ thumbnail-strategy knowledge lives in one table: `gesso_core::content` (`ContentType`). Import classification, main-asset discovery, protocol MIME and thumbnail dispatch all read it — no scattered extension lists.
- **Thumbnails are WYSIWYG captures, not translated previews**: strategy per kind is declared in the content table — static photos are `Direct` (reference the source file, zero generation), everything else is captured from a persistent near-invisible capture window running the real host page; output is one static `thumb.png` plus 15 hover frames under a shared contract, and captures run on one global serial queue. **Windows (M4-W):** the pipeline is webview-native — video via a shadow `crossorigin=anonymous` video drawn to canvas (`toDataURL`), shader via `preserveDrawingBuffer` + `#gl.toDataURL` (`__gessoSeek(t)` freeze), html via PrintWindow screen capture (the only platform seam) — see `capture_win.rs`. **macOS:** video still uses AVFoundation extraction and shader/html use WKWebView snapshots; once both platforms are verified against the shared pipeline, the AVFoundation path (and its objc2 crash surface) is slated for retirement.

## 4. Session state machine (gesso-core)

```
Idle ──Assign──▶ Loading ──Loaded──▶ Playing ⇄ PausedUser
                    │                   ⇅
                    └─LoadFailed─▶ Error   Autopause (fullscreen / battery)
Error ──Reselect──▶ Idle     any ──Quit──▶ Stopped
```

Pure function `transfer(state, event)`; every transition is unit-tested. Sessions are per-monitor (stable ID = `cg-<CGDirectDisplayID>` on macOS; EDID hashing is the upgrade path).

## 5. Engine ↔ UI data flow

- **Writes**: UI/tray enqueue `EngineAction` (`Assign`, `PauseOne`, `PauseAll`, `Import`, `UpdateSettings`, `SetAutostart`, `CycleMain`, `SyncMonitors`, `FocusMainWindow`). One poller (150 ms) executes them against the `SessionManager` — single writer, no UI-side optimistic updates (a failed assign must run the engine's fallback path).
- **Reads**: `snapshot_ui(&sm)` projects engine state into the UI global after any action and on monitor changes (~2 s diff poll for hot-plug).
- Menu/tray events arrive on handlers without GPUI context — everything funnels through the queue. (Toggling booleans in handlers must use `fetch_xor`, not `swap(true)`: see pitfalls.)

## 6. Security model

Wallpaper content is untrusted code. Containment: webview sandbox, zero IPC from wallpaper windows, per-entry directories with random unguessable IDs, network-local content only. Read-only `gesso://` scheme is live: `host` serves the shared host page, `library/<id>/…` serves entry files with traversal protection, CSP header injection, CORS for cross-origin subresources, and Range(206) for video. Details and reporting: [SECURITY.md](../SECURITY.md).

## 7. Performance notes

- Pause really stops work (JS loop + decoding). Fullscreen/battery triggers drive the state machine's `Autopause` state (M5, verified), with a per-display fullscreen-downscale policy (5 fps target via `__gesso.setFps`).
- Multi-monitor: one wallpaper window per display; webview content processes are shared by the platform, decoders are not (accepted).
- Frame-rate control: the engine pushes the per-display fps cap to the host page (`__gesso.setFps`, live without reload) — shader frames are RAF-gated; for html it is an advisory value since the iframe owns its own RAF; video has no frame-rate lever. Cursor feed: the engine routes the global mouse location to the display it is on and pushes it at 30 Hz (`__gesso.mouse`); the host stores only the latest target and smooths per-frame into `iMouse` (read on the main thread — background reads of `NSEvent.mouseLocation` are stale during fast movement). Idle 5 min → 5 fps.

## 8. Where the deeper history lives

- `SPIKE-REPORT.md` — the three feasibility spikes and 14 API facts extracted from them.
- `docs/ENGINEERING-NOTES.md` — binding rules + the pitfall ledger (~20 entries).
- Design archive (interaction spec, visual tokens, prototype) is maintained alongside the project author; the repo carries its engineering conclusions.
