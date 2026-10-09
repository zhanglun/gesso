// Gesso 预置壁纸 · Tide
// 深海潮汐：iChannel0 噪声 fbm 叠浪，缓慢涌动的海面与波峰高光。
float noise(vec2 p) {
    return texture(iChannel0, p / 256.0).r;
}
float fbm(vec2 p) {
    float v = 0.0;
    float a = 0.5;
    for (int i = 0; i < 5; i++) {
        v += a * noise(p);
        p = p * 2.0 + 19.0;
        a *= 0.5;
    }
    return v;
}
void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = fragCoord / iResolution.xy;
    float t = iTime * 0.05;
    vec2 p = uv * vec2(3.0, 1.6);
    float h = fbm(p + vec2(t, t * 0.4)) + 0.4 * fbm(p * 2.3 - vec2(t * 1.7, 0.0));
    vec3 deep = vec3(0.01, 0.09, 0.16);
    vec3 crest = vec3(0.35, 0.75, 0.80);
    vec3 col = mix(deep, crest, smoothstep(0.35, 1.15, h));
    // 波峰高光：海面抬升到峰线处泛起一道冷光
    float spec = pow(clamp(1.0 - abs(h - 0.9) * 6.0, 0.0, 1.0), 3.0);
    col += vec3(0.5, 0.7, 0.7) * spec * 0.35;
    col *= 0.75 + 0.25 * uv.y;
    fragColor = vec4(col, 1.0);
}
