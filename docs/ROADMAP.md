# Roadmap

Status reflects what actually runs on hardware: ✅ means it was verified on a real machine. Time estimates assume spare-time pace.

## Where we are

```
✅ M0    workspace skeleton + CI
✅ M0.5  GPUI integration spike (tray × GPUI loop, native handles, theme tokens)
✅ M1.5  macOS desktop layering spike (pin below icons, click-through, all-spaces)
✅ M2    video wallpaper end-to-end (pin + pause/resume + persistence)
✅ M3    sessions/config/UI engine + UI↔engine actions + import + settings + first-run
✅ M4    renderer completeness — video / image / shader (WebGL2 + Shadertoy subset) / HTML (sandboxed iframe); all four kinds have real webview-captured thumbnails (static + 15-frame hover sequence)
✅ M5    system data bridge — fullscreen/battery auto-pause + time feed + cursor feed (iMouse) + idle downscale
⬜ M1    Windows pinning (needs a Windows machine)
⬜ M6    Wallpaper Engine import I (video/web)
⬜ M7    Wallpaper Engine import II (scene, long-term)
```

## Next up, in priority order

### M5 — system data bridge（✅ 已完成）
- ✅ fullscreen detection: `CGWindowList` layer-0 window covering a display frame（±3pt 容差，y 轴按主显示器高度翻转）→ 按全屏策略执行：暂停 / 降帧到 5 fps / 忽略。
- ✅ battery: IOKit power sources（`IOPSCopyPowerSourcesList`）。电池供电作用于全部显示器，本屏全屏优先。
- ✅ time feed: 引擎 ~1 Hz `evaluate` 驱动 `__gesso.tick(Date.now())`；html 壁纸收 `postMessage {__gesso:"time"}`。
- ✅ cursor feed: 全局光标位置路由到所在显示器的会话，归一化量化（u16）后经 `__gesso.mouse(present,x,y,buttons)` 喂入，shader 写 `iMouse`、html 收 `{__gesso:"mouse"}`；推送 30 Hz、宿主页每帧快速平滑，变化才推。鼠标静止 5 分钟自动降到 5 fps，一动即恢复。
- 权限事实（实测）：位置走 `NSEvent::mouseLocation`、按键/空闲走 CoreGraphics HID 源状态表——纯轮询、不做事件 tap，**无需 Input Monitoring / 辅助功能授权**（只有 `CGEventTapCreate` 才要）。

### M1 — Windows pinning
- Own Win32 window + WebView2 child (`GPUI_DISABLE_DIRECT_COMPOSITION=1`), `SetParent` onto `WorkerW`, icon-hidden fallback path, `TaskbarCreated` re-pin, DPI/multi-monitor placement.
- Blocked on: a Windows machine (hardware or GUI-capable VM).

### M6 — Wallpaper Engine import I
- Workshop scan (`steamapps/workshop/content/431960/<id>/project.json`), `video`/`web` types; WE web-API shim (`wallpaperRegisterAudioListener` etc.).
- GPL boundary: invoke `depkg-cli` as a subprocess or accept project-wide GPL (decision pending, see [docs/WALLPAPER-ENGINE.md](docs/WALLPAPER-ENGINE.md)).

## Known gaps (no milestone yet)

- `gesso://` custom scheme non-functional on current webview stack (zero callbacks; serving via self-contained `file://` entries meanwhile).
- Real-machine monitor outline (signature #1) and tray panel positioning on retina displays.
- ~~HTML thumbnails use the gradient placeholder~~ → Done (`372b4c4`): html captures live frames via the same persistent-window capture pipeline as shader/video.
- HTML wallpapers: iframe-internal navigation is not allow-listed yet (top-navigation / popups / forms are sandbox-blocked; `fps_cap` is advisory for the html kind). Hover previews use a two-phase cycle — preload all 16 frames behind a spinner, then play at a fixed 125 ms/frame (`c6bd3ce`).
- i18n: UI strings are centralized in `ui/strings.rs` (Chinese-first); English translation pass pending.
- Tray polish: dynamic menu copy (muda handlers are `Send`-only — menu handle can't be mutated from the poller) and precise quick-panel positioning relative to the tray icon (retina coordinate conversion).
