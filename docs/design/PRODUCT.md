# Product

<!-- impeccable:product-schema 1 -->

## Platform

desktop (Windows / macOS · GPUI)
<!-- schema 枚举无 desktop，此处显式记录实际平台；本文件位于设计目录，建仓库后随项目根迁移 -->

## Stack

GPUI 0.2（Rust，管理 UI + 应用壳）+ wry webview（壁纸渲染层）+ gesso-core（纯领域逻辑 crate）。完整技术方案见 [技术方案.md](技术方案.md) §2/§3.4。

## Users

- **主用户**：桌面重度用户（开发者 / 玩家），想把视频、Shader、网页内容钉在桌面图标层之下；Windows 或 macOS 单机 / 双机使用者。产品自用优先，即用户即作者本人（v1）。
- 次要（远期）：开源社区使用者，尤其是已订阅 Wallpaper Engine 创意工坊内容的用户。

## Product Purpose

Gesso 是开源跨平台（Windows / macOS）动态壁纸引擎：单一 webview 渲染管线承载视频 / GIF / Shader / HTML 四类壁纸与系统数据联动交互；远期只读兼容 Wallpaper Engine 工坊素材。成功 = 壁纸常驻不扰（性能与电源纪律）+ 高频操作两次点击内完成 + 素材失败永不留下空桌面。

## Positioning

跨 Win+mac、四类内容统一管线、贴壁层为唯一平台代码、WE 素材只读兼容。邻位产品均不可同时成立：Wallpaper Engine 不跨 macOS；Plash 已闭源且仅网页类；Lively 停滞且仅 Windows。

## Operating Context

托盘常驻桌面工具；壁纸窗口钉在桌面图标层之下（WorkerW / NSWindow 压层）；多显示器独立指派（EDID 稳定 ID）；全屏应用 / 电池自动暂停；壁纸内容以不可信代码对待（零 IPC 沙箱）。

## Capabilities and Constraints

- 四类内容统一管线：video / image(GIF·WebP) / shader(WebGL2, Shadertoy 子集) / html
- 系统数据桥：光标 / 时间 / 电源 / 全屏 / 天气（可选）
- 管理窗口 = 轻量工具定位（用户决策 2026-02）：v1 无标签系统、无收藏夹、无轮换列表、无批量管理；信息架构预留后续生长
- 界面语言：中文默认 + 文案抽离预留 i18n（用户决策 2026-02）
- 托盘常驻：关闭主窗口 = 隐藏进托盘，退出仅从托盘菜单（用户决策 2026-02）
- WE 兼容路线：video/web（M6）、scene（M7）、application 永拒；只读本机已订阅内容，不做下载器 / 再分发（法律红线）
- 性能预算：内置样例 shader p95 <15ms @60fps 核显；暂停态 0 开销

## Brand Commitments

- 名称：**Gesso**（用户选定 2026-02）
- 视觉方向（用户选定 2026-02）：跟随系统双主题（亮 / 暗）、中性灰阶、低调强调色；工具感优先
- **界面与交互规格 v1.0 + 高保真原型已冻结**（2026-02，用户确认）：见 [界面与交互设计.md](界面与交互设计.md) / [prototype/index.html](prototype/index.html) / [DESIGN.md](DESIGN.md)；craft bar = Raycast 面板纪律 + Linear 密度键盘 + 系统设置双主题克制

## Evidence on Hand

- [技术方案.md](技术方案.md)：完整工程方案（14 章）+ 5 张工具校验过的架构 / 流程图
- 本会话核实的外部事实（depkg/GPL、Plash 闭源、gpui/gpui-wry 现状等，见技术方案 §2.2/§8.4/§14）
- 高保真交互原型（prototype/index.html，双主题，演示数据全部页面内合成并标注）；无实现代码、无真实用户数据；演示用素材需按 CC0 自制，不得虚构

## Product Principles

1. **贴壁永不打扰**：性能 / 电源纪律优先于功能数量
2. **两次点击**：高频操作（切换 / 暂停）两击完成
3. **优雅降级**：任何素材失败回退静态图，桌面永不开天窗
4. **单向数据流**：UI 不是真源，Rust 状态机是
5. **壁纸内容 = 不可信代码**：安全边界不可协商
