# Wallpaper Engine 素材兼容

> 状态：video/web 已交付；scene 一期基础图片层实时渲染已接入，效果栈与 `.pkg` 仍在计划中。

## 目标

把用户**主动指定**的 Wallpaper Engine 素材目录作为壁纸来源之一。这从来不是“内置 WE 素材库”，而是“读取你已经拥有的东西”——唯一的准入事实是用户通过导入对话框自己选中条目的 `project.json`（典型来源：本机已订阅的创意工坊内容落盘目录）。Gesso **不检测也不依赖 Wallpaper Engine / Steam 是否安装**：不扫描磁盘、不枚举工坊、不解析 Steam 配置。

## 类型支持矩阵

| WE `type` | 内容形态 | 方案 | 状态 |
|---|---|---|---|
| `video` | mp4 | **零拷贝直引**（`gesso://steam` 流式 Range 读源，不拷入库） | ✅ 已交付 |
| `web` | HTML5 + WE JS API | **零拷贝直引**：`gesso://steam` 读原文件，主 HTML 在协议层内存注入 WE API shim（不改原文件），子资源相对路径同源加载 | ✅ 已交付 |
| `scene` | `scene.json`（图层/材质/效果栈）+ 散装贴图 | **WebGL 运行时**：宿主页实时渲染基础图片层、关键帧与鼠标视差 | ✅ 一期已接入（效果栈等不支持项会跳过） |
| `scene`（`.pkg` 打包变体） | 贴图打包为 `scene.pkg`（`.tex` 压缩纹理） | 解包后再走上行流程（GPL 工具链子进程隔离，立项前重评） | 二期 |
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

## scene 方案设计（2026-11-10 定稿；一期已实现）

### 竞品现状（2026-11-10 调研）

**第三方壁纸软件中目前没有已知实现能在自家引擎里实时渲染 WE scene**：
Lively（最流行的开源动态壁纸）不支持，用户社区三年前就在求、只能转 mp4
（开源转换器 we-to-lively 实测也仅支持 mp4 类，scene 转换被禁用）；Sucrose
只兼容 WE 的 web 类型。唯一有此能力的第三方是 Phosphene（macOS，自研
Metal 渲染器烘焙成视频，非实时）。Gesso 的实时路线若做成，即是该领域最
完整的第三方实现。

### 路线选型：webview 内 WebGL 运行时（非烘焙、非原生 Metal）

参考 Phosphene（烘焙成视频，~5300 行 Swift/Metal）后否决烘焙路线：烘焙丢
失实时交互（鼠标视差、音频响应），且与 GPL 工具链纠缠更深。Gesso 钉窗里
跑的是 webview，**WebGL 是 scene 渲染的自然载体**——WE 场景本身就是分层
图像 + GLSL 效果栈，与浏览器渲染模型同构。纹理零拷贝直引走现有
`gesso://steam` 路由（scene 的贴图多为散装 png/jpg，无需解包；`.pkg` 打包
变体二期处理，GPL 子进程隔离方案不变）。

### 分期（每期可独立验收）

- **一期（基础场景，已接入）**：`scene.json` 解析（Rust 侧，进 `we.rs`）+
  宿主页 WebGL 场景运行时：图片层合成（placement/alignment/alpha/color/
  亮度/混合模式/zoom）+ **基础关键帧动画**（常量/线性插值，不含音频绑定）+
  相机视差（`cameraparallax` 总开关 + 层级 `parallaxDepth` 速率，偏移背离
  光标）+ 固定帧率。
  格式约定已与开源参考实现 wallpaper-scene-renderer（catsout）逐项核对：
  正交场景坐标**原点在画布左下、Y 向上**；对象 `origin` 为层中心、
  `alignment` 决定锚定边；层尺寸取值顺序 = 模型 json 宽高 > 对象 `size`
  > `origin×2`；`objects` 数组顺序即绘制顺序（先渲染在底层）；层自身混合
  来自材质 pass 的 `blending`（`colorBlendMode` 是颜色叠加效果的混合模式，
  一期随效果链跳过）。
  不支持项（效果链/粒子/文本/音频/3D）在**导入时如实告知**：Rust 快扫
  （`we::scene_scan`）识别阻断项（有则拒绝导入）与不支持项计数，导入成功
  后 toast 列出跳过内容（对齐 Phosphene 的 omissions 上报模式）。
  实现注意：① 同一格式双解析（Rust 导入校验 + JS 渲染）——用真实
  scene.json 黄金夹具回归测试防漂移；② 导入时提取效果清单与支持矩阵比对，
  库卡片如实显示「支持 X / 缺失 Y」——支持矩阵为 Rust/JS 共享常量；
  ③ scene 条目无现成缩略图，用导入时渲染一帧作卡片预览；④ 安全模型：
  scene 是「可信渲染器 + 不可信数据」（区别于 web 类型的不可信代码），
  实现严禁 eval/拼接场景数据。
- **二期（效果栈）**：WE GLSL 方言 → GLSL ES 翻译器（宿主页 JS 侧做源变
  换），效果链（waves/ripple/blur/glow 等常见效果）、copiesBackground、
  `.pkg` 解包（GPL 子进程隔离，立项前重评）。
- **三期/明确不支持**：粒子系统、音频响应、文本/时钟层、3D 模型、
  puppet-warp——不支持项在导入时如实告知（toast；一期已落地，阻断项如
  puppet/3D 相机则整张拒绝导入）。

### 选型代价与边界（诚实账，2026-11-10 审查补充）

- **兼容性上限**：WE 是活生态，官方持续出新效果——GLSL 方言翻译器是永久
  维护尾巴；每个未翻译的效果 = 该场景视觉降级（靠导入时 omissions 上报
  兜底）。本方案永远不是 100% WE 还原，用户预期管理是功能的一部分。
- **实时渲染性能**：重场景（多层 × 长效果链 × 4K）在弱机上可能达不到目标
  帧率；视频解码路线无此风险（但烘焙同样需要先实现渲染器，兼容性天花板
  相同且额外增加循环查找/编码机器，还丢失视差与无限动画）。现有空闲降帧
  与帧率上限作兜底。
- **逃生舱**：渲染器与烘焙共享同一前端——若实测证明复杂场景实时渲染不可
  用，可在同一渲染器上加「烘焙导出」（canvas 逐帧 → 视频），把重场景降级
  为视频壁纸。选型可逆，路线不可逆的部分只有「解析器 + 渲染器」本身。

### 安全边界（不可信输入，照搬 Phosphene 经验）

路径限制在条目目录内（`PathSafety.contained` 同款）；非有限数值丢弃；纹理
与渲染目标设字节预算；shader 尺寸与 include 深度上限；加载失败的贴图画黑。

### 法律边界

散装 scene（多数项目）无需解包工具，无 GPL 接触；`.pkg` 解包二期再评估
（depkg 子进程隔离 or 接受 GPL，现状倾向前者）。主程序保持 MIT OR Apache-2.0。

## 验收

- 夹具端到端：零拷贝条目指向源目录、steam 路由内存注入 shim + 路径空间隔离、scene 明确拒绝均有测试覆盖；
- video/web 零拷贝直引实机验证（主文档 + 相对子资源均经 `gesso://steam/<entry-id>/…` 加载，0 panic）；以不可猜 entry id 为凭据，未知 id 回 404；
- 大规模工坊实机兼容性（热门 20 个 web）待有 Steam 环境补测。
