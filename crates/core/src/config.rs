//! 应用配置：显示器→壁纸映射 + 全局设置（技术方案 §10）。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Result, WallpaperKind};

/// 显示器→库条目映射；key 为 EDID 哈希稳定 ID（§4.3）。
pub type MonitorMap = BTreeMap<String, String>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// 新指派的默认帧率上限。
    pub fps_cap_default: u8,
    /// 前台应用全屏时的策略。
    pub fullscreen_policy: PausePolicy,
    /// 使用电池时的策略。
    pub battery_policy: PausePolicy,
    /// 光标不在本显示器且无音频时帧率减半（§5.3 优化 5）。
    pub idle_downscale: bool,
    pub autostart: bool,
    pub startup_behavior: StartupBehavior,
    /// 界面语言（Auto = 跟随系统 locale；解析见 app 的 i18n）。
    pub language: Language,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            fps_cap_default: 60,
            fullscreen_policy: PausePolicy::Pause,
            battery_policy: PausePolicy::Pause,
            idle_downscale: true,
            autostart: true,
            startup_behavior: StartupBehavior::RestoreLast,
            language: Language::Auto,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// 跟随系统语言。
    Auto,
    /// 简体中文。
    Zh,
    /// English.
    En,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PausePolicy {
    Pause,
    Downscale,
    Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StartupBehavior {
    RestoreLast,
    Random,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// 显示器稳定 ID → 库条目 ID。
    pub monitors: MonitorMap,
    /// 显示器稳定 ID → 帧率上限（缺省用 settings.fps_cap_default）。
    pub monitor_fps: MonitorFpsMap,
    pub settings: Settings,
    /// 首启向导已触发过（§4.6）：真首启的唯一判定。不放进 [`Settings`]——
    /// 设置页「重置全部设置」与 UpdateSettings 均只写 settings 子结构，
    /// 不应让向导再次自动弹出。
    pub wizard_seen: bool,
}

pub type MonitorFpsMap = BTreeMap<String, u8>;

/// 配置热更新 diff：会话管理器据此只重建受影响的壁纸窗口（§10）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MonitorDiff {
    /// 需要新建/重建会话的显示器（含新指派与换壁纸）。
    pub to_rebuild: Vec<(String, String)>,
    /// 已拔出或取消指派的显示器。
    pub to_teardown: Vec<String>,
}

pub fn diff_monitors(old: &MonitorMap, new: &MonitorMap) -> MonitorDiff {
    let mut d = MonitorDiff::default();
    for (m, entry) in new {
        match old.get(m) {
            Some(e) if e == entry => {}
            _ => d.to_rebuild.push((m.clone(), entry.clone())),
        }
    }
    for m in old.keys() {
        if !new.contains_key(m) {
            d.to_teardown.push(m.clone());
        }
    }
    d
}

impl AppConfig {
    pub fn load(path: &std::path::Path) -> Result<Self> {
        Ok(serde_json::from_slice(&std::fs::read(path)?)?)
    }

    pub fn save(&self, path: &std::path::Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Ok(std::fs::write(path, serde_json::to_vec_pretty(self)?)?)
    }
}

// WallpaperKind 此处仅用于未来按类型筛选的 API 预留，避免未使用告警。
#[allow(dead_code)]
fn _kind_hint(_: WallpaperKind) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> MonitorMap {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn config_roundtrip() {
        let mut cfg = AppConfig::default();
        cfg.monitors.insert("edid-aaa".into(), "entry-1".into());
        let dir = std::env::temp_dir().join(format!("gesso-test-{}", std::process::id()));
        let p = dir.join("config.json");
        cfg.save(&p).unwrap();
        let back = AppConfig::load(&p).unwrap();
        assert_eq!(cfg, back);
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn diff_detects_rebuild_teardown_and_unchanged() {
        let old = map(&[("m1", "a"), ("m2", "b"), ("m3", "c")]);
        let new = map(&[("m1", "a"), ("m2", "z"), ("m4", "d")]);
        let d = diff_monitors(&old, &new);
        assert_eq!(
            d.to_rebuild,
            vec![("m2".into(), "z".into()), ("m4".into(), "d".into())]
        );
        assert_eq!(d.to_teardown, vec!["m3".to_string()]);
        assert!(
            !d.to_rebuild.iter().any(|(m, _)| m == "m1"),
            "未变化的不重建"
        );
    }

    #[test]
    fn defaults_match_spec() {
        let s = Settings::default();
        assert_eq!(s.fps_cap_default, 60);
        assert_eq!(s.fullscreen_policy, PausePolicy::Pause);
        assert!(!AppConfig::default().wizard_seen);
    }

    #[test]
    fn legacy_config_without_wizard_seen_loads_as_unseen() {
        // 升级路径：旧版 config.json 无 wizard_seen 键 → 视为未看过向导（§4.6）
        let dir = std::env::temp_dir().join(format!("gesso-wiz-legacy-{}", std::process::id()));
        let p = dir.join("config.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&p, r#"{"monitors":{},"monitor_fps":{},"settings":{}}"#).unwrap();
        let cfg = AppConfig::load(&p).unwrap();
        assert!(!cfg.wizard_seen);
        std::fs::remove_file(&p).ok();
    }
}
