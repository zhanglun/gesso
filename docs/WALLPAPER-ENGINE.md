# Wallpaper Engine 素材兼容

> 状态：**M6 已交付（video/web）**——scene（M7）仍计划中。

## 目标

在**用户本机已安装 Wallpaper Engine 并已订阅素材**的前提下，把这些素材作为壁纸来源之一。这从来不是"内置 WE 素材库"，而是"读取你已经拥有的东西"。

## 类型支持矩阵（计划）

| WE `type` | 内容形态 | 方案 | 里程碑 |
|---|---|---|---|
| `video` | mp4 | **零拷贝直引**（`gesso://steam` 流式 Range 读源，不拷入库） | ✅ M6+ |
| `web` | HTML5 + WE JS API | **零拷贝直引**：`gesso://steam` 读原文件，主 HTML 在协议层内存注入 WE API shim（不改原文件），子资源相对路径同源加载 | ✅ M6+ |
| `scene` | `scene.pkg`（贴图 `.tex`、时间轴、着色器） | 解包（`depkg`，GPL，子进程隔离）→ 轻量运行时；先行降级：抽取图片资产 | M7+ |
| `application` | 原生 exe | **明确拒绝**（安全模型不允许，macOS 也无法运行） | 永不 |

## 素材从哪来

Steam 创意工坊订阅内容落盘于 `<steam>/steamapps/workshop/content/431960/<workshopid>/`（`431960` = Wallpaper Engine 的 AppID），每个目录含 `project.json`（`title` / `type` / `file` 等）。

## 实现备忘（M6）

- 扫描：`we.rs::scan` 读 `project.json`；Steam 根定位走 `STEAM_DIR` → macOS 默认路径 → Windows 常见盘符。检测不到 Steam 时 UI 隐藏「工坊」入口。
- shim 注入点：主 HTML 文档在 gesso://steam 协议层内存注入（紧贴 `<head>` 之后，`we_shim.rs`）；原文件只读不动。
- 普通「导入」按钮选中 `project.json` 同样触发整目录 WE 导入。

## 法律边界（硬约束）

- **只读本机已订阅内容**；不做下载器、不做素材再分发、不把素材打进 release。
- 不关联、不破解 Steam；用户无 WE 时此功能不可用即可。
- GPL 边界：解包工具链（`depkg`/`repkg`/`PyPKGer`）均为 GPL。集成方式二选一——
  1. **子进程调用** `depkg-cli`（进程边界隔离 GPL，主程序保持宽松许可，当前倾向）；
  2. 接受整个项目 GPL-3.0。
  M6 开工前定案，并在 README 明示。

## 验收（M6 实测）

- 夹具端到端：零拷贝条目指向源目录、steam 路由内存注入 shim + 路径空间隔离、scene 明确拒绝均有测试覆盖；
- video/web 零拷贝直引实机验证（主文档 + 相对子资源均经 gesso://steam 加载，0 panic）；检测不到 Steam 时入口隐藏；
- 大规模工坊实机兼容性（热门 20 个 web）待有 Steam 环境补测。
