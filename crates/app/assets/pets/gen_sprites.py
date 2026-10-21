#!/usr/bin/env python3
"""从基准稿 candle-4 生成 pet_sprite_data.rs（25×50 调色板索引数组）。

用法：python3 gen_sprites.py > ../../src/pet_sprite_data.rs
输入：同目录 candle-base.png（由 docs/design/drafts/candle-candidates/candle-4.png
      半尺寸后确认的网格基准；重生成基准稿时同步替换）。
调色板索引：0=透明 1=蜡身 2=高光 3=暗部 4=火苗亮芯 5=火苗橙 6=火苗深橙 7=描边/眼
"""
import sys

from PIL import Image

try:
    base = Image.open('candle-base.png').convert('RGBA')
except FileNotFoundError:
    base = Image.open('candle-idle-0.png').convert('RGBA')
w, h = base.size
assert (w, h) == (25, 50), f'基准稿必须是 25×50，当前 {w}×{h}'
px = base.load()

BODY = [(255, 246, 239), (255, 252, 237), (247, 234, 210)]
FLAME = [(255, 225, 130), (255, 172, 4), (255, 109, 0)]
OUTLINE = (102, 26, 10)


def classify_cell(gx, gy):
    """网格分类优先级：深色描边/眼 > 亮暖火苗 > 蜡身。"""
    x0, x1 = gx * w // 25, (gx + 1) * w // 25
    y0, y1 = gy * h // 50, (gy + 1) * h // 50
    dark = warm = 0
    rs = gs = bs = ta = n = 0
    for y in range(y0, y1):
        for x in range(x0, x1):
            r, g, b, a = px[x, y]
            if a <= 20:
                continue
            if r + g + b < 360:
                dark += 1
            if r - b > 80 and r > 180:
                warm += 1
            rs += r * a
            gs += g * a
            bs += b * a
            ta += a
            n += 1
    if ta == 0 or ta / n < 90:
        return 0
    avg = (rs // ta, gs // ta, bs // ta)
    if dark >= max(1, n // 8):
        return 7
    if warm >= max(1, n // 6):
        return 4 + min(range(3), key=lambda i: sum((a - b) ** 2 for a, b in zip(avg, FLAME[i])))
    return 1 + min(range(3), key=lambda i: sum((a - b) ** 2 for a, b in zip(avg, BODY[i])))


def despeckle(cells):
    """孤立格（8 邻域 ≤1 不透明）清除，两轮。"""
    for _ in range(2):
        kill = []
        for y in range(50):
            for x in range(25):
                if cells[y][x] == 0:
                    continue
                nb = sum(1 for dx in (-1, 0, 1) for dy in (-1, 0, 1)
                         if (dx or dy) and 0 <= x + dx < 25 and 0 <= y + dy < 50
                         and cells[y + dy][x + dx] != 0)
                if nb <= 1:
                    kill.append((x, y))
        for x, y in kill:
            cells[y][x] = 0


cells = [[classify_cell(x, y) for x in range(25)] for y in range(50)]

# 背景清除：以描边包围盒为界，盒外全部透明（盒内不碰，无泄漏可能）。
sevens = [(x, y) for y in range(50) for x in range(25) if cells[y][x] == 7]
r0 = min(y for _, y in sevens); r1 = max(y for _, y in sevens)
c0 = min(x for x, _ in sevens); c1 = max(x for x, _ in sevens)
for y in range(50):
    for x in range(25):
        if y < r0 or y > r1 or x < c0 or x > c1:
            cells[y][x] = 0
despeckle(cells)

# 帧1（摇曳）：火苗色板降一档 4→5、5→6（亮芯与橙各深一号），其余不变
cells_flicker = [[(c + 1 if c in (4, 5) else c) for c in row] for row in cells]

# 帧2（睡眠）：火苗全部降为深橙；擦净脸部原眼，按簇画闭眼线（线在眼底行）
cells_sleep = [[6 if c in (4, 5, 6) else c for c in row] for row in cells]
dark_pts = [(x, y) for y in range(25, 34) for x in range(5, 18) if cells_sleep[y][x] == 7]
# 按 x 连续性分簇（左眼 / 右眼 / 嘴等）
clusters = []
for x, y in sorted(dark_pts):
    placed = False
    for cl in clusters:
        if any(abs(x - px_) <= 2 for px_, _ in cl):
            cl.append((x, y))
            placed = True
            break
    if not placed:
        clusters.append([(x, y)])
for cl in clusters:
    ex0 = min(x for x, _ in cl)
    ex1 = max(x for x, _ in cl)
    ey1 = max(y for _, y in cl)
    for x, y in cl:
        cells_sleep[y][x] = 1
    for x in range(ex0, ex1 + 1):
        cells_sleep[ey1][x] = 7


def emit(name, grid_cells):
    flat = [grid_cells[y][x] for y in range(50) for x in range(25)]
    out = [f'pub const {name}: [u8; FRAME_LEN] = [']
    for y in range(50):
        row = flat[y * 25:(y + 1) * 25]
        out.append('    ' + ', '.join(map(str, row)) + ',')
    out.append('];')
    return '\n'.join(out)


header = '''//! 自动生成：assets/pets/gen_sprites.py 由基准稿转换，勿手改。
//! 索引：0=透明 1=蜡身 2=高光 3=暗部 4=火苗亮芯 5=火苗橙 6=火苗深橙 7=描边/眼。

pub const GRID_W: usize = 25;
pub const GRID_H: usize = 50;
pub const FRAME_LEN: usize = GRID_W * GRID_H;

'''

print(header)
print(emit('FRAME_IDLE', cells))
print()
print(emit('FRAME_FLICKER', cells_flicker))
print()
print(emit('FRAME_SLEEP', cells_sleep))
