# Gesso

**跨平台动态壁纸引擎（Windows / macOS）**——视频、动图、Shader、网页，钉在桌面图标层之下。Rust + [GPUI](https://gpui.rs)。

[AGENTS.md](AGENTS.md)（AI 会话指南） · [English](README.md) · [架构](docs/ARCHITECTURE.md) · [路线图](docs/ROADMAP.md) · [常见问题](docs/FAQ.md) · [参与开发](CONTRIBUTING.md)

> **状态：早期开发（0.1.x）——macOS 可用，Windows 进行中。**
> 除标注外，下文所有能力均在真机验证过。

---

## 它做什么

- **钉在图标层之下**：壁纸窗口位于桌面图标层下方，图标可见、可点（点击穿透），不遮挡任何工作内容。
- **一套宿主管线，四类内容**：视频（`mp4`/`webm`）、动图（`gif`/`webp`）、Shader（`glsl`，Shadertoy 风格）、网页（`html`）。宿主页是唯一契约，渲染器可插拔。
- **暂停是真停**：暂停时 JS 渲染循环停止、视频解码停止，画面定格而非淡出；全屏应用与电池模式下会自动暂停（见[路线图](docs/ROADMAP.md)）。
- **托盘优先**：暂停/恢复、换壁纸都在菜单栏；管理窗口只用来管内容。
- **壁纸内容零 IPC**：壁纸内容按不可信代码对待，运行在 webview 沙箱里，只能读自己的素材目录。

## 平台状态

| 平台 | 贴壁 | 说明 |
|---|---|---|
| **macOS** | ✅ 可用 | `NSWindow` 压到图标层之下（`level = -2147483604`）+ 点击穿透 + 全空间跟随；已用全屏视频壁纸实机验证 |
| **Windows** | 🚧 进行中 | 计划走自有 Win32 窗口 + WebView2 子窗口，`SetParent` 到 `WorkerW`。**需要 Windows 机器验证** |
| **Linux** | ❌ v1 不做 | KDE/GNOME/X11/Wayland 各需一套桌面集成路径，见[常见问题](docs/FAQ.md) |

| 能力 | 状态 |
|---|---|
| 视频壁纸：钉桌面 + 铺满全屏 | ✅ |
| 托盘暂停/恢复 | ✅ |
| 配置持久化（重启自动恢复） | ✅ |
| 显示器枚举（稳定 ID `cg-<display-id>`） | ✅ |
| 管理窗口（壁纸库 / 显示器 / 设置） | ✅ |
| 导入（文件对话框 + 拖入窗口） | ✅ |
| 设置持久化、开机自启 | ✅ |
| 首启向导 | ✅ |
| Shader 渲染器（Shadertoy 子集，WebGL2） | ✅ |
| HTML 渲染器（沙箱 iframe） | 🚧 计划（M4） |
| 系统数据桥（光标 / 电源 / 全屏 / 音频） | 🚧 计划（M5） |
| Wallpaper Engine 工坊素材（只读） | 🚧 计划（M6），见 [docs/WALLPAPER-ENGINE.md](docs/WALLPAPER-ENGINE.md) |

## 环境要求

- **macOS**：Command Line Tools 即可，**不需要完整 Xcode**（Metal 着色器经 `runtime_shaders` 在运行时编译）。
- **Windows**：MSVC 工具链（此阶段未验证）。
- **Rust**：较新的 stable（开发用 1.95）。

## 快速开始

```bash
git clone <本仓库> && cd gesso
cargo run -p gesso-app          # 托盘图标 + 管理窗口 + 壁纸
```

首次启动会注册内置样例壁纸并指派到主显示器。应用生成的文件都在：

```
~/Library/Application Support/Gesso/     # macOS（Windows 计划为 %USERPROFILE%\.gesso）
├── config.json         # 显示器 → 壁纸映射 + 设置
└── library/
    └── <条目id>/       # 条目自包含：index.html（宿主页）+ index.<ext>（素材）
```

常用命令：

```bash
cargo test  -p gesso-core -p gesso-app    # 逻辑单测（秒级）
cargo clippy -p gesso-core --all-targets -- -D warnings
pkill -f "target/debug/gesso"             # 退出（有单实例锁）
```

## 架构一屏

```
托盘 + 管理窗口 ── GPUI ────────────────┐
                                        │  EngineAction 动作队列（UI → 引擎）
                                        ▼
                              ┌──────────────────────┐
                              │   会话管理器          │  每显示器一个会话
                              │   运行时状态机        │  Idle→Loading→Playing→Paused…
                              └───────┬──────────────┘
                          平台贴壁层             内容库 + 配置
                              │
              ┌───────────────▼───────────────────────────┐
              │  壁纸窗口 = 自有原生窗口                    │  macOS: AppKit NSWindow
              │  + wry webview（宿主页，沙箱）              │  Windows: Win32（计划）
              └───────────────────────────────────────────┘
```

关键限制：**壁纸窗口是自有原生窗口，不是 GPUI 窗口**。实测发现 GPUI 的窗口协调器会反复改写桌面级全屏窗口的位置，两者合不来。详见 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) · 引擎接口 [crates/app/API.md](crates/app/API.md) · 工程规则与踩坑 [docs/ENGINEERING-NOTES.md](docs/ENGINEERING-NOTES.md)。

## 文档索引

| 文档 | 内容 |
|---|---|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | 分层、窗口模型、贴壁参数、内容管线、状态机、安全模型 |
| [docs/ENGINEERING-NOTES.md](docs/ENGINEERING-NOTES.md) | 架构规则 + 15 条实测踩坑 + 常用路径 |
| [crates/app/API.md](crates/app/API.md) | UI 层唯一允许使用的引擎接口 |
| [docs/ROADMAP.md](docs/ROADMAP.md) | 里程碑、当前进度、已知缺口 |
| [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) | 本地开发与调试指南（日志/诊断/排查速查表） |
| [docs/design/技术方案.md](docs/design/技术方案.md) | 完整工程方案（14 章） |
| [docs/WALLPAPER-ENGINE.md](docs/WALLPAPER-ENGINE.md) | 工坊素材兼容方案与法律边界 |
| [docs/FAQ.md](docs/FAQ.md) | 为什么不做 Linux？要 Xcode 吗？有遥测吗？怎么卸载？ |
| [SPIKE-REPORT.md](SPIKE-REPORT.md) | 三项可行性 spike 的实测结论 |

## 致谢与参考

- [Lively Wallpaper](https://github.com/rocksdanister/lively) —— Windows `WorkerW` 方案的参考。
- [Plash](https://github.com/sindresorhus/Plash)（历史版本，MIT）—— macOS 桌面级窗口放置的参考。
- [gpui-kit](https://github.com/longbridge/gpui-kit) / [Zed 的 GPUI](https://github.com/zed-industries/zed) —— UI 框架与组件。
- [wry](https://github.com/tauri-apps/wry)（经 `lb-wry`）—— webview 嵌入。
- 图标：[Lucide](https://lucide.dev)（经 `gpui-kit-assets`）。

## 许可证

双许可：[MIT](LICENSE-MIT) 或 [Apache-2.0](LICENSE-APACHE)，任选其一。

Wallpaper Engine 商标归其所有者。Gesso 与 Wallpaper Engine 无隶属关系，不打包、不下载、不再分发工坊内容。
