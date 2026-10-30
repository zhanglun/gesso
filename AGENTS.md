# AGENTS.md — AI 会话工作指南

> 面向在本仓库工作的 AI 编码助手（与人类贡献者同样适用，另有 CONTRIBUTING.md）。
> 目标：让任何一个新会话在 5 分钟内安全开工，不重复踩坑。

## 项目一句话

Gesso：跨平台（Windows/macOS）动态壁纸引擎——视频/动图/Shader/网页钉在桌面图标层之下。Rust + GPUI + wry。

## 必读文档（按顺序）

1. **[docs/ENGINEERING-NOTES.md](docs/ENGINEERING-NOTES.md)** —— 架构规则 + 当前平台纪律 + 高风险调试路径，改代码前先读。
2. **[crates/app/API.md](crates/app/API.md)** —— UI 层唯一允许调用的引擎接口。
3. **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)** —— 分层、窗口模型、贴壁参数、状态机。
4. **[docs/ROADMAP.md](docs/ROADMAP.md)** —— 当前进度与已知缺口。
5. **[docs/design/](docs/design/)** —— 当前交互规格、视觉 token、可点击原型、技术方案和图表源；辅助图表不是改代码前必读材料。
6. **[docs/RELEASE-CHECKLIST.md](docs/RELEASE-CHECKLIST.md)** —— 发布操作清单与流水记录（产物不签名/不公证，流程已自动化）。

## 硬性规则（历史事故的浓缩，全文见工程笔记 §1–§2）

1. **壁纸窗口是 `pin/` 里的纯 AppKit/Win32 窗口，绝不是 GPUI 窗口。** GPUI 的窗口协调器会重置桌面级全屏几何（实测：origin 被压到 -30 并持续回写）。
2. **UI 只走 `crates/app/API.md` 的接口**；写操作入队 `EngineAction`，读操作走快照，快照合并必须保留 UI 本地状态（tab/query/selected/filter）。
3. **改界面行为先改 `docs/design/` 的规格与原型，再改代码**——设计文档随仓库迭代，随设计迭代更新。
4. **提交前必须**：`cargo check -p gesso-app` 通过 + `cargo test -p gesso-core` 全绿；改了行为要在实机跑对应路径。
5. **原子提交**：一次提交一个逻辑变更；不把别人的未完成文件 `git add -A` 进来（本仓库曾有双会话并行，先 `git status` 确认归属）。
6. **文案说人话**（2026-10-13 用户要求）：提交信息、CHANGELOG、文档与汇报的第一句必须让没参与过的人看懂「改了什么、为什么改」。不自造复合术语（反例：「台账死原值自愈」），不中英混排作比喻，不堆排比句；新概念先给全称和一句解释，已有概念沿用项目原名。

## 常用命令

```bash
cargo build -p gesso-app                 # 构建
cargo run   -p gesso-app                 # 运行（托盘 + 管理窗口 + 壁纸）
cargo test  -p gesso-core -p gesso-app   # 单测
cargo clippy -p gesso-core --all-targets -- -D warnings
pkill -f "target/debug/gesso"            # 退出（有单实例锁）
GESSO_LOCK=dev ./target/debug/gesso      # 开发期多实例并存（用完记得关）
GESSO_UPDATE_MODAL=1 GESSO_LOCK=dev ./target/debug/gesso   # 启动即「检查更新+开小窗」；加 GESSO_UPDATE_AUTO_DL=1 连「发现→下载」一并自动确认（仅 debug）
GESSO_PET_NOTICE=new GESSO_LOCK=dev ./target/debug/gesso   # 启动 ~3s 注入一次宠物模拟通知（气泡链路无头验证，仅 debug）；窗口层行为回归见 tools/pet-verify/regress.ps1
python3 tools/pet-verify/web-drill.py   # GPUI↔Web 双入口并发演练（自启动沙箱实例；钩子 GESSO_PET_GPUI_DRILL="秒:动作,…" 仅 debug，用法见文件头；改 web/ 后先 cargo build）
```

## 仓库地图

