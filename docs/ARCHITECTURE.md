# Architecture

This document describes the current Gesso implementation. Future work belongs in [ROADMAP.md](ROADMAP.md).

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
   wallpaper window — wry webview host page for non-video; native rendering for video
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
| background | opaque + black — the window is always fully covered by content; transparency bought nothing and cost a full-screen alpha blend per frame in WindowServer. "Fall back to the system desktop" = hiding the whole window (`set_visible(false)` → `orderOut`), not the webview |
| geometry | full screen frame including the menu-bar band |

The webview (`lb-wry`, the gpui-kit ecosystem fork of wry) is attached as a child of the window's content view and sized by us (no layout system involved).

**Video wallpapers use native rendering on both platforms.** `pin::MacVideoWindow` uses `AVQueuePlayer`, `AVPlayerLooper`, and `AVPlayerLayer`; `pin::WinVideoWindow` uses Media Foundation and a flip-model DXGI swap chain. Shader, HTML, and image wallpapers use the webview. Video windows overscan the monitor frame by 2 pt to hide sampling seams at aspect-fill boundaries.

**Why not GPUI windows?** Its coordinator and safe-area layout do not preserve desktop-level geometry. On Windows, Gesso uses a Win32 popup mounted below icons on WorkerW. Mount before creating WebView2; Explorer restarts require a full window rebuild. The process declares PerMonitorV2 DPI awareness before any window is created.

GPUI owns the manager window and the tray. Both run in the same process/event loop; AppKit windows created by `pin/` coexist with the GPUI app (creation order matters: after `gpui_kit::init`, before GPUI windows — otherwise tray-icon init panics on `NSApplication`).

**Display reconfiguration is event-driven convergence, not polling discovery.** Platform callbacks only increment a topology epoch. The engine debounces the epoch, compares each session's applied frame with the enumerated monitors, hides stale windows, then reapplies geometry and pinning once the topology is stable. A short periodic patrol reasserts unchanged sessions as a recovery path for missed events or platform mutations.

**Transition frames (`desktop_bridge.rs`): runtime never changes the OS desktop picture.** Before hiding a geometry-stale macOS wallpaper window, Gesso may show a same-level transition window with a predecoded static frame. It is removed after the real wallpaper becomes visible. Startup performs one-time cleanup only if an older Gesso version left a bridge image as the system desktop. Image entries without an available decoded frame may briefly reveal the user's desktop.

## 3. Content pipeline

```
LibraryEntry
   → macOS video: pin::MacVideoWindow + AVPlayerLayer (loopback media stream, no host page)
   → Windows video: pin::WinVideoWindow + MF SourceReader → flip-model swapchain (local file, no host page)
   → everything else (image / shader / html):
     ContentSpec { kind, source, fit, fps_cap, audio, meta }
     → host page URL  gesso://host/index.html?spec=<urlencoded JSON>
     → wallpaper webview
        assets → gesso://library/<id>/<asset> (remote url entries load the https:// page in the iframe)
```

