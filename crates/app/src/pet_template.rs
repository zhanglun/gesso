//! 角色模板：一份数据描述一个宠物角色的全部帧、色板与行为参数。
//!
//! 窗口/渲染层只认本模块的通用接口；具体角色（如小蜡烛）提供模板实例。
//! 新增角色 = 新增一份模板数据，窗口与领养流程零改动。

use std::time::Duration;

pub type Rgb = (u8, u8, u8);

/// 单帧类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    Idle,
    Flicker,
    Sleep,
    /// 眨眼：眼睛闭合的清醒帧。
    Blink,
    /// 注视：眼簇向左偏一格。
    GazeLeft,
    /// 注视：眼簇向右偏一格。
    GazeRight,
}

impl FrameKind {
    /// 预渲槽位序 → 帧类型。预渲染与运行时帧选择都必须经此对应，
    /// 避免两处手写 match 漂移（曾致 Blink/Gaze 预渲成睡眠帧，见工程笔记 §2）。
    pub fn from_index(index: usize) -> Self {
        const ALL: [FrameKind; 6] = [
            FrameKind::Idle,
            FrameKind::Flicker,
            FrameKind::Sleep,
            FrameKind::Blink,
            FrameKind::GazeLeft,
            FrameKind::GazeRight,
        ];
        ALL.get(index).copied().unwrap_or(FrameKind::Idle)
    }
}

/// 角色的全部帧变体（由模板数据提供）。
#[derive(Debug, Clone)]
pub struct PetFrames {
    pub idle: Vec<u8>,
    pub flicker: Vec<u8>,
    pub sleep: Vec<u8>,
    pub blink: Vec<u8>,
    pub gaze_left: Vec<u8>,
    pub gaze_right: Vec<u8>,
}

impl PetFrames {
    /// 帧变体数量（供渲染缓冲分配）。
    pub fn kind_count(&self) -> usize {
        6
    }
}

/// 色板条目：key 匹配外观参数值，colors 为该变体的三个色位。
#[derive(Debug, Clone)]
pub struct PaletteEntry {
    pub key: u8,
    pub colors: [Rgb; 3],
}

/// 条件装饰：候选的 marking/body 达到阈值时叠加的格子。
#[derive(Debug, Clone)]
pub struct Decoration {
    pub min_marking: u8,
    pub min_body: u8,
    /// (x, y, 调色板索引)
    pub cells: Vec<(usize, usize, u8)>,
}

/// 行为与命中参数。
#[derive(Debug, Clone)]
pub struct BehaviorParams {
    /// 火苗摇曳的帧交替间隔。
    pub flicker_interval: Duration,
    /// 眨眼间隔（两次眨眼的起始间距）。
    pub blink_interval: Duration,
    /// 眨眼闭合时长。
    pub blink_duration: Duration,
    /// 自主走动的最大横向距离（pt）。
    pub walk_excursion: f64,
    /// 自主走动的单程时长（秒）。
    pub walk_seconds: f64,
    /// 可见主体命中区（格坐标：起始列、起始行、列数、行数；行自顶部起）。
    pub hit: (usize, usize, usize, usize),
    /// 命中圆角半径（pt）。
    pub hit_radius: f64,
}

/// 角色模板。
#[derive(Debug, Clone)]
pub struct PetTemplate {
    pub grid_w: usize,
    pub grid_h: usize,
    pub frames: PetFrames,
    /// 主体色板族（按外观 coat_color 精确匹配）。
    pub body_palettes: Vec<PaletteEntry>,
    /// 点缀色板族（按外观 accent 精确匹配）。
    pub accent_palettes: Vec<PaletteEntry>,
    /// 通知火苗强化色（下标 0 = 注意档，1 = 警告档）。
    pub notice_palettes: [[Rgb; 3]; 2],
    /// 描边/眼固定色（不参与随机）。
    pub outline: Rgb,
    /// 条件装饰（蜡泪、托盘等）。
    pub decorations: Vec<Decoration>,
    pub params: BehaviorParams,
}

impl PetTemplate {
    /// 指定帧的索引网格。
    pub fn frame(&self, kind: FrameKind) -> &[u8] {
        let f = &self.frames;
        match kind {
            FrameKind::Idle => &f.idle,
            FrameKind::Flicker => &f.flicker,
            FrameKind::Sleep => &f.sleep,
            FrameKind::Blink => &f.blink,
            FrameKind::GazeLeft => &f.gaze_left,
            FrameKind::GazeRight => &f.gaze_right,
        }
    }

    /// 眨眼时机：清醒时长落入每个周期的闭合窗口即为眨眼。
    pub fn blink_active(&self, awake: Duration) -> bool {
        let period = (self.params.blink_interval + self.params.blink_duration).as_millis() as u64;
        if period == 0 {
            return false;
        }
        (awake.as_millis() as u64 % period) >= self.params.blink_interval.as_millis() as u64
    }

    /// 组合索引 → 颜色表（下标 0 为透明占位）。
    ///
    /// `notice_boost`：0 无、1 注意档、2 警告档（警告档同时作用于注意档位）。
    pub fn palette(&self, coat_color: u8, accent: u8, notice_boost: u8) -> [Rgb; 8] {
        let pick = |entries: &[PaletteEntry], key: u8| -> [Rgb; 3] {
            entries
                .iter()
                .find(|entry| entry.key == key)
                .or_else(|| entries.first())
                .map(|entry| entry.colors)
                .unwrap_or([(0, 0, 0); 3])
        };
        let body = pick(&self.body_palettes, coat_color % 3);
        // 通知激活时火苗换强化色板；平时按外观点缀参数选火苗色板
        let flame = if notice_boost == 0 {
            pick(&self.accent_palettes, accent % 2)
        } else {
            self.notice_palettes[(notice_boost - 1) as usize]
        };
        [
            (0, 0, 0),
            body[0],
            body[1],
            body[2],
            flame[0],
            flame[1],
            flame[2],
            self.outline,
        ]
    }