```
crates/core     纯领域逻辑（ContentSpec/配置/库/状态机/content 内容类型表）——零平台依赖，改这里最安全
crates/app      应用：
  ├─ pin/         平台贴壁层（macOS: AppKit；Windows: Win32 + WorkerW 挂载）——唯一允许碰原生窗口的模块
  ├─ session.rs   会话管理器（唯一编排者）
  ├─ capture.rs   缩略图采集（macOS WKWebView 快照；Windows 走 capture_win.rs：webview 抽帧 + PrintWindow）
  ├─ build.rs     Windows exe 图标资源内嵌（winresource，ID 1；托盘/任务栏图标规格见 docs/design/DESIGN.md 品牌图形）
  ├─ thumb.rs     视频抽帧 + 缩略图策略调度
  ├─ encoding.rs  纯工具：base64url/路径编码/目录拷贝/主资源发现
  ├─ host_cmd.rs  Rust→宿主页类型化命令（HostCommand）
  ├─ we.rs / we_shim.rs   WE 用户导入解析 / WE API 垫片
  ├─ protocol.rs  gesso:// 资源协议（已打通：host 统一宿主页 + library 条目资源，视频 Range）
  ├─ bridge/      系统事件桥（光标/全屏/电源/时间）
  ├─ engine.rs    AppState 全局 + EngineAction 动作队列
  ├─ pet_window.rs / pet_window/   桌面宠物窗（macOS AppKit / Windows 分层窗口；逐拍 sync）
  ├─ pet_store.rs / pet_cache.rs   宠物存档安全加载与缓存事务（状态机在 core: adoption/pet）
  ├─ pet_template*.rs / pet_sprite_data.rs  宠物软件渲染（程序化占位蜡烛；正式美术接入面）
  ├─ web_control.rs / web/        本机 Web 控制台（loopback Server + Svelte 前端，产物编译期内嵌；领养/设置/重置唯一管理面）
  ├─ update.rs    应用自更新（静态 latest.json 清单检查 + 下载/交付；状态真源在模块全局，代数计数回灌）
  ├─ ui/          管理窗口 UI（gpui-kit；strings.rs 双语包；update_view.rs 更新小窗；pet_view.rs 宠物展示页）
  └─ assets/      宿主页（host/index.html）/ 预置壁纸（samples/）/ 产品图标（icons/ 产出 + 再生成管线，见其 README）
docs/           工程文档 + design/（当前设计源；历史资料在 docs/archive/）
tools/pet-verify  宠物回归：窗口行为（regress.ps1，Windows）+ Web 控制台 e2e/drill（跨平台）
```

## 当前实现边界

- macOS 和 Windows 壁纸窗口由 `pin/` 自己管理，不使用 GPUI 窗口；Windows 使用 WorkerW，macOS 使用 AppKit。
- 视频使用原生管线：macOS 为 AVPlayerLayer，Windows 为 Media Foundation + DXGI；WebView 负责 shader、图片和 HTML。
- UI 写操作只能进入 `EngineAction`；读取使用快照；壁纸资源通过只读 `gesso://` 路由提供。
- WE 只接受用户主动导入的 `video` / `web` 条目，不扫描 Steam/工坊；远端网页只接受用户主动指定的 HTTPS 地址。
- 显示器重配使用 epoch 事件、去抖收敛和 macOS 自家过渡窗；运行时不写系统桌面。
- 内容类型、扩展名、MIME 和缩略图策略以 `gesso_core::content` 为唯一事实源；UI 接口以 `crates/app/API.md` 为唯一事实源。
- 应用更新不静默替换正在运行的程序；Windows 多显示器硬件回归、WE scene 和更广泛 HTML 兼容性仍在 `docs/ROADMAP.md`。

## 已知平台事实（写代码前扫一眼，全文见工程笔记 §2）

- macOS 构建**不需要 Xcode**（runtime_shaders）；图标用 `gpui_kit_assets::IconName`（完整 Lucide）；`Button` 无 `color()`/`when()`（用变体 + `if`）；`overflow_y_scroll` 必须在 `.id()` 之后；库路径含空格必须百分号编码；`swap(true)` 当开关必错（用 `fetch_xor`）。
- WE **只由用户主动导入**（对话框选 `project.json`），绝不扫描磁盘/枚举 Steam。video/web 不拷入库，`source_dir` 指用户选中目录，经 `gesso://steam/<entry-id>/…` 以不可猜 entry id 为凭据只读直引（视频 Range 206、shim 内存注入）。
- **Windows 壁纸窗口创建顺序 = 生死线**：必须先 `SetParent` 挂 WorkerW、**后**创建 WebView2——反序则窗口树全绿但整窗不渲染（DComp 视觉树创建时绑定宿主层级）；explorer 重启会连带销毁跨进程子窗口，重钉 = 整窗重建（`session::remount_all`）。
- Windows 进程 DPI awareness 必须自己在 main() 顶部声明 PMv2（GPUI/wry 均不设置），否则 125%+ 缩放下与 WorkerW 无法像素对齐；`GPUI_DISABLE_DIRECT_COMPOSITION=1` 同步设置。
- 本机系统若被精简过（System32 缺 Windows.UI.Composition.dll 等 WinRT UI 运行时）：WinRT UI 类激活 E_ACCESSDENIED，勿选 WinRT 呈现路线；经典 dcomp.dll/DXGI/MF 完好可用。
- Windows 侧 `gesso://` 走 WebView2 workaround（页面实际 origin 为 `http://gesso.<host段>/`）；`protocol::entry_url` 负责生成 workaround 形态，回调再还原为 `gesso://` 路由。
