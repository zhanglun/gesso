# Gesso 产品图标

概念「底色画布」：gesso 白画布上，壁纸的黎明色自底部涌起，右上角一列桌面图标点仍在 —— 钉在图标层之下的产品事实。规格见 [docs/design/DESIGN.md §品牌图形](../../../../docs/design/DESIGN.md)，预览见 [docs/design/06-品牌图标.brand.html](../../../../docs/design/06-品牌图标.brand.html)。

## 布局

```
src/      源矢量 + 1024 master（app-icon.svg / app-icon-compact.svg / tray-*.svg）
mac/      icon.iconset/ + Gesso.icns（16 → 1024 全尺寸）
win/      gesso.ico（16/20/24/32/48/64/128/256，PNG 直嵌；**满幅稿**——Windows 图标不留 Big Sur 边距）
tray/     托盘图标 PNG：trayTemplate(.png 22pt / @2x 44px)、tray-*（单色字形，macOS 备用）、tray-app-32（Windows 托盘，满幅）
tools/    构建管线（sharp 光栅化 + iconutil + ICO 打包）
```

## 消费点

- **托盘**（`crates/app/src/main.rs::tray_icon_rgba`）：macOS 内嵌 `trayTemplate@2x.png`（44px，
  tray-icon 按菜单栏 22pt 约束 → Retina 清晰）+ template 标志随菜单栏亮暗自适应；
  Windows 内嵌 `tray-app-32.png`（彩色 compact 应用图标，与任务栏/exe 图标同稿——描边字形
  缩到托盘 16px 读不出，单色 template 机制仅 macOS，2026-10-06 用户决策）。
- **Dock**（`main.rs::apply_dock_icon`）：`mac/Gesso.icns` 经 `NSImage` 设为运行期应用图标；
  正式打包后由 bundle 的 icns 接管。
- **Windows 安装包/EXE**：`win/gesso.ico` 经 `build.rs`（winresource）以**资源 ID 1** 内嵌进 exe
  —— GPUI 启动时按 `MAKEINTRESOURCE(1)` 加载它作窗口类图标（任务栏/标题栏共用），资源缺失时
  静默降级为 Windows 通用图标；资源管理器里的 exe 文件图标同样来自这里。

## 再生成

```bash
cd crates/app/assets/icons/tools && npm install && node build.mjs
```

改图形先改 `docs/design/DESIGN.md §品牌图形` 与 `src/*.svg`（或 `tools/build.mjs` 内的 SVG 模板），
再跑管线；产出随仓库提交，运行时只依赖产出文件。
