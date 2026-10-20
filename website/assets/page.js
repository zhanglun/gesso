/* 方案变体共用的活体壁纸运行时（精简版）：
   WebGL2 跑产品预置着色器 + Clock 网页壁纸 + 名字切换。
   着色器源码与 crates/app/assets/samples/shader/ 同源；完整版见 page.js。 */
(function () {
  "use strict";

  var SHADERS = {
    plasma:
"void mainImage(out vec4 fragColor, in vec2 fragCoord) {\n" +
"    vec2 uv = fragCoord / iResolution.xy;\n" +
"    vec2 p = (fragCoord * 2.0 - iResolution.xy) / min(iResolution.x, iResolution.y);\n" +
"    float t = iTime * 0.6;\n" +
"    float v = sin(p.x * 3.0 + t) + sin((p.y + t) * 2.0) + sin((p.x + p.y + t) * 2.0)\n" +
"            + sin(length(p + vec2(sin(t * 0.3), cos(t * 0.2))) * 4.0);\n" +
"    vec3 col = 0.5 + 0.5 * cos(t + v + vec3(0.0, 2.1, 4.2));\n" +
"    fragColor = vec4(col * (0.6 + 0.4 * uv.y), 1.0);\n" +
"}\n",
    aurora:
"void mainImage(out vec4 fragColor, in vec2 fragCoord) {\n" +
"    vec2 uv = fragCoord / iResolution.xy;\n" +
"    float t = iTime * 0.15;\n" +
"    float band = 0.0;\n" +
"    for (int i = 0; i < 3; i++) {\n" +
"        float fi = float(i);\n" +
"        float y = uv.y - 0.35 - fi * 0.10 - 0.12 * sin(uv.x * (3.0 + fi) + t * (1.0 + fi * 0.7));\n" +
"        band += 0.012 / max(abs(y), 0.004);\n" +
"    }\n" +
"    vec3 col = vec3(0.04, 0.10, 0.09) + vec3(0.10, 0.90, 0.55) * band;\n" +
"    col += vec3(0.03, 0.05, 0.06) * uv.y;\n" +
"    fragColor = vec4(col, 1.0);\n" +
"}\n",
    starfield:
"float hash(vec2 p) { return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }\n" +
"void mainImage(out vec4 fragColor, in vec2 fragCoord) {\n" +
"    vec2 uv = fragCoord / iResolution.xy;\n" +
"    vec3 col = vec3(0.012, 0.016, 0.035);\n" +
"    float n = sin(uv.x * 3.1 + iTime * 0.02) * cos(uv.y * 2.3 - iTime * 0.015)\n" +
"            + sin((uv.x + uv.y) * 4.7 + iTime * 0.011);\n" +
"    col += vec3(0.10, 0.13, 0.22) * (0.5 + 0.5 * n) * 0.35;\n" +
"    for (int layer = 1; layer <= 3; layer++) {\n" +
"        float scale = 30.0 * float(layer);\n" +
"        float cellPx = iResolution.y / scale;\n" +
"        vec2 g = fragCoord / cellPx;\n" +
"        vec2 id = floor(g);\n" +
"        vec2 f = fract(g) - 0.5;\n" +
"        float h = hash(id);\n" +
"        if (h > 0.80) {\n" +
"            vec2 offs = (vec2(hash(id + 7.3), hash(id + 3.1)) - 0.5) * 0.7;\n" +
"            float dPx = length(f - offs) * cellPx;\n" +
"            float tw = 0.55 + 0.45 * sin(iTime * (0.5 + h) + h * 40.0);\n" +
"            col += mix(vec3(0.82, 0.87, 1.0), vec3(1.0, 0.9, 0.75), h)\n" +
"                   * (smoothstep(1.8, 0.4, dPx) + smoothstep(5.0, 0.0, dPx) * 0.30) * tw / float(layer) * 1.3;\n" +
"        }\n" +
"    }\n" +
"    fragColor = vec4(col, 1.0);\n" +
"}\n",
    ember:
"float hash(vec2 p) { return fract(sin(dot(p, vec2(269.5, 183.3))) * 43758.5453); }\n" +
"void mainImage(out vec4 fragColor, in vec2 fragCoord) {\n" +
"    vec2 uv = fragCoord / iResolution.xy;\n" +
"    vec3 col = vec3(0.03, 0.02, 0.025);\n" +
"    col += vec3(0.35, 0.10, 0.04) * exp(-uv.y * 4.5) * (0.8 + 0.2 * sin(iTime * 0.7));\n" +
"    for (int layer = 0; layer < 2; layer++) {\n" +
"        float fl = float(layer);\n" +
"        float scale = 22.0 + fl * 10.0;\n" +
"        float cellPx = iResolution.y / scale;\n" +
"        vec2 g = fragCoord / cellPx;\n" +
"        g.y += iTime * (0.30 + 0.15 * fl);\n" +
"        g.x += 0.25 * sin(iTime * 0.4 + g.y * 0.8);\n" +
"        vec2 id = floor(g);\n" +
"        vec2 f = fract(g) - vec2(0.5, 0.0);\n" +
"        float h = hash(id + fl * 31.7);\n" +
"        if (h > 0.72) {\n" +
"            float life = fract(h * 13.0 + iTime * (0.08 + 0.05 * h));\n" +
"            float dPx = length(f - vec2(0.0, 0.30)) * cellPx;\n" +
"            float fade = (1.0 - life) * (0.5 + 0.5 * sin(iTime * 2.2 + h * 60.0));\n" +
"            vec3 tint = mix(vec3(1.0, 0.5, 0.12), vec3(1.0, 0.85, 0.45), h);\n" +
"            col += tint * (smoothstep(1.6, 0.3, dPx) + smoothstep(6.0, 0.0, dPx) * 0.35) * fade * 0.9;\n" +
"        }\n" +
"    }\n" +
"    fragColor = vec4(col, 1.0);\n" +
"}\n"
  };

  var WALLS = [
    { id: "plasma",    name: "Plasma",    kind: "shader", poster: "#2450d6" },
    { id: "aurora",    name: "Aurora",    kind: "shader", poster: "#0a1a17" },
    { id: "starfield", name: "Starfield", kind: "shader", poster: "#030412" },
    { id: "ember",     name: "Ember",     kind: "shader", poster: "#080506" },
    { id: "clock",     name: "Clock",     kind: "clock",  poster: "#101426" }
  ];

  var canvas = document.querySelector("[data-wall-canvas]");
  var frame = document.querySelector("[data-wall-frame]");
  var clockWall = document.querySelector("[data-clock-wall]");
  var clockTime = document.querySelector("[data-clock-time]");
  var clockDate = document.querySelector("[data-clock-date]");
  var buttons = Array.prototype.slice.call(document.querySelectorAll("[data-wall-name]"));
  var reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  var gl = canvas && canvas.getContext("webgl2", { antialias: false, alpha: false });
  var programs = {}, uni = null, index = 0, visible = true;
  var frameCount = 0, t0 = performance.now(), lastT = t0, clockTimer = 0;

  var VERT = "#version 300 es\nvoid main(){ vec2 p = vec2(float((gl_VertexID<<1)&2), float(gl_VertexID&2)); gl_Position = vec4(p*2.0-1.0, 0.0, 1.0); }\n";
  var HEAD = "#version 300 es\nprecision highp float;\nuniform vec3 iResolution;\nuniform float iTime;\nout vec4 outColor;\n";
  var MAIN = "void main(){ vec4 c; mainImage(c, gl_FragCoord.xy); outColor = c; }\n";

  function useShader(id) {
    if (!gl) return false;
    if (!programs[id]) {
      var ok = true;
      try {
        var mk = function (type, src) {
          var s = gl.createShader(type);
          gl.shaderSource(s, src);
          gl.compileShader(s);
          if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s) || "compile failed");
          return s;
        };
        var p = gl.createProgram();
        gl.attachShader(p, mk(gl.VERTEX_SHADER, VERT));
        gl.attachShader(p, mk(gl.FRAGMENT_SHADER, HEAD + SHADERS[id] + MAIN));
        gl.linkProgram(p);
        programs[id] = {
          p: p,
          res: gl.getUniformLocation(p, "iResolution"),
          time: gl.getUniformLocation(p, "iTime")
        };
      } catch (e) { ok = false; }
      if (!ok) programs[id] = false;
    }
    if (!programs[id]) return false;
    gl.useProgram(programs[id].p);
    uni = programs[id];
    return true;
  }

  function draw() {
    if (!gl || !uni) return;
    var dpr = Math.min(window.devicePixelRatio || 1, 1.5);
    var w = Math.max(2, Math.round(canvas.clientWidth * dpr));
    var h = Math.max(2, Math.round(canvas.clientHeight * dpr));
    if (canvas.width !== w || canvas.height !== h) { canvas.width = w; canvas.height = h; }
    gl.viewport(0, 0, canvas.width, canvas.height);
    gl.uniform3f(uni.res, canvas.width, canvas.height, 1.0);
    gl.uniform1f(uni.time, (performance.now() - t0) / 1000);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    frameCount++;
    lastT = performance.now();
  }

  function pad(n) { return (n < 10 ? "0" : "") + n; }
  function tickClock() {
    var d = new Date();
    if (clockTime) clockTime.textContent = pad(d.getHours()) + ":" + pad(d.getMinutes()) + ":" + pad(d.getSeconds());
    if (clockDate) {
      var week = ["日", "一", "二", "三", "四", "五", "六"][d.getDay()];
      clockDate.textContent = d.getFullYear() + " 年 " + (d.getMonth() + 1) + " 月 " + d.getDate() + " 日 · 周" + week;
    }
  }

  function setWall(i) {
    index = (i + WALLS.length) % WALLS.length;
    var w = WALLS[index];
    var isClock = w.kind === "clock";
    if (clockWall) clockWall.hidden = !isClock;
    if (canvas) canvas.style.display = isClock ? "none" : "";
    if (isClock) { tickClock(); if (!clockTimer) clockTimer = setInterval(tickClock, 1000); }
    else if (clockTimer) { clearInterval(clockTimer); clockTimer = 0; }
    var ok = !isClock && useShader(w.id);
    if ((!ok && !isClock) || reduced) {
      if (frame) frame.style.background = w.poster;
      if (canvas) canvas.style.visibility = ok && !reduced ? "visible" : "hidden";
    } else if (canvas) {
      canvas.style.visibility = "visible";
    }
    if (!isClock && ok) draw();
    buttons.forEach(function (b) {
      b.classList.toggle("is-active", b.getAttribute("data-wall-name") === w.id);
    });
  }

  buttons.forEach(function (b) {
    b.addEventListener("click", function () {
      setWall(WALLS.findIndex(function (w) { return w.id === b.getAttribute("data-wall-name"); }));
    });
  });

  if (canvas) {
    setWall(0);
    if ("IntersectionObserver" in window) {
      new IntersectionObserver(function (es) { visible = es[0].isIntersecting; }, { threshold: 0.02 })
        .observe(canvas);
    }
    (function loop() {
      if (visible && !document.hidden && !reduced && WALLS[index].kind === "shader" && uni) draw();
      requestAnimationFrame(loop);
    })();
    window.addEventListener("resize", function () { if (!reduced) draw(); });
  }
})();

/* ---------- 入场：瓷砖依次浮起 ---------- */
(function () {
  var tiles = Array.prototype.slice.call(document.querySelectorAll(".tile"));
  if (!tiles.length) return;
  if (!("IntersectionObserver" in window) || window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
    tiles.forEach(function (t) { t.classList.add("in"); });
    return;
  }
  var io = new IntersectionObserver(function (es) {
    es.forEach(function (e) {
      if (e.isIntersecting) { e.target.classList.add("in"); io.unobserve(e.target); }
    });
  }, { threshold: 0.08 });
  tiles.forEach(function (t, i) { t.style.setProperty("--d", Math.min(i, 6) * 70 + "ms"); io.observe(t); });
})();
