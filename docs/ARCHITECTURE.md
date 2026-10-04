# Architecture

This document describes how Gesso actually works. Everything below was verified on macOS; items marked *planned* are not implemented yet.

## 1. Layers

```
┌─ UI layer (gpui-kit, GPUI) ──────────────────────────────┐
│  manager window (library / monitors / settings / wizard)  │
│  tray menus                                               │
├─ engine bridge ──────────────────────────────────────────┤
│  AppState global · EngineAction queue · snapshot_ui       │
├─ core services ──────────────────────────────────────────┤
│  SessionManager (state machine, per-monitor sessions)     │
│  content library · config persistence                     │
├─ platform layer (pin/) ──────────────────────────────────┤
│  macOS: AppKit NSWindow    Windows(planned): Win32        │
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

**Why not GPUI windows?** GPUI's window coordinator resets desktop-level full-screen geometry (it pushed `origin.y` to `-menuBarHeight` on every notification cycle), and its layout insets content by safe areas. Fighting it from outside loses — verified by a dedicated spike (`SPIKE-REPORT.md`, M1.5). Windows will mirror this: our own Win32 window + WebView2 child, `SetParent` onto `WorkerW`.

GPUI owns the manager window and the tray. Both run in the same process/event loop; AppKit windows created by `pin/` coexist with the GPUI app (creation order matters: after `gpui_kit::init`, before GPUI windows — otherwise tray-icon init panics on `NSApplication`).

## 3. Content pipeline

```
LibraryEntry (library/<random-id>/index.html + index.<ext>)
   → ContentSpec { kind, source, fit, fps_cap, audio, meta }
   → host page URL  file://…/index.html?spec=<urlencoded JSON>
   → wallpaper webview
```

- Each library entry is **self-contained**: the host page is copied into the entry directory at startup (dev behavior; frozen per-entry later) and media is referenced relatively. This sidesteps the currently-broken custom scheme (see pitfalls) and matches how Wallpaper Engine structures its projects.
- The **source extension is preserved on import** (`webm` stays `webm`): webviews type media by extension, so normalizing to `index.mp4` breaks playback. Import, validity checking and host-spec generation all call this `main_asset_name` helper — keep it that way.
- The host page exposes `window.__gesso.{pause, resume}`; the engine pauses via `evaluate_script` — pause is a **JS-level frame stop**, the window and webview stay resident.
- Renderer coverage today: video, image (gif/webp), shader (WebGL2 + Shadertoy subset, source via `code=` base64url), html (user page fixed at `wallpaper.html`, loaded in a sandboxed iframe `allow-scripts` — opaque origin throws `SecurityError` on storage/IPC; pause/resume delivered via `postMessage {__gesso:"pause"|"resume"}`).

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

Wallpaper content is untrusted code. Containment: webview sandbox, zero IPC from wallpaper windows, self-contained per-entry directories with random unguessable IDs, network-local content only. End-state (tracked): read-only `gesso://` scheme with traversal protection, CSP header injection and a navigation allow-list; current `file://` mode is an intermediate step. Details and reporting: [SECURITY.md](../SECURITY.md).

## 7. Performance notes

- Pause really stops work (JS loop + decoding); `Autopause` triggers (fullscreen/battery) are M5.
- Multi-monitor: one wallpaper window per display; webview content processes are shared by the platform, decoders are not (accepted).
- No frame-rate governor in the host page yet (M4, RAF gate with dynamic resolution scaling is specced).

## 8. Where the deeper history lives

- `SPIKE-REPORT.md` — the three feasibility spikes and 14 API facts extracted from them.
- `docs/ENGINEERING-NOTES.md` — binding rules + the pitfall ledger (15 entries).
- Design archive (interaction spec, visual tokens, prototype) is maintained alongside the project author; the repo carries its engineering conclusions.