    /// 组合帧：按候选外观叠加条件装饰。
    pub fn compose(&self, kind: FrameKind, marking: u8, body: u8) -> Vec<u8> {
        let mut cells = self.frame(kind).to_vec();
        for decoration in &self.decorations {
            if marking >= decoration.min_marking && body >= decoration.min_body {
                for &(x, y, idx) in &decoration.cells {
                    cells[y * self.grid_w + x] = idx;
                }
            }
        }
        cells
    }

    /// 渲染为 RGBA 缓冲（非预乘，长度 = 格数 × 4）。
    pub fn render_rgba(&self, cells: &[u8], colors: &[Rgb; 8], out: &mut [u8]) {
        debug_assert_eq!(cells.len(), self.grid_w * self.grid_h);
        debug_assert_eq!(out.len(), cells.len() * 4);
        for (i, &idx) in cells.iter().enumerate() {
            let (r, g, b) = colors[idx as usize];
            out[i * 4] = r;
            out[i * 4 + 1] = g;
            out[i * 4 + 2] = b;
            out[i * 4 + 3] = if idx == 0 { 0 } else { 255 };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_kind_index_roundtrip_locks_sprite_slots() {
        // 预渲槽位契约：序数即槽位，六个类别不得重叠/兑底
        for (index, expected) in [
            (0, FrameKind::Idle),
            (1, FrameKind::Flicker),
            (2, FrameKind::Sleep),
            (3, FrameKind::Blink),
            (4, FrameKind::GazeLeft),
            (5, FrameKind::GazeRight),
        ] {
            assert_eq!(FrameKind::from_index(index), expected);
            assert_eq!(expected as u8, index as u8);
        }
    }

    /// 测试用最小模板：2×2 网格，索引 1=主体 7=描边。
    fn tiny_template() -> PetTemplate {
        PetTemplate {
            grid_w: 2,
            grid_h: 2,
            frames: PetFrames {
                idle: vec![1, 1, 1, 7],
                flicker: vec![1, 1, 1, 1],
                sleep: vec![1, 0, 0, 7],
                blink: vec![1, 1, 1, 1],
                gaze_left: vec![7, 1, 1, 7],
                gaze_right: vec![1, 1, 7, 7],
            },
            body_palettes: vec![PaletteEntry {
                key: 0,
                colors: [(10, 10, 10), (20, 20, 20), (30, 30, 30)],
            }],
            accent_palettes: vec![PaletteEntry {
                key: 0,
                colors: [(40, 40, 40), (50, 50, 50), (60, 60, 60)],
            }],
            notice_palettes: [
                [(70, 70, 70), (80, 80, 80), (90, 90, 90)],
                [(100, 100, 100), (110, 110, 110), (120, 120, 120)],
            ],
            outline: (1, 2, 3),
            decorations: vec![Decoration {
                min_marking: 1,
                min_body: 0,
                cells: vec![(0, 1, 7)],
            }],
            params: BehaviorParams {
                flicker_interval: Duration::from_millis(550),
                blink_interval: Duration::from_millis(4000),
                blink_duration: Duration::from_millis(180),
                walk_excursion: 56.0,
                walk_seconds: 3.0,
                hit: (0, 0, 2, 2),
                hit_radius: 1.0,
            },
        }
    }

    #[test]
    fn blink_phase_alternates_closed_and_open() {
        let t = tiny_template();
        // 周期 = 4180ms：4000ms 睁眼 + 180ms 闭合
        assert!(!t.blink_active(Duration::from_millis(3999)));
        assert!(t.blink_active(Duration::from_millis(4000)));
        assert!(t.blink_active(Duration::from_millis(4179)));
        assert!(!t.blink_active(Duration::from_millis(4180)));
    }

    #[test]
    fn palette_selects_by_key_and_notice_boost() {
        let t = tiny_template();
        let normal = t.palette(0, 0, 0);
        assert_eq!(normal[4], (40, 40, 40));
        let attention = t.palette(0, 0, 1);
        assert_eq!(attention[4], (70, 70, 70));
        let warning = t.palette(0, 0, 2);
        assert_eq!(warning[4], (100, 100, 100));
        // 警告档覆盖注意档位（下标 4 对应通知第一色位）
        assert_eq!(t.palette(0, 0, 2)[5], (110, 110, 110));
    }

    #[test]
    fn decorations_apply_only_above_thresholds() {
        let t = tiny_template();
        let without = t.compose(FrameKind::Idle, 0, 0);
        assert_eq!(without[2], 1);
        let with = t.compose(FrameKind::Idle, 1, 0);
        assert_eq!(with[2], 7);
    }

    #[test]
    fn render_maps_indices_and_transparency() {
        let t = tiny_template();
        let cells = t.compose(FrameKind::Idle, 0, 0);
        let colors = t.palette(0, 0, 0);
        let mut out = [0u8; 16];
        t.render_rgba(&cells, &colors, &mut out);
        assert_eq!(&out[0..4], &[10, 10, 10, 255]);
        assert_eq!(&out[12..16], &[1, 2, 3, 255]);
    }
}
