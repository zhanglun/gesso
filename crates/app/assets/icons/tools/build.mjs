#!/usr/bin/env node
// Gesso 产品图标构建管线
// 用法: cd tools && npm install && node build.mjs
// 产出: ../src/*.svg (源矢量)、../mac (iconset + icns)、../win/gesso.ico、../tray/*.png
// 依赖: sharp(SVG 光栅化)、系统 iconutil(macOS 自带)
import { execSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import sharp from "sharp";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const SRC = join(ROOT, "src");
const MAC = join(ROOT, "mac", "icon.iconset");
const WIN = join(ROOT, "win");
const TRAY = join(ROOT, "tray");
for (const d of [SRC, MAC, WIN, TRAY]) mkdirSync(d, { recursive: true });

// ---------- 几何:Big Sur 风格方圆角(superellipse 采样) ----------
// 1024 画布,824 图形区(四周 100 留白),指数 n=5 逼近 Apple 连续曲率。
function squirclePath(cx, cy, a, n = 5, steps = 720) {
  const pts = [];
  for (let i = 0; i < steps; i++) {
    const t = (i / steps) * Math.PI * 2;
    const c = Math.cos(t), s = Math.sin(t);
    pts.push([
      cx + a * Math.sign(c) * Math.abs(c) ** (2 / n),
      cy + a * Math.sign(s) * Math.abs(s) ** (2 / n),
    ]);
  }
  return "M" + pts.map(([x, y]) => `${x.toFixed(2)},${y.toFixed(2)}`).join("L") + "Z";
}
const SQ = squirclePath(512, 512, 412);

// ---------- 应用图标 master(1024,含透明边距;Windows 直接复用) ----------
// 概念「底色画布 · 潮升」:gesso 白画布上,黎明大浪立起,半轮日出从浪后升起。
// 二稿(2026-10-03,用户反馈驱动):移除桌面图标点(叙事未传达)、浪幅加倍、日出为焦点;
// 托盘弃双矩形(撞系统「屏幕镜像」)改「屏中有浪」。
// compact = ≤32px 小尺寸稿:浪位整体抬高(-78),去光晕/浪尖线,保留日出(16px = 橙点记忆)。
function appSvg(compact) {
  const dy = compact ? -78 : 0;
  const front = `M100,${742 + dy} C300,${694 + dy} 460,${796 + dy} 620,${742 + dy} C720,${708 + dy} 790,${632 + dy} 870,${642 + dy} C896,${646 + dy} 914,${656 + dy} 924,${664 + dy}`;
  const back = `M100,${700 + dy} C340,${660 + dy} 470,${760 + dy} 610,${712 + dy} C700,${682 + dy} 760,${600 + dy} 840,${606 + dy} C878,${610 + dy} 906,${628 + dy} 924,${644 + dy}`;
  const sunCy = 640 + dy;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
  <defs>
    <clipPath id="sq"><path d="${SQ}"/></clipPath>
    <!-- gesso 白:暖调,上亮下沉,像打好底料的画布 -->
    <linearGradient id="gesso" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#FDFCF9"/>
      <stop offset="0.62" stop-color="#F6F4EF"/>
      <stop offset="1" stop-color="#EFEBE3"/>
    </linearGradient>
    <!-- 顶部内侧阴影(Big Sur 规范) -->
    <linearGradient id="topshade" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#3A3226" stop-opacity="0.10"/>
      <stop offset="1" stop-color="#3A3226" stop-opacity="0"/>
    </linearGradient>
    <!-- 黎明渐变:蓝主导 → 紫 → 暖收边(浪体,左下→右上) -->
    <linearGradient id="dawn" x1="0" y1="1" x2="1" y2="0">
      <stop offset="0" stop-color="#2450D6"/>
      <stop offset="0.38" stop-color="#316EF5"/>
      <stop offset="0.7" stop-color="#7C5CE0"/>
      <stop offset="1" stop-color="#E08A4E"/>
    </linearGradient>
    <linearGradient id="dawnBack" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0" stop-color="#7FA8F2"/>
      <stop offset="0.55" stop-color="#9C7BE8"/>
      <stop offset="1" stop-color="#F0B878"/>
    </linearGradient>
    <!-- 日出 -->
    <linearGradient id="sun" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#F8D9A8"/>
      <stop offset="1" stop-color="#E88C4C"/>
    </linearGradient>
    <!-- 浪尖高光 -->
    <linearGradient id="crest" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0" stop-color="#AECBFA"/>
      <stop offset="0.7" stop-color="#CDB9F4"/>
      <stop offset="1" stop-color="#F6D3AC"/>
    </linearGradient>
    <!-- 浪体上缘的晨光水色 -->
    <linearGradient id="sheen" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#FFFFFF" stop-opacity="0.2"/>
      <stop offset="0.45" stop-color="#FFFFFF" stop-opacity="0"/>
    </linearGradient>
    <filter id="soft" x="-40%" y="-40%" width="180%" height="180%">
      <feGaussianBlur stdDeviation="40"/>
    </filter>
  </defs>

  <g clip-path="url(#sq)">
    <rect width="1024" height="1024" fill="url(#gesso)"/>
    <rect width="1024" height="150" fill="url(#topshade)"/>

    <!-- 日出光晕 + 半轮日(浪随后盖住下缘 → 半升) -->
    <circle cx="668" cy="${sunCy}" r="140" fill="#F0B878" opacity="0.5" filter="url(#soft)"/>
    <circle cx="668" cy="${sunCy}" r="98" fill="url(#sun)"/>

    <!-- 后层浪(远处的暖色,在浪峰右上方露出暖沿) -->
    <path d="${back} L924,924 L100,924 Z" fill="url(#dawnBack)" opacity="0.85"/>

    <!-- 前层浪(黎明主体,峰在右) -->
    <path d="${front} L924,924 L100,924 Z" fill="url(#dawn)"/>
    <path d="${front} L924,924 L100,924 Z" fill="url(#sheen)"/>
    ${compact ? "" : `
    <!-- 浪尖高光线 -->
    <path d="${front}" fill="none" stroke="url(#crest)" stroke-width="7" stroke-linecap="round" opacity="0.9"/>
    `}
  </g>
</svg>`;
}

// ---------- 托盘字形(viewBox 24,「屏中有浪」) ----------
// 圆角屏描边 + 内腔底部实浪:读作「显示器 + 活的桌面」。
// 刻意避开双/错位矩形 —— macOS 菜单栏「屏幕镜像」系统字形即双圆角矩形,不能撞。
// 纯黑+alpha = macOS template;白色版供 Windows 深色任务栏。
function traySvg(fill) {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24">
  <defs>
    <clipPath id="in"><rect x="5.2" y="6.2" width="13.6" height="11.6" rx="2.6"/></clipPath>
  </defs>
  <rect x="3.7" y="4.7" width="16.6" height="14.6" rx="4" fill="none" stroke="${fill}" stroke-width="2.2"/>
  <g clip-path="url(#in)">
    <path d="M4,14.2 C6.5,12.6 8.5,15.4 11,14.2 C13.5,13 15.5,15.6 20,13.6 L20,19 L4,19 Z" fill="${fill}"/>
  </g>
</svg>`;
}

// ---------- 渲染 ----------
async function render(svg, size, out) {
  const buf = await sharp(Buffer.from(svg), { density: 300 })
    .resize(size, size, { kernel: "lanczos3" })
    .png()
    .toBuffer();
  await sharp(buf).toFile(out);
  return buf;
}

// ---------- ICO 打包(PNG 直嵌,兼容 Vista+) ----------
function packIco(entries) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0); header.writeUInt16LE(1, 2); header.writeUInt16LE(entries.length, 4);
  const dir = Buffer.alloc(16 * entries.length);
  let offset = 6 + dir.length; const blobs = [];
  entries.forEach(([size, png], i) => {
    const e = dir.subarray(i * 16);
    e.writeUInt8(size >= 256 ? 0 : size, 0);
    e.writeUInt8(size >= 256 ? 0 : size, 1);
    e.writeUInt8(0, 2); e.writeUInt8(0, 3);
    e.writeUInt16LE(1, 4); e.writeUInt16LE(32, 6);
    e.writeUInt32LE(png.length, 8); e.writeUInt32LE(offset, 12);
    offset += png.length; blobs.push(png);
  });
  return Buffer.concat([header, dir, ...blobs]);
}

