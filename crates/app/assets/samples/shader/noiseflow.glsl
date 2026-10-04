// Gesso 内置样例 3/3 · Noise Flow
// 依赖 iChannel0 预设噪声纹理（Gesso 宿主页内置 256x256 RGBA 噪声，REPEAT 包装）：
// 白噪声 → fbm → 域扭曲流场。对应 M4 DoD「shader 样例须含 iChannel 纹理」。
float noise(vec2 p) {
    return texture(iChannel0, p / 256.0).r;
}
float fbm(vec2 p) {
    float v = 0.0;
    float a = 0.5;
    for (int i = 0; i < 5; i++) {
        v += a * noise(p);
        p = p * 2.0 + 17.0;
        a *= 0.5;
    }
    return v;
}
void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = fragCoord / iResolution.xy;
    vec2 q = uv * 3.0;
    q += 1.5 * vec2(fbm(q + iTime * 0.15), fbm(q + vec2(5.2) - iTime * 0.12));
    float f = fbm(q * 2.0);
    vec3 col = mix(vec3(0.10, 0.16, 0.30), vec3(0.45, 0.75, 0.85), f);
    col = mix(col, vec3(0.95, 0.85, 0.65), smoothstep(0.62, 0.85, f));
    fragColor = vec4(col * (0.35 + 0.65 * uv.y), 1.0);
}
