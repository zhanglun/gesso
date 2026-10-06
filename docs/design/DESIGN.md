# Design

> 从已冻结的 v1.0 原型（prototype/index.html）记录的视觉世界。实现（GPUI）以本文件 + 原型为唯一视觉依据；改动先改这里与原型，再改代码。

## 世界（thesis）

**界面越安静，壁纸越出彩。** 跟随系统的原生专业工具：中性灰阶为骨，唯一的高饱和内容是壁纸预览本身。强调色只做三件事——主操作、当前选中、状态指示；禁止装饰性用色。

## 工艺基准

Raycast 的面板纪律 + Linear 的密度与键盘 + 系统设置的双主题克制（随 v1.0 冻结确认）。

## Tokens（双主题，值取自原型 CSS 变量）

| 语义 | 亮 | 暗 |
|---|---|---|
| `bg/surface` | `#FFFFFF` | `#1E1E20` |
| `bg/panel` | `#F5F5F7` | `#26262A` |
| `bg/elevated` | `#FFFFFF` + 阴影 | `#303034` |
| `text/primary` | `#1D1D1F` | `#EBEBED` |
| `text/secondary` | `#6E6E73` | `#9B9BA1` |
| `accent` | `#316EF5` | `#5B8DEF` |
| `accent/soft` | 蓝 12% | 蓝 18% |
| `danger` | `#D33A3A` | `#E5545B` |
| `hairline` | `#E5E5EA` | `#3A3A3F` |

GPUI 落地：两套 `Theme` Entity + 系统主题监听（Win 注册表 / mac `effectiveAppearance` KVO）。

## 排版

系统栈单一 sans（SF Pro / Segoe UI Variable / PingFang SC）。固定阶梯（比例 1.16）：12 / **13（基准）** / 15 / 20 / 28（仅向导）。数据一律 tabular-nums。UI 内不用 mono（那是代码与数据的事）。

## 组件（12 件，六态齐全；基础控件用 gpui-kit，业务件自绘）

**来源分层**（2026-10 采用 gpui-kit 0.7，Apache-2.0）：Button/Input/Form/Menu/Tab/List/Dropdown/Switch/Popover 等**基础控件用 kit**（含 Lucide 图标 = 本设计的线性 1.5px 图标规格）；**业务组件自绘**——库卡片、屏卡片、托盘快速面板、空状态、角标（信息密度和状态语义是我们的）。主题：DESIGN token 映射到 kit 语义主题（M0.5 ④ 验证），亮暗随系统。

按钮×3（主/次/文字，高 28，圆角 6）· 开关（36×20）· 下拉 · 输入 · 页签 · 右键菜单（elevated + 阴影 + 分隔线，破坏项红字置底）· Toast（底部居中，2.6s）· 库卡片（16:9 预览 + 56px 信息条 + 3px 语义指示条）· 显示器卡片 · 角标（10px 胶囊）· 空状态（区分"无结果"与"库为空"）。

图标：自绘线性 SVG，1.5px 描边圆角端点，16/20 两档；**禁 emoji/unicode 代替图标**。

## 动效纪律

150–250ms `ease-out`；只表达状态（弹出、指派淡入、描边点亮）；禁页面加载编排与装饰循环。

## Signature ×2（2026-10-03 起冻结，验收必测）

> 原 #3「托盘左键快速面板」经用户决策撤销（浮动小窗观感突兀，改纯托盘菜单，见界面与交互设计 §4.1 变更记录）。

1. 悬停库卡片 → 对应显示器边框 2px accent 点亮（真机=物理显示器边缘描边）
2. 拖卡片 → 显示器投放区 → 放手即指派（真机=拖到显示器页拓扑）

## 状态词汇

hover / focus(2px accent 环，永远可见) / active / disabled / selected / loading / error(danger 只表真故障) —— 任何新组件六态补齐才许出厂。

## 主题化浏览器面

选区色、滚动条、caret、焦点环全部走 token（原型已实现，GPUI 对应自绘）。

