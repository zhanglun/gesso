// Gesso 内置样例 1/3 · Plasma
// Shadertoy mainImage 子集风格；无纹理依赖，纯时域色场。
void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = fragCoord / iResolution.xy;
    vec2 p = (fragCoord * 2.0 - iResolution.xy) / min(iResolution.x, iResolution.y);
    float t = iTime * 0.6;
    float v = sin(p.x * 3.0 + t)
            + sin((p.y + t) * 2.0)
            + sin((p.x + p.y + t) * 2.0)
            + sin(length(p + vec2(sin(t * 0.3), cos(t * 0.2))) * 4.0);
    vec3 col = 0.5 + 0.5 * cos(t + v + vec3(0.0, 2.1, 4.2));
    fragColor = vec4(col * (0.6 + 0.4 * uv.y), 1.0);
}
