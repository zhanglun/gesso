//! 管理窗口 UI（gpui-kit 0.7）—— 界面与交互设计 v1.0 的页面实现。
//!
//! 模块地图（技术方案 §3.4）：
//! - `theme`        DESIGN.md token → kit 语义主题（亮/暗，跟随系统）
//! - `strings`      中文文案唯一出处（i18n 预留）
//! - `data`         数据类型定义
//! - `app_state`    全局状态（唯一真源；M3 换 Entity 观察模式）
//! - `widgets`      自绘业务小件（角标/预览/状态行/设置行）
//! - `shell`        窗口壳：顶栏 + 三页签 + 键盘模型
//! - `library_view` 壁纸库页（默认页）
//! - `monitors_view`显示器页（拓扑 + 投放指派）
//! - `settings_view`设置页（分组表单，即时生效）
//! - `first_run`    首启向导（独立小窗）
//!
//! 托盘为纯原生菜单（muda，左/右键同菜单）——2026-10-03 用户决策撤销快速面板。

pub mod app_state;
pub mod data;
pub mod first_run;
pub mod library_view;
pub mod monitors_view;
pub mod settings_view;
pub mod shell;
pub mod strings;
pub mod theme;
pub mod widgets;
