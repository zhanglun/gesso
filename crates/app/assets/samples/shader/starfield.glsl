// Gesso 预置壁纸 · Starfield
// 星野：网格哈希星点（亮度呼吸式闪烁）+ 底层极淡星云漂移，深夜色。
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
    // 星点：三层网格，越近层越疏越大；每颗星按自身相位呼吸
    for (int layer = 1; layer <= 3; layer++) {
        float scale = 24.0 * float(layer);
        vec2 g = fragCoord / iResolution.y * scale;
        vec2 id = floor(g);
        vec2 f = fract(g) - 0.5;
        float h = hash(id);
        if (h > 0.82) {
            vec2 offs = (vec2(hash(id + 7.3), hash(id + 3.1)) - 0.5) * 0.6;
            float d = length(f - offs);
            float tw = 0.6 + 0.4 * sin(iTime * (0.6 + h) + h * 40.0);
            float star = smoothstep(0.10 + 0.05 * h, 0.0, d) * tw / float(layer);
            col += mix(vec3(0.8, 0.85, 1.0), vec3(1.0, 0.9, 0.75), h) * star;
        }
    }
    fragColor = vec4(col, 1.0);
}
