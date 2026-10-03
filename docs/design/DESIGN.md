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