- **Video bytes never stream through the custom scheme on any platform.** WKWebView/WebView2 load media over custom schemes via small fragmented Range requests relayed through the WebContent process (measured: ~92 req/s of 2–4 KB chunks on dual screens) — `protocol::start_media_server` serves `library`/`steam` routes over a loopback-only HTTP socket so AVPlayer uses its native streaming path (one socket, whole-file reads; ~6 requests total). Path resolution and traversal guards are shared with the scheme handler (`resolve()`); CSP is generated per-boot with the random port (`csp()`).
- **Looping video is served from RAM (≤256 MB per entry).** Players re-read the whole file every loop (AVPlayerLooper clones the item each cycle; web engines don't cache custom-scheme media), and OS page cache is evictable — on memory-pressured machines a 20 MB wallpaper hits disk every loop (measured in the wild). The media server pins entries ≤256 MB in RAM on first read and slices every Range request from memory; larger entries keep streaming from disk. The native macOS video window uses the loopback URL.

- Each local library entry is **self-contained** on disk; the host page is a single shared copy served from `gesso://host/index.html` (no per-entry duplication) and entry media is referenced relatively.
- **Remote web entries** (`origin="url"`): the `https://` URL itself is stored in `LibraryEntry.source_url` (no local copy); the host page loads it directly in the sandboxed iframe. Thumbnails use the live-capture path (same as html).
- The **source extension is preserved on import** (`webm` stays `webm`): webviews type media by extension, so normalizing to `index.mp4` breaks playback. Import, validity checking and host-spec generation all call this `main_asset_name` helper — keep it that way.
- The host page exposes `window.__gesso`; commands are typed on the Rust side (`HostCommand` — Pause/Resume/SetFps/Tick/Mouse), serialized in exactly one place (`host_cmd.rs`) and delivered via `evaluate_script`. Pause is a **JS-level frame stop**; the window and webview stay resident.
- Renderer coverage today: video, image (gif/webp/jpg/jpeg/png/avif — animated gif/webp distinguished from static photos in the UI), shader (WebGL2 + Shadertoy subset, source via `code=` base64url), html (user page fixed at `wallpaper.html`, loaded in a sandboxed iframe `allow-scripts allow-same-origin` — same-scheme subresources load, storage of cross-origin frames throws `SecurityError`; pause/resume delivered via `postMessage {__gesso:"pause"|"resume"}`).
- The type ↔ extension ↔ MIME ↔ thumbnail-strategy knowledge lives in one table: `gesso_core::content` (`ContentType`). Import classification, main-asset discovery, protocol MIME and thumbnail dispatch all read it — no scattered extension lists.
- **Thumbnails are WYSIWYG captures, not translated previews**: static photos use their source directly; other types produce one `thumb.png` and 15 hover frames on a global serial queue. Windows captures through `capture_win.rs`; macOS uses AVFoundation for video and WKWebView snapshots for shader and HTML.

## 4. Session state machine (gesso-core)

```
Idle ──Assign──▶ Loading ──Loaded──▶ Playing ⇄ PausedUser
                    │                   ⇅
                    └─LoadFailed─▶ Error   Autopause (fullscreen / battery)
Error ──Reselect──▶ Idle     any ──Quit──▶ Stopped
```

Pure function `transfer(state, event)`; every transition is unit-tested. Sessions are per-monitor. On macOS, IDs prefer the EDID vendor/model/serial tuple, fall back to `cg-<CGDirectDisplayID>` when unavailable, and append the CG ID to disambiguate identical displays. Windows prefers the monitor PnP identity and falls back to `win-<DISPLAYn>`; wake/reconnect still has geometry remapping as a fallback.

## 5. Engine ↔ UI data flow

- **Writes**: UI/tray enqueue `EngineAction` — the authoritative action set lives in [API.md](../crates/app/API.md) (assign / pause / import / settings / update-check and friends). One poller (150 ms) executes them against the `SessionManager` — single writer, no UI-side optimistic updates (a failed assign must run the engine's fallback path).
- **Reads**: `snapshot_ui(&sm)` projects engine state into the UI global after any action and on topology changes (epoch-driven convergence — see §2).
- Menu/tray events arrive on handlers without GPUI context — everything funnels through the queue. (Toggling booleans in handlers must use `fetch_xor`, not `swap(true)` — see ENGINEERING-NOTES §2.)

## 6. Security model

Wallpaper content is untrusted code. Containment: webview sandbox, zero IPC from wallpaper windows, per-entry directories with random unguessable IDs, local content plus user-imported `https://` URLs (remote web wallpapers — nothing is fetched beyond what the user assigns). Read-only `gesso://` scheme is live: `host` serves the shared host page, `library/<id>/…` serves entry files with traversal protection, CSP header injection, CORS for cross-origin subresources, and Range(206) for video. Details and reporting: [SECURITY.md](../SECURITY.md).

## 7. Performance notes

- Pause really stops work (JS loop + decoding; native video: `player.pause()`). Fullscreen/battery triggers drive the state machine's `Autopause` state, with a per-display fullscreen-downscale policy (5 fps target via `__gesso.setFps`).
- Multi-monitor: one wallpaper window per display; webview content processes are shared by the platform, decoders are not (accepted).
- Frame pacing: the host page counts presented video frames (`requestVideoFrameCallback`) and page ticks (rAF); `GESSO_VIDEO_DIAG=1` samples them for webview playback. Native video windows have no frame probe yet — their diagnosis rides on engine logs.
- Frame-rate control: the engine pushes the per-display fps cap to the host page (`__gesso.setFps`, live without reload) — shader frames are RAF-gated; for html it is an advisory value since the iframe owns its own RAF; video has no frame-rate lever. Cursor feed: the engine routes the global mouse location to the display it is on and pushes it at 30 Hz (`__gesso.mouse`); the host stores only the latest target and smooths per-frame into `iMouse` (read on the main thread — background reads of `NSEvent.mouseLocation` are stale during fast movement). Idle 5 min → 5 fps.

## 8. Where the deeper history lives

- [ENGINEERING-NOTES.md](ENGINEERING-NOTES.md) — current engineering rules and debugging paths.
- [design/README.md](design/README.md) — current interaction spec, tokens, prototype, diagram sources, and archive boundary.
