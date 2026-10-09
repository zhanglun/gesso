// Gesso 预置壁纸 · Ember
// 余烬：自底部缓缓上升的火星微粒（1-2px 硬核 + 微光），呼吸式明灭，深夜色背景。
// 粒子半径以像素计（iResolution 为物理像素），缩放下依旧锐利。
float hash(vec2 p) {
    return fract(sin(dot(p, vec2(269.5, 183.3))) * 43758.5453);
}
void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = fragCoord / iResolution.xy;
    vec3 col = vec3(0.03, 0.02, 0.025);
    // 底部余温光晕
    col += vec3(0.35, 0.10, 0.04) * exp(-uv.y * 4.5) * (0.8 + 0.2 * sin(iTime * 0.7));
    // 两层火星：网格随时间上升并轻微摇曳，粒子按各自生命周期明灭
    for (int layer = 0; layer < 2; layer++) {
        float fl = float(layer);
        float scale = 22.0 + fl * 10.0;
        float cellPx = iResolution.y / scale;
        vec2 g = fragCoord / cellPx;
        g.y += iTime * (0.30 + 0.15 * fl);
        g.x += 0.25 * sin(iTime * 0.4 + g.y * 0.8);
        vec2 id = floor(g);
        vec2 f = fract(g) - vec2(0.5, 0.0);
        float h = hash(id + fl * 31.7);
        if (h > 0.72) {
            float life = fract(h * 13.0 + iTime * (0.08 + 0.05 * h));
            float dPx = length(f - vec2(0.0, 0.30)) * cellPx;
            float fade = (1.0 - life) * (0.5 + 0.5 * sin(iTime * 2.2 + h * 60.0));
            float core = smoothstep(1.6, 0.3, dPx);
            float halo = smoothstep(6.0, 0.0, dPx) * 0.35;
            vec3 tint = mix(vec3(1.0, 0.5, 0.12), vec3(1.0, 0.85, 0.45), h);
            col += tint * (core + halo) * fade * 0.9;
        }
    }
    fragColor = vec4(col, 1.0);
}
