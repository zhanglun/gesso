# Roadmap

Status reflects what actually runs on hardware: ✅ means it was verified on a real machine. Time estimates assume spare-time pace.

## Where we are

```
✅ M0    workspace skeleton + CI
✅ M0.5  GPUI integration spike (tray × GPUI loop, native handles, theme tokens)
✅ M1.5  macOS desktop layering spike (pin below icons, click-through, all-spaces)
✅ M2    video wallpaper end-to-end (pin + pause/resume + persistence)
✅ M3    sessions/config/UI engine + UI↔engine actions + import + settings + first-run
✅ M4    renderer completeness — video / image / shader (WebGL2 + Shadertoy subset) / HTML (sandboxed iframe)
⬜ M5    system data bridge — fullscreen/battery auto-pause, cursor/time feeds
⬜ M1    Windows pinning (needs a Windows machine)
⬜ M6    Wallpaper Engine import I (video/web)
⬜ M7    Wallpaper Engine import II (scene, long-term)
```

## Next up, in priority order

### M5 — system data bridge
- Fullscreen detection (macOS `NSWorkspace` notifications) and battery policy → drives the `Autopause` state; cursor/time feeds for interactive wallpapers (global cursor polling — desktop-level windows receive no mouse events).
- DoD: entering a fullscreen app pauses with zero GPU cost; clock wallpaper keeps ticking from Rust-driven events.

### M1 — Windows pinning
- Own Win32 window + WebView2 child (`GPUI_DISABLE_DIRECT_COMPOSITION=1`), `SetParent` onto `WorkerW`, icon-hidden fallback path, `TaskbarCreated` re-pin, DPI/multi-monitor placement.
- Blocked on: a Windows machine (hardware or GUI-capable VM).

### M6 — Wallpaper Engine import I
- Workshop scan (`steamapps/workshop/content/431960/<id>/project.json`), `video`/`web` types; WE web-API shim (`wallpaperRegisterAudioListener` etc.).
- GPL boundary: invoke `depkg-cli` as a subprocess or accept project-wide GPL (decision pending, see [docs/WALLPAPER-ENGINE.md](docs/WALLPAPER-ENGINE.md)).

## Known gaps (no milestone yet)

- `gesso://` custom scheme non-functional on current webview stack (zero callbacks; serving via self-contained `file://` entries meanwhile).
- `hovered` UI state reset by 150 ms snapshots → in-window hover highlight flickers; fix together with the real-machine monitor-outline window (signature #1).
- Real-machine monitor outline (signature #1) and tray panel positioning on retina displays.
- ~25 compiler warnings in `gesso-app`; example `m15.rs` is a reference, not a product surface.
- HTML thumbnails use the gradient placeholder (the webview capture pipeline currently covers shader/video only).
- HTML wallpapers: iframe-internal navigation is not allow-listed yet (top-navigation / popups / forms are sandbox-blocked; `fps_cap` is advisory for the html kind).
- i18n: UI strings are centralized in `ui/strings.rs` (Chinese-first); English translation pass pending.
- Tray polish: dynamic menu copy (muda handlers are `Send`-only — menu handle can't be mutated from the poller) and precise quick-panel positioning relative to the tray icon (retina coordinate conversion).
