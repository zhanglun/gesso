#!/usr/bin/env node
// 三个候选方向渲染(不动 build.mjs,选定后才落地)
// 产出: ../../../../docs/design/drafts/{a,b,c}-*.png
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import sharp from "sharp";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const OUT = join(ROOT, "..", "..", "..", "..", "docs", "design", "drafts");
mkdirSync(OUT, { recursive: true });

function squirclePath(cx, cy, a, n = 5, steps = 720) {
  const pts = [];
  for (let i = 0; i < steps; i++) {
    const t = (i / steps) * Math.PI * 2;
    const c = Math.cos(t), s = Math.sin(t);
    pts.push([cx + a * Math.sign(c) * Math.abs(c) ** (2 / n), cy + a * Math.sign(s) * Math.abs(s) ** (2 / n)]);
  }
  return "M" + pts.map(([x, y]) => `${x.toFixed(2)},${y.toFixed(2)}`).join("L") + "Z";
}
const SQ = squirclePath(512, 512, 412);
const DEFS = `
    <clipPath id="sq"><path d="${SQ}"/></clipPath>
    <linearGradient id="dawn" x1="0" y1="1" x2="1" y2="0">
      <stop offset="0" stop-color="#2450D6"/>
      <stop offset="0.38" stop-color="#316EF5"/>
      <stop offset="0.7" stop-color="#7C5CE0"/>
      <stop offset="1" stop-color="#E08A4E"/>
    </linearGradient>
    <linearGradient id="dawnBack" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0" stop-color="#7FA8F2"/><stop offset="0.55" stop-color="#9C7BE8"/><stop offset="1" stop-color="#F0B878"/>
    </linearGradient>
    <linearGradient id="sun" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#F8D9A8"/><stop offset="1" stop-color="#E88C4C"/>
    </linearGradient>
    <linearGradient id="gesso" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#FDFCF9"/><stop offset="0.62" stop-color="#F6F4EF"/><stop offset="1" stop-color="#EFEBE3"/>
    </linearGradient>
    <linearGradient id="topshade" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#3A3226" stop-opacity="0.10"/><stop offset="1" stop-color="#3A3226" stop-opacity="0"/>
    </linearGradient>
    <linearGradient id="night" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#26262C"/><stop offset="1" stop-color="#15151A"/>
    </linearGradient>
    <linearGradient id="deepsea" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#161E3C"/><stop offset="1" stop-color="#0E1428"/>
    </linearGradient>
    <linearGradient id="crest" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0" stop-color="#AECBFA"/><stop offset="0.7" stop-color="#CDB9F4"/><stop offset="1" stop-color="#F6D3AC"/>
    </linearGradient>
    <filter id="b40" x="-40%" y="-40%" width="180%" height="180%"><feGaussianBlur stdDeviation="40"/></filter>
    <filter id="b26" x="-30%" y="-30%" width="160%" height="160%"><feGaussianBlur stdDeviation="26"/></filter>`;

// ── A 潮升:大浪 + 半轮日出,胆量版现有 DNA ──
const A = `<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
<defs>${DEFS}
  <linearGradient id="sheenA" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity="0.2"/><stop offset="0.45" stop-color="#fff" stop-opacity="0"/></linearGradient>
</defs>
<g clip-path="url(#sq)">
  <rect width="1024" height="1024" fill="url(#gesso)"/>
  <rect width="1024" height="150" fill="url(#topshade)"/>
  <circle cx="668" cy="640" r="140" fill="#F0B878" opacity="0.5" filter="url(#b40)"/>
  <circle cx="668" cy="640" r="98" fill="url(#sun)"/>
  <path d="M100,700 C340,660 470,760 610,712 C700,682 760,600 840,606 C878,610 906,628 924,644 L924,924 L100,924 Z" fill="url(#dawnBack)" opacity="0.85"/>
  <path id="wa" d="M100,742 C300,694 460,796 620,742 C720,708 790,632 870,642 C896,646 914,656 924,664 L924,924 L100,924 Z" fill="url(#dawn)"/>
  <path d="M100,742 C300,694 460,796 620,742 C720,708 790,632 870,642 C896,646 914,656 924,664 L924,924 L100,924 Z" fill="url(#sheenA)"/>
  <path d="M100,742 C300,694 460,796 620,742 C720,708 790,632 870,642 C896,646 914,656 924,664" fill="none" stroke="url(#crest)" stroke-width="7" stroke-linecap="round" opacity="0.9"/>
</g>
</svg>`;

