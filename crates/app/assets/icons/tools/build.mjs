#!/usr/bin/env node
// Gesso 产品图标构建管线
// 用法: cd tools && npm install && node build.mjs
// 产出: ../src/*.svg (源矢量)、../mac (iconset + icns)、../win/gesso.ico、../tray/*.png
// 依赖: sharp(SVG 光栅化)、系统 iconutil(macOS 自带)
import { execSync } from "node:child_process";
import { mkdirSync, writeFileSync, rmSync } from "node:fs";
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
// 概念「底色画布」:gesso 白画布上,壁纸的黎明色自底部涌起;右上角几点桌面图标仍可见。
// compact = ≤32px 用的小尺寸简化稿:浪位抬高、去点/辉光/浪尖线(仿 Apple 小尺寸专用稿)。
function appSvg(compact) {
  const front = compact
    ? "M100,668 C280,628 460,682 640,648 C770,624 862,636 924,612"
    : "M100,752 C280,712 460,766 640,732 C770,708 862,720 924,696";
  const back = compact
    ? "M100,636 C320,608 430,650 580,630 C720,610 830,594 924,582"
    : "M100,716 C320,688 430,730 580,710 C720,690 830,674 924,662";
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
    <!-- 黎明渐变:蓝主导 → 紫 → 暖收边 -->
    <linearGradient id="dawn" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0" stop-color="#2E63E6"/>
      <stop offset="0.42" stop-color="#316EF5"/>
      <stop offset="0.72" stop-color="#7C5CE0"/>
      <stop offset="1" stop-color="#E08A4E"/>
    </linearGradient>
    <linearGradient id="dawnBack" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0" stop-color="#7FA8F2"/>
      <stop offset="0.55" stop-color="#9C7BE8"/>
      <stop offset="1" stop-color="#F0B878"/>
    </linearGradient>
    <!-- 浪尖高光 -->
    <linearGradient id="crest" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0" stop-color="#AECBFA"/>
      <stop offset="0.7" stop-color="#CDB9F4"/>
      <stop offset="1" stop-color="#F6D3AC"/>
    </linearGradient>
    <!-- 浪体上缘的晨光水色 -->
    <linearGradient id="sheen" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#FFFFFF" stop-opacity="0.22"/>
      <stop offset="0.5" stop-color="#FFFFFF" stop-opacity="0"/>
    </linearGradient>
    <filter id="soft" x="-30%" y="-30%" width="160%" height="160%">
      <feGaussianBlur stdDeviation="26"/>
    </filter>
  </defs>

  <g clip-path="url(#sq)">
    <rect width="1024" height="1024" fill="url(#gesso)"/>
    <rect width="1024" height="150" fill="url(#topshade)"/>

    ${compact ? "" : `
    <!-- 桌面图标仍在(右上角一列,mac 桌面默认排列位) -->
    <g fill="#DCD8CF">
      <rect x="744" y="156" width="42" height="42" rx="12"/>
      <rect x="744" y="232" width="42" height="42" rx="12"/>
      <rect x="744" y="308" width="42" height="42" rx="12"/>
    </g>

    <!-- 辉光 = 后层浪的模糊复制,色相永远与浪一致(漫上画布的地平线光) -->
    <use href="#back" transform="translate(0,-26)" filter="url(#soft)" opacity="0.5"/>
    `}

    <!-- 后层浪(远处的暖色) -->
    <path id="back" d="${back} L924,924 L100,924 Z" fill="url(#dawnBack)"/>

    <!-- 前层浪(黎明主体) -->
    <path d="${front} L924,924 L100,924 Z" fill="url(#dawn)"/>
    <path d="${front} L924,924 L100,924 Z" fill="url(#sheen)"/>
    ${compact ? "" : `
    <!-- 浪尖高光线 -->
    <path d="${front}" fill="none" stroke="url(#crest)" stroke-width="5" stroke-linecap="round" opacity="0.8"/>
    `}
  </g>
</svg>`;
}

// ---------- 托盘字形(viewBox 24,双层矩形:后层壁纸从前层桌面之下探出) ----------
// 纯黑+alpha = macOS template;白色版供 Windows 深色任务栏。
function traySvg(fill) {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24">
  <defs>
    <mask id="gap">
      <rect width="24" height="24" fill="white"/>
      <!-- 前层外扩 1.75 的留缝,保证 16px 下两层不粘连 -->
      <rect x="1.75" y="7" width="17" height="15.25" rx="5.25" fill="black"/>
    </mask>
  </defs>
  <rect x="9" y="3" width="11.5" height="9" rx="3" fill="${fill}" mask="url(#gap)"/>
  <rect x="3.5" y="8.75" width="13.5" height="11.75" rx="3.5" fill="${fill}"/>
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
// >32 从 master 缩(边缘一致);≤32 用简化稿直出
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
  const png = size <= 32
    ? await render(SMALL, size, join(MAC, name))
    : await fromMaster(size).then(b => { writeFileSync(join(MAC, name), b); return b; });
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
// macOS template:22pt@1x/@2x;tray-icon 直接吃像素,代码里选 22
await render(trayBlack, 22, join(TRAY, "trayTemplate.png"));
await render(trayBlack, 44, join(TRAY, "trayTemplate@2x.png"));
await render(trayBlack, 32, join(TRAY, "tray-32.png"));
// Windows:深色任务栏用白色;预留黑色版(浅色任务栏)
await render(trayWhite, 32, join(TRAY, "tray-white-32.png"));
await render(trayWhite, 24, join(TRAY, "tray-white-24.png"));
await render(trayBlack, 32, join(TRAY, "tray-black-32.png"));

console.log("✓ 全部产出完成:", ROOT);