// ---------- 主流程 ----------
console.log("→ 渲染应用图标…");
const STD = appSvg(false), SMALL = appSvg(true);
const master1024 = await render(STD, 1024, join(SRC, "app-icon@1024.png"));
writeFileSync(join(SRC, "app-icon.svg"), STD);
writeFileSync(join(SRC, "app-icon-compact.svg"), SMALL);
// >32 从 master 缩(边缘一致);≤32 用 compact 稿直出
const fromMaster = async (s) =>
  sharp(master1024).resize(s, s, { kernel: "lanczos3" }).png().toBuffer();

console.log("→ macOS iconset + icns…");
const iconset = [
  ["icon_16x16.png", 16], ["icon_16x16@2x.png", 32],
  ["icon_32x32.png", 32], ["icon_32x32@2x.png", 64],
  ["icon_128x128.png", 128], ["icon_128x128@2x.png", 256],
  ["icon_256x256.png", 256], ["icon_256x256@2x.png", 512],
  ["icon_512x512.png", 512], ["icon_512x512@2x.png", 1024],
];
for (const [name, size] of iconset) {
  if (size <= 32) {
    await render(SMALL, size, join(MAC, name));
  } else {
    const png = await fromMaster(size);
    writeFileSync(join(MAC, name), png);
  }
}
execSync(`iconutil -c icns "${MAC}" -o "${join(ROOT, "mac", "Gesso.icns")}"`);

console.log("→ Windows ico…");
const icoEntries = [];
for (const s of [16, 20, 24, 32, 48, 64, 128, 256]) {
  const png = s <= 32 ? await render(SMALL, s, join(SRC, `app-icon@${s}.png`)) : await fromMaster(s);
  icoEntries.push([s, png]);
}
writeFileSync(join(WIN, "gesso.ico"), packIco(icoEntries));

console.log("→ 托盘图标…");
const trayBlack = traySvg("#000000");
const trayWhite = traySvg("#FFFFFF");
writeFileSync(join(SRC, "tray-black.svg"), trayBlack);
writeFileSync(join(SRC, "tray-white.svg"), trayWhite);
// macOS template:22pt@1x/@2x;tray-icon 直接吃像素,代码里传 44px(22pt 约束,Retina 清晰)
await render(trayBlack, 22, join(TRAY, "trayTemplate.png"));
await render(trayBlack, 44, join(TRAY, "trayTemplate@2x.png"));
await render(trayBlack, 32, join(TRAY, "tray-32.png"));
// Windows:深色任务栏用白色;预留黑色版(浅色任务栏)
await render(trayWhite, 32, join(TRAY, "tray-white-32.png"));
await render(trayWhite, 24, join(TRAY, "tray-white-24.png"));
await render(trayBlack, 32, join(TRAY, "tray-black-32.png"));

console.log("✓ 全部产出完成:", ROOT);
