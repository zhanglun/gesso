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
    /// 自主走动的最大横向距离（pt）。
    pub walk_excursion: f64,
    /// 自主走动的单程时长（秒）。
    pub walk_seconds: f64,
    /// 可见蜡身宽度（pt），命中检测按此收窄。
    pub hit_width: f64,
    /// 命中圆角半径（pt）。
    pub hit_radius: f64,
}

/// 角色模板。
#[derive(Debug, Clone)]
pub struct PetTemplate {
    pub grid_w: usize,
    pub grid_h: usize,
    /// 三帧索引网格：0=清醒 1=摇曳 2=睡眠。
    pub frames: [Vec<u8>; 3],
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
        match kind {
            FrameKind::Idle => &self.frames[0],
            FrameKind::Flicker => &self.frames[1],
            FrameKind::Sleep => &self.frames[2],
        }
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

    /// 领养预览用：每行输出色块行程 (格数, 调色板索引)，索引 0 为透明。
    pub fn row_runs(&self, cells: &[u8]) -> Vec<Vec<(usize, u8)>> {
        (0..self.grid_h)
            .map(|y| {
                let row = &cells[y * self.grid_w..(y + 1) * self.grid_w];
                let mut runs = Vec::new();
                let mut len = 0usize;
                let mut idx = 0u8;
                for &c in row {
                    if len > 0 && c == idx {
                        len += 1;
                    } else {
                        if len > 0 {
                            runs.push((len, idx));
                        }
                        len = 1;
                        idx = c;
                    }
                }
                if len > 0 {
                    runs.push((len, idx));
                }
                runs
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用最小模板：2×2 网格，索引 1=主体 7=描边。
    fn tiny_template() -> PetTemplate {
        PetTemplate {
            grid_w: 2,
            grid_h: 2,
            frames: [vec![1, 1, 1, 7], vec![1, 1, 1, 1], vec![1, 0, 0, 7]],
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
                walk_excursion: 56.0,
                walk_seconds: 3.0,
                hit_width: 2.0,
                hit_radius: 1.0,
            },
        }
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

    #[test]
    fn row_runs_split_by_color() {
        let t = tiny_template();
        let cells = t.compose(FrameKind::Idle, 0, 0);
        let runs = t.row_runs(&cells);
        assert_eq!(runs[0], vec![(2, 1)]);
        assert_eq!(runs[1], vec![(1, 1), (1, 7)]);
    }
}
