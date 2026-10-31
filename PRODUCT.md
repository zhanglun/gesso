# Product

<!-- impeccable:product-schema 1 -->

> 本文件由仓库文档（AGENTS.md、docs/design/桌面宠物设计.md、桌面宠物 Web 控制台设计.md/技术方案.md、缝隙宇宙设定集）推断整理，2026-10-30 经用户三点确认：解除 Web 中性基线、纯视觉替换范围、静态原型验证方式；「游戏一样」的交互气质为用户当场钉死的约束。

## Platform

web

## Users

- 主用户：Gesso 桌面宠物功能的使用者——当前即开发者本人（自用优先）。在 Gesso 运行中的同一台电脑上，从托盘「在浏览器中管理宠物…」打开本机控制台，完成领养、调档位、预览动作、发模拟事件。
- 次用户（远期）：开源社区里跑了 Gesso 的桌面宠物玩家。无账号、无远程、单机。

## Product Purpose

Gesso Pet Studio 是桌面宠物的唯一管理面（2026-10-30 起 GPUI 领养小窗已撤销）：用浏览器承载领养流程、档位设置、动作预览和模拟事件，替代桌面小窗承载不了的复杂界面。成功 = 领养一只宠物并把它调教到舒服的状态，全程不需要碰文件和配置；预览面板为正式美术接入提供验收画布。

## Positioning

本机 loopback Server + 系统浏览器，Rust 是唯一状态/规则/存档真源：Web 只读快照、写操作走 `EngineAction` 队列 + revision 乐观锁。桌面上同时活着一只原生窗口的宠物（AppKit/Win32），Web 页面永远不替代它——这是与「把宠物整个搬进浏览器」路线的根本区别。

## Operating Context

- Gesso 托盘常驻；控制台页面 1.5s 轮询 `/api/state`，连接状态常显；
- 预览帧来自 `/api/pet/preview?state=idle|sleep|blink[&candidate=]`，真实精灵 PNG，`image-rendering: pixelated`；
- 浏览器 `prefers-reduced-motion` 只降 Web 预览动画，永不改变桌面宠物行为；
- 领养 = 状态机 `NotAdopted → 候选生成 → 选择 → 保存中 → Adopted`，三候选、无限 reroll、确认才落盘；
- 重置必须两步确认（request → confirm），模拟事件三类（新结果/需要关注/警告）走真实宠物气泡。

## Capabilities and Constraints

- 四页固定：概览、领养、宠物实验室、设置（+折叠的关于与诊断）；本次重设计为纯视觉替换，结构/文案/契约不变；
- MVP 实验室只有 `idle`/`sleep`/`blink` 三态 + 三种检查背景 + 自动播放；
- 尺寸三档（小/标准/大）、活跃度三档（安静/正常/活泼），离散档位不用滑杆；
- 统计只读（陪伴时长/被摸/被戳），不存最后互动时间；
- 首版不做：局域网/远程、账号、系统通知捕获、宠物包导入、行为编辑器。

## Brand Commitments

- Gesso 产品品牌：「底色画布·潮升」——gesso 白画布 + 黎明大浪，动态壁纸的黎明（docs/design/DESIGN.md）；
- 桌面宠物世界观：《缝隙宇宙·世界观设定集》v1（2026-10-25 起）是角色/美术/文案唯一上位依据——屏幕像素缝隙里的古老小生命，治愈为底 + 个别俏皮，火苗即生命状态，永不烧短；
- 首发角色：拟物小蜡烛（工作名），正式美术方向 = 分层位图 + 领养时离线合成，程序不压缩美术；
- **2026-10-30 用户钉死：Web 控制台要做成「像游戏一样的交互页面」**——游戏 UI 传统是本界面的合法语言，管理后台气质是需要逃离的默认；
- **2026-10-31 用户改选方向 3「图鉴观测站」并钉死「Web 页面本体像素风」**（像素字体/硬边/阶梯/抖动，不只是精灵是像素）；现行视觉世界 = 「像素图鉴 · 观测台」（v0.3，伴宠机 v0.2 废弃）；
- **2026-10-31 用户定稿前端栈：Preact + HTM 免构建静态文件**（vendor 运行时 + import map，无 npm/Node 依赖；React 手感、未来可无痛升级真 React）；
- Web 控制台视觉独立于主应用 GPUI 窗口（用户解除 v0.1 的中性灰阶基线）。

## Evidence on Hand

- 真实精灵资产：docs/archive/design-drafts/drafts/candle-candidates/candle-1..8.png（189×374 RGBA 候选稿）+ 正式预览 API 的实时 PNG；
- 行为断言：tools/pet-verify/ 下 web-e2e（23 断言）与 web-drill（24 断言）均为 HTTP 驱动，不依赖 DOM；
- 完整交互规格：docs/design/桌面宠物 Web 控制台设计.md（615 行，能力与文案事实源）。

## Product Principles

1. 宠物是生命不是资产：界面语言服务「它住在你的桌面」的情感事实，不做冷冰冰的配置管理；
2. 预览不扰真：Web 上的把玩永不惊动桌面上那位，除非用户明确下达动作；
3. 真源唯一：Rust 说了算，Web 只表达；乐观锁冲突时提示重试而不是静默覆盖；
4. 治愈为底：安静、温柔、有点俏皮——杜绝 gamification 的任务/积分/焦虑机制，游戏感来自世界的可信，不来自激励闭环。

## Accessibility & Inclusion

- `prefers-reduced-motion`：关闭 Web 预览非必要动画（已实现，必须保留）；
- 键盘可达 + 焦点可见；中文为一等文案（zh 为主，en 为辅注）。