// ── B 色叠:墨蓝场 + 双透明色纸 ──
const B = `<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024"><defs>${DEFS}
  <filter id="b30" x="-40%" y="-40%" width="180%" height="180%"><feGaussianBlur stdDeviation="30"/></filter>
</defs>
<g clip-path="url(#sq)">
  <rect width="1024" height="1024" fill="url(#deepsea)"/>
  <rect width="1024" height="150" fill="url(#topshade)" opacity="0.6"/>
  <ellipse cx="470" cy="700" rx="380" ry="330" fill="#000000" opacity="0.5" filter="url(#b40)"/>
  <g transform="rotate(-9 512 584)">
    <rect x="212" y="294" width="600" height="580" rx="130" fill="url(#dawn)"/>
    <rect x="212" y="294" width="600" height="580" rx="130" fill="none" stroke="#FFFFFF" stroke-opacity="0.28" stroke-width="3"/>
  </g>
  <g transform="rotate(7 610 420)">
    <rect x="350" y="160" width="520" height="520" rx="112" fill="#FFFFFF" opacity="0.12"/>
    <rect x="350" y="160" width="520" height="520" rx="112" fill="none" stroke="#FFFFFF" stroke-opacity="0.22" stroke-width="2.5"/>
  </g>
</g>
</svg>`;

// ── C G潮:墨场 + 字标 G,笔画下段化浪 ──
const C = `<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
<defs>${DEFS}
  <clipPath id="wav"><path d="M0,648 C240,606 420,676 640,638 C780,614 900,628 1024,610 L1024,1024 L0,1024 Z"/></clipPath>
</defs>
<g clip-path="url(#sq)">
  <rect width="1024" height="1024" fill="url(#night)"/>
  <rect width="1024" height="150" fill="url(#topshade)" opacity="0.5"/>
  <!-- G 本体:近白(圆弧 238 + 杠,笔画 108,圆端) -->
  <g fill="none" stroke="#EDEDEF" stroke-width="108" stroke-linecap="round">
    <path d="M 704.5 376.1 A 238 238 0 1 0 744.8 565.5"/>
  </g>
  <rect x="498" y="462" width="247" height="108" fill="#EDEDEF"/>
  <!-- 笔画下段 = 浪:同一 G 形重画,裁进浪区 -->
  <g clip-path="url(#wav)">
    <g fill="none" stroke="url(#dawn)" stroke-width="108" stroke-linecap="round">
      <path d="M 704.5 376.1 A 238 238 0 1 0 744.8 565.5"/>
    </g>
    <rect x="498" y="462" width="247" height="108" fill="url(#dawn)"/>
  </g>
</g>
</svg>`;

// ── 托盘字形(24 网格,黑 template)──
// A 屏中有浪:圆角方框描边 + 底部实浪 —— 不与「屏幕镜像」双矩形混淆
const trayA = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24">
<defs><clipPath id="in"><rect x="5.2" y="6.2" width="13.6" height="11.6" rx="2.6"/></clipPath></defs>
<rect x="3.7" y="4.7" width="16.6" height="14.6" rx="4" fill="none" stroke="#000" stroke-width="2.2"/>
<g clip-path="url(#in)"><path d="M4,14.2 C6.5,12.6 8.5,15.4 11,14.2 C13.5,13 15.5,15.6 20,13.6 L20,19 L4,19 Z" fill="#000"/></g>
</svg>`;
// B 分色方:实心圆角方 + 透明对角缝(对应双纸的对角构成)
const trayB = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24">
<defs><mask id="slit"><rect width="24" height="24" fill="#fff"/><line x1="4.6" y1="19.4" x2="19.4" y2="4.6" stroke="#000" stroke-width="1.8"/></mask></defs>
<rect x="4" y="4" width="16" height="16" rx="4" fill="#000" mask="url(#slit)"/>
</svg>`;
// C 字标 G:圆弧 + 杠
const trayC = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24">
<g fill="none" stroke="#000" stroke-width="2.6" stroke-linecap="round">
  <path d="M 16.67 8.08 A 6.1 6.1 0 1 0 17.92 13.48"/>
</g>
<rect x="12" y="10.65" width="5.5" height="2.7" fill="#000"/>
</svg>`;

async function render(svg, size, out) {
  const buf = await sharp(Buffer.from(svg), { density: 300 }).resize(size, size, { kernel: "lanczos3" }).png().toBuffer();
  await sharp(buf).toFile(out);
  return buf;
}
async function renderRect(svg, w, h, out) {
  const buf = await sharp(Buffer.from(svg), { density: 300 }).resize(w, h, { kernel: "lanczos3" }).png().toBuffer();
  await sharp(buf).toFile(out);
}

for (const [k, svg] of [["a", A], ["b", B], ["c", C]]) {
  writeFileSync(join(OUT, `${k}-app.svg`), svg);
  await render(svg, 512, join(OUT, `${k}-app-512.png`));
  await render(svg, 64, join(OUT, `${k}-app-64.png`));
  await render(svg, 32, join(OUT, `${k}-app-32.png`));
  await render(svg, 16, join(OUT, `${k}-app-16.png`));
}
for (const [k, svg] of [["a", trayA], ["b", trayB], ["c", trayC]]) {
  writeFileSync(join(OUT, `${k}-tray.svg`), svg);
  await render(svg, 22, join(OUT, `${k}-tray-22.png`));
  await render(svg, 44, join(OUT, `${k}-tray-44.png`));
}
console.log("✓ 三方案产出:", OUT);
