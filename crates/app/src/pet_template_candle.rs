//! 首发角色「小蜡烛」的模板实例。
//!
//! 帧数据由 `assets/pets/gen_sprites.py` 从基准稿生成（pet_sprite_data）；
//! 色板族、装饰格与行为参数在本文件定稿（色值见角色设计方向稿 §7）。

use super::pet_template::{BehaviorParams, Decoration, PaletteEntry, PetTemplate, Rgb};
use crate::pet_sprite_data::{FRAME_FLICKER, FRAME_IDLE, FRAME_SLEEP};

const OUTLINE: Rgb = (102, 26, 10);

/// 小蜡烛模板（当前唯一正式角色）；OnceLock 共享同一实例。
pub fn candle() -> &'static PetTemplate {
    static CANDLE: std::sync::OnceLock<PetTemplate> = std::sync::OnceLock::new();
    CANDLE.get_or_init(candle_template)
}

fn candle_template() -> PetTemplate {
    PetTemplate {
        grid_w: 25,
        grid_h: 50,
        frames: [
            FRAME_IDLE.to_vec(),
            FRAME_FLICKER.to_vec(),
            FRAME_SLEEP.to_vec(),
        ],
        body_palettes: vec![
            PaletteEntry {
                key: 0,
                colors: [(255, 246, 239), (255, 252, 237), (247, 234, 210)], // 奶白
            },
            PaletteEntry {
                key: 1,
                colors: [(247, 234, 210), (255, 246, 224), (232, 210, 174)], // 暖米
            },
            PaletteEntry {
                key: 2,
                colors: [(239, 217, 188), (249, 236, 212), (217, 190, 150)], // 奶茶
            },
        ],
        accent_palettes: vec![
            PaletteEntry {
                key: 0,
                colors: [(255, 225, 130), (255, 172, 4), (255, 109, 0)], // 暖橙
            },
            PaletteEntry {
                key: 1,
                colors: [(255, 243, 184), (255, 211, 78), (255, 176, 0)], // 蜜金
            },
        ],
        notice_palettes: [
            [(255, 246, 216), (255, 194, 78), (255, 142, 46)], // 注意档：提亮
            [(255, 216, 160), (255, 142, 46), (232, 60, 0)],   // 警告档：偏红
        ],
        outline: OUTLINE,
        decorations: vec![
            Decoration {
                min_marking: 1,
                min_body: 0,
                cells: (20..25).map(|y| (5, y, 3)).collect(), // 左侧蜡泪
            },
            Decoration {
                min_marking: 2,
                min_body: 0,
                cells: (23..28).map(|y| (19, y, 3)).collect(), // 双侧蜡泪（右）
            },
            Decoration {
                min_marking: 0,
                min_body: 1,
                cells: (3..22).map(|x| (x, 42, 3)).collect(), // 浅托盘
            },
            Decoration {
                min_marking: 0,
                min_body: 2,
                cells: (4..21).map(|x| (x, 43, 3)).collect(), // 高托盘
            },
        ],
        params: BehaviorParams {
            flicker_interval: std::time::Duration::from_millis(550),
            walk_excursion: 56.0,
            walk_seconds: 3.0,
            hit_width: 50.0,
            hit_radius: 22.0,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pet_template::FrameKind;

    #[test]
    fn candle_template_has_three_frames_and_locked_palette() {
        let t = candle();
        assert_eq!(t.frames.len(), 3);
        assert!(t.frames.iter().all(|f| f.len() == t.grid_w * t.grid_h));
        // 锁定调色板：全部帧的索引不超出 0..=7
        assert!(t.frames.iter().all(|f| f.iter().all(|&idx| idx <= 7)));
        assert_eq!(t.outline, OUTLINE);
    }

    #[test]
    fn decorations_grow_with_marking_and_body() {
        let t = candle();
        let bare = t.compose(FrameKind::Idle, 0, 0);
        let tears = t.compose(FrameKind::Idle, 1, 0);
        let both = t.compose(FrameKind::Idle, 2, 2);
        let marked = tears
            .iter()
            .zip(bare.iter())
            .filter(|(w, b)| **w != **b)
            .count();
        let added = both
            .iter()
            .zip(tears.iter())
            .filter(|(w, b)| **w != **b)
            .count();
        assert!(marked > 0, "单侧蜡泪应叠加格子");
        assert!(added > 0, "双侧蜡泪与托盘应继续叠加");
    }
}
