# Gesso

**A cross-platform dynamic wallpaper engine for Windows and macOS.**
Videos, GIFs, shaders and web pages pinned *behind* your desktop icons — written in Rust with [GPUI](https://gpui.rs).

[AGENTS.md](AGENTS.md)（AI 会话指南） · [简体中文](README.zh-CN.md) · [Architecture](docs/ARCHITECTURE.md) · [Roadmap](docs/ROADMAP.md) · [FAQ](docs/FAQ.md) · [Contributing](CONTRIBUTING.md)

> **Status: early development (0.1.x), macOS working, Windows in progress.**
> Design and engineering decisions are documented in-repo; everything below is verified on real hardware unless marked otherwise.

---

## What it does

- **Pinned behind the icons** — the wallpaper window sits *below* the desktop icon layer: your icons stay visible, stay clickable, and keep working.
- **One host pipeline, four content kinds** — video (`mp4`/`webm`), animated images (`gif`/`webp`), shaders (`glsl`, Shadertoy-style) and web pages (`html`). The host page is the single contract; renderers are pluggable.
- **Real pause** — pausing stops the JS render loop and video decoding, not just the visuals. Full-screen apps and battery mode will pause automatically (see [Roadmap](docs/ROADMAP.md)).
- **Tray-first UX** — pause/resume and switch wallpapers from the menu bar; open the manager window only when you want to manage content.
- **No IPC into the app** — wallpaper content is treated as untrusted: it runs in a webview sandbox and can only read its own asset folder.

## Platform status

| Platform | Wallpaper pinning | Notes |
|---|---|---|
| **macOS** | ✅ working | `NSWindow` below the icon layer (`level = -2147483604`), click-through, all-spaces; verified on macOS with a full-screen video wallpaper |
| **Windows** | 🚧 in progress | Planned via `WorkerW` + `SetParent` on our own Win32 window + WebView2 child. Needs a Windows machine to verify |
| **Linux** | ❌ out of scope (v1) | KDE/GNOME/X11/Wayland would each need a separate desktop-integration path — see [FAQ](docs/FAQ.md) |

| Feature | Status |
|---|---|
| Video wallpaper, pinned & full-screen | ✅ |
| Pause / resume from tray (stops decoding, not just visuals) | ✅ |
| Config persistence (restores on launch) | ✅ |
| Monitor enumeration with stable IDs (`cg-<display-id>`) | ✅ |
| Manager UI (library / monitors / settings) | ✅ |
| Import files (dialog + drag & drop) | ✅ |
| Settings persistence, autostart | ✅ |
| First-run wizard | ✅ |
| Shader renderer (Shadertoy subset, WebGL2) | ✅ |
| HTML renderer (sandboxed iframe, storage/IPC denied) | ✅ |
| System data bridge (fullscreen/battery auto-pause · time feed) | ✅ M5; cursor/audio later |
| Wallpaper Engine workshop assets (read-only) | 🚧 planned (M6) — see [docs/WALLPAPER-ENGINE.md](docs/WALLPAPER-ENGINE.md) |

## Requirements

- **macOS**: Command Line Tools are enough — **Xcode is not required** (Metal shaders are compiled at runtime via the `runtime_shaders` feature in the GPUI snapshot we depend on).
- **Windows**: MSVC toolchain (untested at this stage).
- **Rust**: recent stable (developed on 1.95).

## Quick start

```bash
git clone <this repo> && cd gesso
cargo run -p gesso-app          # tray icon + manager window + wallpaper
```

First launch registers a built-in sample wallpaper and assigns it to the main display. Everything the app creates lives under:

```
~/Library/Application Support/Gesso/     # macOS   (Windows: %USERPROFILE%\.gesso)
├── config.json         # monitor → wallpaper mapping + settings
└── library/
    └── <entry-id>/     # self-contained: index.html (host page) + index.<ext> (asset)
```

Useful commands:

```bash
cargo test  -p gesso-core -p gesso-app    # logic tests (fast)
cargo clippy -p gesso-core --all-targets -- -D warnings
pkill -f "target/debug/gesso"             # quit (single-instance lock)
```

## Architecture at a glance

```
tray + manager window ── GPUI ──────────┐
                                        │  EngineAction queue (UI → engine)
                                        ▼
                              ┌──────────────────────┐
                              │   SessionManager      │  one session per monitor
                              │   state machine       │  Idle→Loading→Playing→Paused…
                              └───────┬──────────────┘
                     pin layer (platform)         content library + config
                              │
              ┌───────────────▼───────────────────────────┐
              │  wallpaper window = native window          │  macOS: AppKit NSWindow
              │  + wry webview (host page, sandboxed)      │  Windows: Win32 (planned)
              └───────────────────────────────────────────┘
```

Key rule: **wallpaper windows are native windows we own, not GPUI windows.** GPUI manages the manager window and tray only — the macOS layering spike showed GPUI's window coordinator keeps rewriting desktop-level fullscreen window geometry, so wallpaper windows bypass it entirely. Details: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) · engine API: [crates/app/API.md](crates/app/API.md) · engineering rules & pitfalls: [docs/ENGINEERING-NOTES.md](docs/ENGINEERING-NOTES.md).

## Documentation

| Doc | Contents |
|---|---|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Layers, window model, pinning parameters, content pipeline, state machine, security |
| [docs/ENGINEERING-NOTES.md](docs/ENGINEERING-NOTES.md) | Architecture rules, 15 verified pitfalls, key paths, commands |
| [crates/app/API.md](crates/app/API.md) | The interface UI code uses to talk to the engine |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Milestones, current status, known gaps |
| [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) | Local development & debugging guide (logs, diagnostics, troubleshooting) |
| [docs/design/技术方案.md](docs/design/技术方案.md) | Full engineering plan, 14 chapters (Chinese) |
| [docs/WALLPAPER-ENGINE.md](docs/WALLPAPER-ENGINE.md) | Workshop asset compatibility plan and its legal boundary |
| [docs/FAQ.md](docs/FAQ.md) | Linux? Xcode? telemetry? uninstall? |
| [SPIKE-REPORT.md](SPIKE-REPORT.md) | Verified results of the three feasibility spikes |

## Credits & prior art

- [Lively Wallpaper](https://github.com/rocksdanister/lively) — reference for the Windows `WorkerW` approach.
- [Plash](https://github.com/sindresorhus/Plash) (historical, MIT) — reference for macOS desktop-level window placement.
- [gpui-kit](https://github.com/longbridge/gpui-kit) / [Zed's GPUI](https://github.com/zed-industries/zed) — UI framework and components.
- [wry](https://github.com/tauri-apps/wry) (via `lb-wry`) — webview embedding.
- Icons: [Lucide](https://lucide.dev) via `gpui-kit-assets`.

## License

Dual-licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.

Wallpaper Engine is a trademark of its respective owner. Gesso is not affiliated with Wallpaper Engine and does not bundle, download or redistribute workshop content.
