# AGENTS.md — AI 会话工作指南

> 面向在本仓库工作的 AI 编码助手（与人类贡献者同样适用，另有 CONTRIBUTING.md）。
> 目标：让任何一个新会话在 5 分钟内安全开工，不重复踩坑。

## 项目一句话

Gesso：跨平台（Windows/macOS）动态壁纸引擎——视频/动图/Shader/网页钉在桌面图标层之下。Rust + GPUI + wry。

## 必读文档（按顺序）

1. **[docs/ENGINEERING-NOTES.md](docs/ENGINEERING-NOTES.md)** —— 5 条架构规则 + 25 条实测踩坑，改代码前先读。
2. **[crates/app/API.md](crates/app/API.md)** —— UI 层唯一允许调用的引擎接口。
3. **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)** —— 分层、窗口模型、贴壁参数、状态机。
4. **[docs/ROADMAP.md](docs/ROADMAP.md)** —— 当前进度与已知缺口。
5. **[docs/design/](docs/design/)** —— 设计归档：交互规格（界面与交互设计.md，已冻结 v1.0）、视觉 token（DESIGN.md）、**可点击 HTML 原型**（prototype/index.html，视觉契约）、完整工程方案（技术方案.md，14 章）+ 5 张架构/流程图。

## 硬性规则（历史事故的浓缩，全文见工程笔记 §1–§2）

1. **壁纸窗口是 `pin/` 里的纯 AppKit/Win32 窗口，绝不是 GPUI 窗口。** GPUI 的窗口协调器会重置桌面级全屏几何（M1.5 实测：origin 被压到 -30 并持续回写）。
2. **UI 只走 `crates/app/API.md` 的接口**；写操作入队 `EngineAction`，读操作走快照，快照合并必须保留 UI 本地状态（tab/query/selected/filter）。
3. **改界面行为先改 `docs/design/` 的规格与原型，再改代码**——设计文档随仓库迭代，随设计迭代更新。
4. **提交前必须**：`cargo check -p gesso-app` 通过 + `cargo test -p gesso-core` 全绿；改了行为要在实机跑对应路径。
5. **原子提交**：一次提交一个逻辑变更；不把别人的未完成文件 `git add -A` 进来（本仓库曾有双会话并行，先 `git status` 确认归属）。

## 常用命令

```bash
cargo build -p gesso-app                 # 构建
cargo run   -p gesso-app                 # 运行（托盘 + 管理窗口 + 壁纸）
cargo test  -p gesso-core -p gesso-app   # 单测
cargo clippy -p gesso-core --all-targets -- -D warnings
pkill -f "target/debug/gesso"            # 退出（有单实例锁）
GESSO_LOCK=dev ./target/debug/gesso      # 开发期多实例并存（用完记得关）
```

## 仓库地图

```
crates/core     纯领域逻辑（ContentSpec/配置/库/状态机）——零平台依赖，改这里最安全
crates/app      应用：
  ├─ pin/         平台贴壁层（macOS: AppKit；Windows: 计划中）——唯一允许碰原生窗口的模块
  ├─ session.rs   会话管理器（唯一编排者）
  ├─ protocol.rs  gesso:// 资源协议（当前 file:// 自包含模式，协议修复待办）
  ├─ bridge/      系统事件桥（光标/全屏/电源/时间）
  ├─ engine.rs    AppState 全局 + EngineAction 动作队列
  ├─ ui/          管理窗口 UI（gpui-kit）
  └─ assets/      宿主页（host/index.html）/ 内置样例（samples/）/ 产品图标（icons/ 产出 + 再生成管线，见其 README）
docs/           工程文档 + design/（设计归档）
```

## 当前状态速览（截至本文件更新）

- ✅ macOS 视频壁纸钉桌面全链路（贴壁/穿透/多空间/菜单栏带覆盖/暂停恢复/持久化）
- ✅ 导入（对话框 + 拖入）、托盘（右键菜单）、设置持久化、开机自启、首启向导
- ✅ 管理窗口三页签（真数据桥接）+ 缩略图悬停预览 + 显示器页「桌面沙盘 + 详情条」改版
- ✅ M4 渲染器完备：shader（WebGL2 + Shadertoy 子集 uniforms + iChannel）+ html（沙箱 iframe，实机 DoD：storage/IPC 全部 SecurityError）+ 内置样例 ×5
- ⬜ M1 Windows 贴壁验证（**缺 Windows 机器**）
- ✅ M5 系统数据桥主体：全屏检测 + 电池策略 → 自动暂停/降帧（实机验证）+ 时间脉冲 feed；⬜ 光标 feed（需权限流）
- ⬜ M6 WE 导入
- ⬜ 协议修复（`gesso://` 自定义协议零回调，当前 file:// 自包含模式）

## 已知平台事实（写代码前扫一眼，全文见工程笔记 §2）

- macOS 构建**不需要 Xcode**（runtime_shaders）；图标用 `gpui_kit_assets::IconName`（完整 Lucide）；`Button` 无 `color()`/`when()`（用变体 + `if`）；`overflow_y_scroll` 必须在 `.id()` 之后；库路径含空格必须百分号编码；`swap(true)` 当开关必错（用 `fetch_xor`）。
- `gesso://` 自定义协议在本版 lb-wry/WKWebView **回调零触发**——条目自包含 file:// 是当前方案；协议修复是独立待办。
