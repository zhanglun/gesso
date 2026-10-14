# Wallpaper Engine 素材兼容

> 状态：video/web 已交付；scene 解析仍在计划中。

## 目标

把用户**主动指定**的 Wallpaper Engine 素材目录作为壁纸来源之一。这从来不是“内置 WE 素材库”，而是“读取你已经拥有的东西”——唯一的准入事实是用户通过导入对话框自己选中条目的 `project.json`（典型来源：本机已订阅的创意工坊内容落盘目录）。Gesso **不检测也不依赖 Wallpaper Engine / Steam 是否安装**：不扫描磁盘、不枚举工坊、不解析 Steam 配置。

## 类型支持矩阵

| WE `type` | 内容形态 | 方案 | 状态 |
|---|---|---|---|
| `video` | mp4 | **零拷贝直引**（`gesso://steam` 流式 Range 读源，不拷入库） | ✅ 已交付 |
| `web` | HTML5 + WE JS API | **零拷贝直引**：`gesso://steam` 读原文件，主 HTML 在协议层内存注入 WE API shim（不改原文件），子资源相对路径同源加载 | ✅ 已交付 |
| `scene` | `scene.pkg`（贴图 `.tex`、时间轴、着色器） | 解包（`depkg`，GPL，子进程隔离）→ 轻量运行时；先行降级：抽取图片资产 | 计划中 |
| `application` | 原生 exe | **明确拒绝**（安全模型不允许，macOS 也无法运行） | 永不 |

## 素材从哪来

Steam 创意工坊订阅内容落盘于 `<steam>/steamapps/workshop/content/431960/<workshopid>/`（`431960` = Wallpaper Engine 的 AppID），每个目录含 `project.json`（`title` / `type` / `file` 等）。用户在导入对话框里自行浏览到该目录；Gesso 不枚举这个路径。

## 实现备忘

- 导入：用户**自己用导入对话框选中** WE 条目的 `project.json`（或整目录）；`we.rs::import_we_at` 解析 `project.json`。Gesso **不扫描磁盘、不枚举 Steam/工坊、不依赖 Steam 是否安装**。
- shim 注入点：主 HTML 文档在 gesso://steam 协议层内存注入（紧贴 `<head>` 之后，`we_shim.rs`）；原文件只读不动。
- 普通「导入」按钮选中 `project.json` 同样触发整目录 WE 导入。

## 法律边界（硬约束）

- **只读本机已订阅内容**；不做下载器、不做素材再分发、不把素材打进 release。
- 不关联、不破解 Steam，也不检测 WE/Steam 安装状态；没有素材目录就没有条目，不提供任何获取通路。
- GPL 边界：解包工具链（`depkg`/`repkg`/`PyPKGer`）均为 GPL。集成方式二选一——
  1. **子进程调用** `depkg-cli`（进程边界隔离 GPL，主程序保持宽松许可，当前倾向）；
  2. 接受整个项目 GPL-3.0。
  该方案尚未进入当前实现；在 scene 立项前重新评估，并在 README 明示。

## 验收

- 夹具端到端：零拷贝条目指向源目录、steam 路由内存注入 shim + 路径空间隔离、scene 明确拒绝均有测试覆盖；
- video/web 零拷贝直引实机验证（主文档 + 相对子资源均经 `gesso://steam/<entry-id>/…` 加载，0 panic）；以不可猜 entry id 为凭据，未知 id 回 404；
- 大规模工坊实机兼容性（热门 20 个 web）待有 Steam 环境补测。
