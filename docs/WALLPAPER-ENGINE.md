# Wallpaper Engine 素材兼容

> 状态：**计划中（M6/M7）**——本文档记录方案与边界，尚未实现。

## 目标

在**用户本机已安装 Wallpaper Engine 并已订阅素材**的前提下，把这些素材作为壁纸来源之一。这从来不是"内置 WE 素材库"，而是"读取你已经拥有的东西"。

## 类型支持矩阵（计划）

| WE `type` | 内容形态 | 方案 | 里程碑 |
|---|---|---|---|
| `video` | mp4 | **零拷贝直引**（库中登记引用，不复制文件） | M6 |
| `web` | HTML5 + WE JS API | 本地渲染 + 注入 WE API shim（`wallpaperRegisterAudioListener`、`wallpaperPropertyListener` 等，数据源为系统数据桥） | M6 |
| `scene` | `scene.pkg`（贴图 `.tex`、时间轴、着色器） | 解包（`depkg`，GPL，子进程隔离）→ 轻量运行时；先行降级：抽取图片资产 | M7+ |
| `application` | 原生 exe | **明确拒绝**（安全模型不允许，macOS 也无法运行） | 永不 |

## 素材从哪来

Steam 创意工坊订阅内容落盘于 `<steam>/steamapps/workshop/content/431960/<workshopid>/`（`431960` = Wallpaper Engine 的 AppID），每个目录含 `project.json`（`title` / `type` / `file` 等）。

## 法律边界（硬约束）

- **只读本机已订阅内容**；不做下载器、不做素材再分发、不把素材打进 release。
- 不关联、不破解 Steam；用户无 WE 时此功能不可用即可。
- GPL 边界：解包工具链（`depkg`/`repkg`/`PyPKGer`）均为 GPL。集成方式二选一——
  1. **子进程调用** `depkg-cli`（进程边界隔离 GPL，主程序保持宽松许可，当前倾向）；
  2. 接受整个项目 GPL-3.0。
  M6 开工前定案，并在 README 明示。

## 验收（届时）

- 热门 20 个 web 壁纸 ≥18 个可渲染（shim 缺失即失败）；
- video 类型全通；
- `application` 类型给出明确的"不支持"提示。
