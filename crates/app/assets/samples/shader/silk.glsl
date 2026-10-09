// Gesso 预置壁纸 · Silk
// 流光绸缎：两层域扭曲的正弦绸面，暮色金蓝渐变，光泽缓慢流动。
void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = fragCoord / iResolution.xy;
    vec2 p = (fragCoord * 2.0 - iResolution.xy) / min(iResolution.x, iResolution.y);
    float t = iTime * 0.08;
    vec2 q = p;
    q += 0.35 * vec2(sin(p.y * 1.7 + t * 2.0), cos(p.x * 1.5 - t * 1.6));
    q += 0.20 * vec2(sin((p.x + p.y) * 2.3 - t * 2.6), cos((p.x - p.y) * 2.1 + t * 2.2));
    float bands = sin(q.x * 2.4 + q.y * 3.2 + t * 3.0);
    float sheen = pow(0.5 + 0.5 * bands, 3.0);
    vec3 dusk = vec3(0.07, 0.10, 0.18);
    vec3 gold = vec3(0.85, 0.62, 0.34);
    vec3 blue = vec3(0.25, 0.45, 0.70);
    vec3 col = mix(dusk, blue, 0.5 + 0.5 * q.y);
    col = mix(col, gold, sheen * 0.55);
    col *= 0.7 + 0.3 * uv.y;
    fragColor = vec4(col, 1.0);
}
