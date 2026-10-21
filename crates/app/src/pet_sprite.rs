//! 小蜡烛精灵的运行时渲染：调色板索引 → RGBA，供桌面窗口与领养预览共用。
//!
//! 像素数据在 [`crate::pet_sprite_data`]（由 gen_sprites.py 从基准稿生成，
//! 25×50、7 色锁定调色板）。个体随机 = 换色板 + 蜡泪/托盘格子叠加，
//! 不需要额外图片文件。

pub use crate::pet_sprite_data::{
    FRAME_FLICKER, FRAME_IDLE, FRAME_LEN, FRAME_SLEEP, GRID_H, GRID_W,
};

pub const FRAME_COUNT: usize = 3;

/// 精灵调色板：下标 0 为透明占位，1..=7 对应数据文件中的索引。
pub struct SpritePalette {
    pub body: (u8, u8, u8),
    pub highlight: (u8, u8, u8),
    pub shade: (u8, u8, u8),
    pub flame_core: (u8, u8, u8),
    pub flame: (u8, u8, u8),
    pub flame_deep: (u8, u8, u8),
    pub outline: (u8, u8, u8),
}

/// 通知火苗强化档位：0 无 / 1 新动静与需要注意 / 2 警告。
pub const FLAME_BOOST_NONE: u8 = 0;
pub const FLAME_BOOST_ATTENTION: u8 = 1;
pub const FLAME_BOOST_WARNING: u8 = 2;

/// 基准调色板（candle-4 提取色值，描边与眼固定不变）。
const BASE: SpritePalette = SpritePalette {
    body: (255, 246, 239),
    highlight: (255, 252, 237),
    shade: (247, 234, 210),
    flame_core: (255, 225, 130),
    flame: (255, 172, 4),
    flame_deep: (255, 109, 0),
    outline: (102, 26, 10),
};

/// 蜡身色板变体（coat_color 0..=2）：奶白 / 暖米 / 奶茶。
const BODY_VARIANTS: [[(u8, u8, u8); 3]; 3] = [
    [(255, 246, 239), (255, 252, 237), (247, 234, 210)],
    [(247, 234, 210), (255, 246, 224), (232, 210, 174)],
    [(239, 217, 188), (249, 236, 212), (217, 190, 150)],
];

/// 火苗色板变体（accent 0..=1）：暖橙 / 蜜金。
const FLAME_VARIANTS: [[(u8, u8, u8); 3]; 2] = [
    [(255, 225, 130), (255, 172, 4), (255, 109, 0)],
    [(255, 243, 184), (255, 211, 78), (255, 176, 0)],
];

/// 通知火苗强化色（越紧急越偏红亮）。
const FLAME_BOOSTS: [[(u8, u8, u8); 3]; 2] = [
    [(255, 246, 216), (255, 194, 78), (255, 142, 46)],
    [(255, 216, 160), (255, 142, 46), (232, 60, 0)],
];

/// 按候选外观与通知档位选择调色板。
pub fn palette_for(coat_color: u8, accent: u8, flame_boost: u8) -> SpritePalette {
    let body = BODY_VARIANTS[(coat_color % 3) as usize];
    let flame = if flame_boost == FLAME_BOOST_NONE {
        FLAME_VARIANTS[(accent % 2) as usize]
    } else {
        FLAME_BOOSTS[(flame_boost - 1) as usize]
    };
    SpritePalette {
        body: body[0],
        highlight: body[1],
        shade: body[2],
        flame_core: flame[0],
        flame: flame[1],
        flame_deep: flame[2],
        outline: BASE.outline,
    }
}