## 品牌图形（2026-10-03 定稿 · 二稿，验收必测）

**概念「底色画布 · 潮升」**：Gesso = 画布的底料层。应用图标 = gesso 白画布上，黎明大浪立起，半轮日出从浪后升起 —— 动态壁纸的黎明，一句话讲完。**二稿**（用户反馈驱动；三案对比选定「A 潮升」，另案 B 色叠 / C 字标 G 存档 `docs/design/drafts/`）：移除一稿「桌面图标点」（叙事未传达）、浪幅加倍、日出为焦点；托盘弃双矩形（撞系统「屏幕镜像」字形）。装饰性用色禁令在图标处让位于语义：**色 = 壁纸内容本身**，白 = gesso 底。

**应用图标**（源 `crates/app/assets/icons/src/app-icon.svg`，1024，透明边距 100）

- 形：Big Sur 方圆角（superellipse n=5，824/1024）+ 顶部内侧阴影 10%。
- 底：gesso 白纵向 `#FDFCF9→#F6F4EF→#EFEBE3`。
- 日出：半轮日 `cx668 cy640 r98`，`#F8D9A8→#E88C4C` 纵向；光晕 r140 `#F0B878` 50% blur40；浪盖住下缘 → 半升。
- 浪：前层黎明对角（左下→右上）`#2450D6→#316EF5→#7C5CE0→#E08A4E`，峰在右 + 上缘白 sheen 20% + 浪尖高光线（`#AECBFA→#CDB9F4→#F6D3AC`）；后层暖 `#7FA8F2→#9C7BE8→#F0B878` 85%，在峰右上露暖沿。
- **小尺寸（≤32px）compact 稿**：浪位整体抬高 78（色带 ≈40%）、去光晕/浪尖线，**保留日出**（16px = 白/蓝/橙三点记忆）；icns/ico 混排两稿 = Apple 惯例。
- **Windows 满幅稿（2026-10-06）**：Big Sur 边距是 macOS Dock 网格惯例，Windows 任务栏/托盘图标满幅铺满——带边距在 Windows 上比别家小一圈（实测）。`win/gesso.ico` 与托盘 `tray-app-32.png` 用 viewBox 裁到 824 图形区的满幅稿（≤32 仍 compact 稿），macOS icns/src master 保持带边距不动。
- 产出：`mac/Gesso.icns`（16→1024，带边距）、`win/gesso.ico`（16→256，PNG 直嵌，满幅）、`tray/tray-app-32.png`（满幅 compact）。

**托盘字形**（`src/tray-*.svg`，24 网格，「屏中有浪」）：圆角屏描边 `16.6×14.6 r4 stroke2.2` + 内腔底部实浪（clip 内腔）。读作「显示器 + 活的桌面」。**禁止双/错位矩形** —— macOS 菜单栏「屏幕镜像」系统字形即双圆角矩形，一稿踩过，永不回头。macOS 黑字形 + `with_icon_templated`（菜单栏亮暗自适应），内嵌 44px@2x（tray-icon 约束 22pt，Retina 清晰）。**Windows 不用字形**：直接内嵌彩色 compact 应用图标 `tray-app-32.png`（32px，与任务栏/exe 图标同稿）——2026-10-06 用户决策：描边字形缩到托盘 16px 只剩轮廓线读不出，白/单色字形又受任务栏亮暗牵制；原「彩色图标 16px 下日出成噪点」的顾虑让位于可读性，单色 template 机制自此**仅 macOS**。**禁止**：macOS template 字形上色 / 加投影 / 带底板；托盘用双/错位矩形。

**再生成**：`cd crates/app/assets/icons/tools && npm i && node build.mjs`（sharp + iconutil）。改图形先改本节与源，再跑管线，产出随仓库提交；运行时只 `include_bytes!` 产出文件（`main.rs::tray_icon_rgba` / `apply_dock_icon`）。预览契约：`06-品牌图标.brand.html`（真机场景亮暗双主题）。
