// Gesso 内置样例 2/3 · Aurora
// 缓动光带；无纹理依赖。
void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = fragCoord / iResolution.xy;
    float t = iTime * 0.15;
    float band = 0.0;
    for (int i = 0; i < 3; i++) {
        float fi = float(i);
        float y = uv.y - 0.35 - fi * 0.10
                - 0.12 * sin(uv.x * (3.0 + fi) + t * (1.0 + fi * 0.7));
        band += 0.012 / max(abs(y), 0.004);
    }
    vec3 col = vec3(0.04, 0.10, 0.09) + vec3(0.10, 0.90, 0.55) * band;
    col += vec3(0.03, 0.05, 0.06) * uv.y;
    fragColor = vec4(col, 1.0);
}
