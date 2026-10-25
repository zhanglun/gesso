# Windows 宠物窗口自动化回归

开发机（真实桌面会话）上运行的行为回归，覆盖 P2-W 窗口层的关键路径：
渲染 / 单击戳 / 300ms 连点摸 / 拖拽与位置存档 / 注视方向 / 火苗摇曳 /
自主走动 / 入睡与唤醒 / 通知气泡 / 右键菜单选择。

## 前置

- debug 构建已启动且宠物已领养（存档 `~/.gesso/pet-save-v1.json`）。
  走动/入睡阶段需要存档 `activity: "quiet"`（30s 走 / 45s 睡）；
  气泡阶段用 `GESSO_PET_NOTICE=1` 启动（注入一次通知）。
- 建议用 `GESSO_LOCK=dev` 与日常实例并存；运行期间不要动鼠标（合成输入
  会与真实输入互相干扰）。
- 验证期间杀掉其他 gesso 测试实例，避免宠物窗口归属歧义。

## 用法

```powershell
# 逐阶段（每个阶段独立可重跑）
powershell -ExecutionPolicy Bypass -File tools/pet-verify/regress.ps1 -Phase render
# 阶段：render poke touch drag gaze flicker walksleep wake bubble devimport bubblecap menu menuuia menugeo
```

- 戳/摸/拖拽会改写存档统计与位置——测试存档别用真实领养数据。
- `walksleep`/`wake` 是一对（约 50s，先走后睡再唤醒），需连续运行。
- `bubble` 假定实例以 `GESSO_PET_NOTICE=1` 启动且开机 <7s；
  `menugeo` + `bubblecap` 组合验证「菜单选择 → 通知 → 气泡」全链路
  （菜单命令是否分发要看应用日志，需 `GESSO_PET_HIT_DIAG=1`）。
- 坐标全部按窗口 116×164pt 比例换算，不依赖 DPI；像素断言基于
  PrintWindow（PW_RENDERFULLCONTENT），被遮挡也能拍。