/// 组合帧：叠加蜡泪（marking 1 单侧 / 2 双侧）与托盘（body 1 浅 / 2 高）。
pub fn compose_frame(frame: &[u8], marking: u8, body: u8) -> Vec<u8> {
    let mut cells = frame.to_vec();
    let mut put = |x: usize, y: usize, v: u8| cells[y * GRID_W + x] = v;
    if marking == 1 || marking == 2 {
        for y in 20..25 {
            put(5, y, 3); // 左侧蜡泪
        }
    }
    if marking == 2 {
        for y in 23..28 {
            put(19, y, 3); // 右侧蜡泪
        }
    }
    if body >= 1 {
        for x in 3..22 {
            put(x, 42, 3); // 浅托盘
        }
    }
    if body >= 2 {
        for x in 4..21 {
            put(x, 43, 3); // 高托盘
        }
    }
    cells
}

/// 渲染为 RGBA 缓冲（非预乘，长度 = 25×50×4）。
pub fn render_rgba(cells: &[u8], palette: &SpritePalette, out: &mut [u8]) {
    debug_assert_eq!(cells.len(), FRAME_LEN);
    debug_assert_eq!(out.len(), FRAME_LEN * 4);
    let colors: [(u8, u8, u8); 8] = [
        (0, 0, 0),
        palette.body,
        palette.highlight,
        palette.shade,
        palette.flame_core,
        palette.flame,
        palette.flame_deep,
        palette.outline,
    ];
    for (i, &idx) in cells.iter().enumerate() {
        let (r, g, b) = colors[idx as usize];
        out[i * 4] = r;
        out[i * 4 + 1] = g;
        out[i * 4 + 2] = b;
        out[i * 4 + 3] = if idx == 0 { 0 } else { 255 };
    }
}

/// 调色板颜色查询（索引 0 返回透明黑）。
pub fn palette_color(palette: &SpritePalette, idx: u8) -> (u8, u8, u8) {
    match idx {
        1 => palette.body,
        2 => palette.highlight,
        3 => palette.shade,
        4 => palette.flame_core,
        5 => palette.flame,
        6 => palette.flame_deep,
        7 => palette.outline,
        _ => (0, 0, 0),
    }
}

/// 领养预览用：每行输出色块行程 (格数, 调色板索引)，索引 0 为透明。
pub fn row_runs(cells: &[u8]) -> Vec<Vec<(usize, u8)>> {
    (0..GRID_H)
        .map(|y| {
            let row = &cells[y * GRID_W..(y + 1) * GRID_W];
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

pub fn frame(cells_index: usize) -> &'static [u8] {
    match cells_index {
        0 => &FRAME_IDLE,
        1 => &FRAME_FLICKER,
        _ => &FRAME_SLEEP,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composed_sleep_frame_keeps_closed_eyes() {
        let cells = compose_frame(&FRAME_SLEEP, 0, 0);
        // 睡眠帧闭眼线：各眼簇底行有描边色横线；原眼行已填蜡身
        assert_eq!(cells[30 * GRID_W + 14], 7);
        assert_eq!(cells[31 * GRID_W + 7], 7);
        assert_eq!(cells[28 * GRID_W + 14], 1);
    }

    #[test]
    fn marking_and_tray_add_cells() {
        let base = compose_frame(&FRAME_IDLE, 0, 0);
        let with = compose_frame(&FRAME_IDLE, 2, 2);
        let added = with
            .iter()
            .zip(base.iter())
            .filter(|(w, b)| **w != 0 && **b == 0)
            .count();
        assert!(added > 0, "蜡泪与托盘应新增不透明格子");
    }

    #[test]
    fn render_rgba_maps_palette_indices() {
        let mut cells = [0u8; FRAME_LEN];
        cells[0] = 1;
        cells[1] = 7;
        let mut out = [0u8; FRAME_LEN * 4];
        render_rgba(&cells, &BASE, &mut out);
        assert_eq!(&out[0..3], &[255, 246, 239]);
        assert_eq!(out[3], 255);
        assert_eq!(&out[4..8], &[102, 26, 10, 255]);
        assert_eq!(&out[8..12], &[0, 0, 0, 0]);
    }
}
