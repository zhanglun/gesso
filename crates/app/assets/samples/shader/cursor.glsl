// Cursor Glow —— 光标跟随样例（Gesso 内置）。
// iMouse.xy 为当前光标位置（CSS 像素，顶左原点，M5 光标桥喂入）。
// 一团柔光始终聚在光标周围；无点击交互，z/w 不用。
void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = fragCoord / iResolution.xy;

    float d = distance(fragCoord, iMouse.xy);
    float glow = exp(-d * d / (2.0 * 90.0 * 90.0));

    vec3 base = mix(vec3(0.040, 0.045, 0.070), vec3(0.090, 0.110, 0.180), uv.y);
    // 缓慢漂移的底纹，纯静态背景太死板
    base += 0.02 * vec3(sin(iTime + uv.x * 6.0), sin(iTime * 0.8 + uv.y * 5.0), 0.5);

    vec3 light = vec3(0.55, 0.75, 1.0);
    vec3 col = base + glow * light;

    fragColor = vec4(col, 1.0);
}
