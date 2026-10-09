// Gesso 预置壁纸 · Starfield
// 星野：三层网格星点（1-2px 硬核 + 小范围微光，呼吸式闪烁）+ 底层极淡星云。
// 星半径以像素计（iResolution 为物理像素），缩放下依旧锐利。
float hash(vec2 p) {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}
void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = fragCoord / iResolution.xy;
    vec3 col = vec3(0.012, 0.016, 0.035);
    // 星云：两组缓行的柔性明暗，几乎不可察觉地漂移
    float n = sin(uv.x * 3.1 + iTime * 0.02) * cos(uv.y * 2.3 - iTime * 0.015)
            + sin((uv.x + uv.y) * 4.7 + iTime * 0.011);
    col += vec3(0.10, 0.13, 0.22) * (0.5 + 0.5 * n) * 0.35;
    // 星点：三层网格（48/24/16px 单元 @1440p），越近层越疏越亮
    for (int layer = 1; layer <= 3; layer++) {
        float scale = 30.0 * float(layer);
        float cellPx = iResolution.y / scale;
        vec2 g = fragCoord / cellPx;
        vec2 id = floor(g);
        vec2 f = fract(g) - 0.5;
        float h = hash(id);
        if (h > 0.80) {
            vec2 offs = (vec2(hash(id + 7.3), hash(id + 3.1)) - 0.5) * 0.7;
            float dPx = length(f - offs) * cellPx;
            float tw = 0.55 + 0.45 * sin(iTime * (0.5 + h) + h * 40.0);
            float core = smoothstep(1.8, 0.4, dPx);
            float halo = smoothstep(5.0, 0.0, dPx) * 0.30;
            col += mix(vec3(0.82, 0.87, 1.0), vec3(1.0, 0.9, 0.75), h)
                   * (core + halo) * tw / float(layer) * 1.3;
        }
    }
    fragColor = vec4(col, 1.0);
}
