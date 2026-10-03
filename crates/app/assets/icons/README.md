# Gesso 产品图标

概念「底色画布」：gesso 白画布上，壁纸的黎明色自底部涌起，右上角一列桌面图标点仍在 —— 钉在图标层之下的产品事实。规格见 [docs/design/DESIGN.md §品牌图形](../../../../docs/design/DESIGN.md)，预览见 [docs/design/06-品牌图标.brand.html](../../../../docs/design/06-品牌图标.brand.html)。

## 布局

```
src/      源矢量 + 1024 master（app-icon.svg / app-icon-compact.svg / tray-*.svg）
mac/      icon.iconset/ + Gesso.icns（16 → 1024 全尺寸）
win/      gesso.ico（16/20/24/32/48/64/128/256，PNG 直嵌）
tray/     托盘字形 PNG：trayTemplate(.png 22pt / @2x 44px)、tray-white-*、tray-black-32
tools/    构建管线（sharp 光栅化 + iconutil + ICO 打包）
```

## 消费点

- **托盘**（`crates/app/src/main.rs::tray_icon_rgba`）：macOS 内嵌 `trayTemplate@2x.png`（44px，
  tray-icon 按菜单栏 22pt 约束 → Retina 清晰）+ template 标志随菜单栏亮暗自适应；
  Windows 内嵌 `tray-white-32.png`（深色任务栏）。
- **Dock**（`main.rs::apply_dock_icon`）：`mac/Gesso.icns` 经 `NSImage` 设为运行期应用图标；
  正式打包后由 bundle 的 icns 接管。
- **Windows 安装包/EXE**：`win/gesso.ico` 供 winresource（build.rs）内嵌，Windows 验证时接线。

## 再生成

```bash
cd crates/app/assets/icons/tools && npm install && node build.mjs
```

改图形先改 `docs/design/DESIGN.md §品牌图形` 与 `src/*.svg`（或 `tools/build.mjs` 内的 SVG 模板），
再跑管线；产出随仓库提交，运行时只依赖产出文件。
