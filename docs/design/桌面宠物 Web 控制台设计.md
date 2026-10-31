# Gesso 桌面宠物 Web 控制台设计 v0.1

> 状态：**能力与交互规格草案，已确认本地 Server + 系统浏览器方向**。
> 自 2026-10-30 起本控制台是领养/设置/重置的唯一管理面（GPUI 领养小窗撤销、托盘合并单项、主窗口宠物页瘦身为展示+入口，见《桌面宠物设计.md》v1.1 修订）；原生宠物窗口与管理窗口其他页签不受影响。
> 相关产品规格：[桌面宠物设计.md](桌面宠物设计.md)；视觉 token：[DESIGN.md](DESIGN.md)。
>
> **Web 视觉基线 v0.3（2026-10-31 换世界，取代 v0.2「伴宠机」）**：用户看过三方向 demo 后改选**方向 3「图鉴观测站」**，并钉死新约束——**Web 页面本体必须是像素风**（像素字体、硬边、阶梯角、抖动纹理，不只是精灵是像素）。v0.2 伴宠机（实体设备拟物）随之废弃；v0.1 中性基线解除决议维持有效。现行世界 = 「**像素图鉴**」：纸墨图鉴的骨架（纸底、墨字、朱红唯一强调色、观测角标、条目编号）× 全页像素语法（中文像素字体、零圆角、2px 硬边框、硬像素投影、抖动底纹、steps() 步进动效）；观测台 = 深色像素 LCD 屏（只读舞台）。四页结构、全部文案与 Server 契约不变；`prefers-reduced-motion` 仍只降 Web 预览动画。字体：Fusion Pixel Font（缝合像素字体，OFL 许可，TakWolf/fusion-pixel-font）woff2 内嵌。方向决策记录见 `.impeccable/surfaces/crates-app-web-src-app-svelte.md`（开发用，不入产品事实源）。

## 0. 定位

**Gesso Pet Studio** 是运行在本机浏览器中的桌面宠物管理、领养和预览工作台。

它解决三件事：

1. 用 Web UI 承载领养、设置等复杂管理界面；
2. 用大画布预览宠物外观、动作和通知气泡；
3. 为正式美术资源接入提供动作、帧序和状态的验收面板。

它不是第二套宠物引擎，也不是远程控制服务。Rust 仍是唯一的状态、规则和存档真源。

## 1. 范围与不变项

### 1.1 本次新增

- 本地 loopback Web Server；
- 系统默认浏览器打开控制台；
- Web 版概览、领养、实验室、设置；其中 MVP 实验室只提供 `idle` / `sleep` / `blink` 受控预览；
- Web 写操作转换为已有 `EngineAction`；
- Web 读取引擎快照；
- 预览帧和状态展示；
- 三类模拟通知入口。

### 1.2 保持不变

- GPUI 管理窗口继续存在；
- 托盘「管理宠物…」（2026-10-30 起合并原「开始领养/宠物设置」二态项）与「显示/隐藏宠物」继续可用；
- macOS AppKit / Windows Win32 原生宠物窗口继续负责桌面显示；
- 穿透、拖动、置顶、全屏隐藏、多屏位置和 DPI 逻辑不迁移到 Web；
- Web 不直接读取或写入 `pet.json`、缓存目录或任意本地路径；
- Web 预览不会改变桌面宠物的真实行为状态，除非用户明确执行设置或模拟事件操作。
- Web 第一版的“减少动态”只影响浏览器内预览，并遵循浏览器 `prefers-reduced-motion`；它不是宠物存档设置，不改变桌面窗口行为。

### 1.3 首版不做

- 局域网监听、手机远程访问和云同步；
- 账号、登录和服务端；
- 自动捕获其他应用的系统通知；
- 外部宠物包导入；
- 自定义脚本、行为编辑器和任意文件浏览；
- 壁纸库管理；
- WebView 嵌入 Gesso 管理窗口；
- 用 WebView 替换桌面原生宠物窗口。

## 2. 产品结构

桌面端默认打开控制台后使用单页应用结构：

```text
Gesso Pet Studio
├── 概览
├── 领养
├── 宠物实验室
├── 设置
└── 关于与诊断（折叠信息，不作为首版主导航）
```

桌面宽屏使用左侧导航；窄屏使用顶部或底部导航。页面始终显示本机连接状态。

```text
┌────────────────────────────────────────────┐
│ Gesso Pet Studio          本机 · 已连接     │
├──────────────┬─────────────────────────────┤
│ 概览         │                             │
│ 领养         │          页面内容             │
│ 宠物实验室   │                             │
│ 设置         │                             │
│              │                             │
│ 当前宠物     │                             │
│ 小蜡烛       │                             │
└──────────────┴─────────────────────────────┘
```

## 3. 页面与能力

### 3.1 概览

概览是默认页面，目标是让用户一眼知道宠物当前是否存在、是否显示、正在做什么。

#### 已领养

展示：

- 当前宠物大尺寸预览；
- 名称和角色；
- 当前行为：Idle / Watching / Walking / Sleeping；
- 显示 / 隐藏状态；
- 尺寸和活跃度；
- 陪伴时间；
- 被戳、被摸统计。

“最后一次互动时间”不作为首版字段：当前存档只记录计数，不保存该时间戳；后续若产品需要，再单独扩展存档版本。

主要操作：

```text
[预览动作] [显示 / 隐藏] [宠物设置]
```

概览不放“重置宠物”这种破坏性操作。

#### 未领养

```text
还没有一位桌面居民
选择一个角色，生成三位候选，确认后它才会来到桌面。

[开始领养]
```

未领养时不显示宠物预览、不累计统计；页面不自动弹出领养流程。

### 3.2 领养

Web 版沿用现有领养状态机：

```text
NotAdopted
  → 角色选择
  → 候选生成
  → 候选选择
  → 保存中
  → Adopted
```

#### 角色选择

当前首发只有小蜡烛一张角色卡。页面保留可扩展的角色卡结构，但不制造不存在的多角色选择。

卡片内容：

- 稳定的角色预览；
- 角色名；
- 一句话性格；
- 代表性动作预览；
- “选择”按钮。

文案：

> 选择你想遇见的角色。确认领养后，普通流程中不能更换角色。

#### 候选生成

一次展示三个同角色候选。每张候选卡展示：

- 外观预览；
- 轻微 idle 动画；
- 中文名和英文名；
- 已解析外观特征摘要；
- 选中状态。

操作：

```text
[返回选择] [重新抽取]                         [确认领养]
```

规则：

- reroll 无用户可见次数上限；
- 生成期间禁用 reroll 和确认按钮；
- 一次只允许一个生成任务；
- 候选只在内存中存在，不写入正式存档；
- 关闭页面、退出流程或 reroll 后丢弃未确认候选；
- 只有确认成功后创建正式 `pet_id`、正式缓存和统计；
- 保存失败保留当前候选，显示原因并提供重试。

#### 确认领养

确认前明确说明：

- 当前个体会被固定；
- 普通流程不能换角色或重新生成；
- 宠物会出现在主屏右下角；
- 之后仍可修改尺寸、活跃度和显示状态。

成功后返回概览，并显示：

```text
欢迎来到桌面，小蜡烛。
[查看桌面宠物] [前往设置]
```

### 3.3 宠物实验室

宠物实验室是 Web 控制台最有价值的差异化页面，用于预览和验收，而不是控制桌面真实状态。

布局：

```text
┌────────────────────────────────┬──────────────────┐
│                                │ 自动播放         │
│                                │ [开 / 关]         │
│                                │                  │
│            宠物预览             │ 行为状态         │
│                                │ Idle             │
│                                │ Watching         │
│                                │ Walking          │
│                                │ Sleeping         │
│                                │                  │
│                                │ 动作             │
│                                │ Poke / Touch     │
│                                │ Blink / Gaze     │
└────────────────────────────────┴──────────────────┘
```

#### 预览状态

完整实验室（正式精灵资源接入后）支持：

- Idle；
- Watching；
- Walking；
- Sleeping；
- Poke；
- Touch；
- Dragging；
- Blink；
- GazeLeft；
- GazeRight；
- reduced-motion。

MVP 只实现 `idle`、`sleep`、`blink` 三种预览状态；其他状态在资源契约稳定后加入。

#### 两种模式

**自动播放**：MVP 只按真实节奏展示 `idle`、`blink`、`sleep`；正式精灵资源接入后再扩展到 watching、walking 等完整行为。

**手动检查**：MVP 固定检查 `idle`、`sleep`、`blink`；完整动作检查后置到正式资源验收阶段。

#### 预览背景

MVP 提供：

- 透明棋盘格；
- 浅色单色背景；
- 深色单色背景。

背景只用于检查 alpha、边缘和对比度，不模拟完整桌面或壁纸。

实验室预览不得修改桌面宠物真实状态。模拟通知是唯一例外，因为它是明确的用户操作，会通过引擎事件发送到真实宠物。

### 3.4 设置

设置按“日常调整”和“高级操作”分组。

#### 显示

- 显示宠物；
- 隐藏宠物；
- 尺寸：小 / 标准 / 大。

尺寸使用离散档位，不使用自由缩放滑杆，保证双平台语义一致。

#### 行为

- 活跃度：安静 / 正常 / 活泼；
- 浏览器预览减少动态：只在 Web 预览中提供，并默认跟随 `prefers-reduced-motion`。

文案：

- 安静：更少的闲时走动和自主动作；
- 正常：默认节奏；
- 活泼：更频繁地观察和走动；
- 预览减少动态：停止 Web 预览中的非必要连续动画，不改变桌面宠物设置。

#### 统计

只读展示：

- 陪伴时间；
- 被戳次数；
- 被摸次数。

首版不展示“最后互动时间”：当前存档只记录计数，不保存该时间戳。

统计不改变行为、不解锁功能、不上传网络。

#### 模拟事件

首版在设置页或实验室中提供：

```text
新的结果       [发送]
需要注意       [发送]
警告           [发送]
```

发送后真实宠物按现有优先级规则显示气泡。通知正文不持久化。

#### 高级：重置宠物

破坏性操作单独放在高级区域：

```text
重置宠物数据
这会删除当前宠物、名字、统计、位置和生成缓存，且无法恢复。
[重置宠物数据]
```

必须二次确认。重置成功后：

- 隐藏当前宠物；
- 删除个体、统计、位置和缓存；
- 原子写入未领养状态；
- Web 返回未领养概览；
- 不自动打开领养流程。

删除或写入失败时保留原数据。

## 4. 状态与连接体验

页面顶部或侧栏固定显示：

```text
本机 Gesso · 已连接
```

状态分为：

| 状态 | 页面行为 |
|---|---|
| connecting | 显示连接中，不允许写操作 |
| connected | 正常显示和操作 |
| processing | 当前动作按钮禁用，显示具体动作 |
| conflict | 快照已变化，丢弃本地过期输入并要求刷新 |
| error | 保留当前页面和输入，显示可操作的错误原因 |
| disconnected | 提示 Gesso 已退出或本地服务不可用 |

错误示例：

```text
保存失败，当前候选仍然保留。
原有宠物数据没有被覆盖。
[重试]
```

不要使用没有上下文的“发生错误”。

## 5. 预览资源与真实状态边界

Web 预览和原生窗口应使用同一份宠物资源契约：

```text
角色资源
├── manifest
├── idle
├── blink
├── gaze-left
├── gaze-right
├── walk
├── sleep
└── interaction frames
```

Web 侧可以用 Canvas 或图片帧播放；原生窗口继续使用平台渲染器。状态到帧的映射只能由 Rust/共享资源契约定义，不能由前端自行推断。

边界：

- Web 预览状态是页面本地状态；
- 桌面宠物状态来自 Rust 快照；
- 设置操作才会改变桌面真实状态；
- 模拟通知通过 `EngineAction::PetBehavior` 进入真实引擎；
- 预览不可伪造或写入互动统计。

## 6. 本地 Server 交互契约

第一版 API 以 JSON 为主，具体路由在实现阶段以 `crates/app/API.md` 为准。

### 6.1 读取

```text
GET /api/state
GET /api/pet/preview?state=idle|sleep|blink
GET /api/pet/adoption/preview?session_id=<session-id>&candidate_id=<temporary-id>
```

`/api/state` 返回页面所需的聚合快照，不暴露内部路径、seed 或存档原文。首版 DTO 明确包含：

```json
{
  "state_revision": 12,
  "adoption": {
    "session_id": "adopt-...",
    "adoption_revision": 4,
    "selected_candidate_id": null,
    "candidates": [
      {
        "candidate_id": "candidate-...",
        "name": { "zh": "小蜡烛", "en": "Candle" },
        "appearance": { "coat": "controlled-data" },
        "preview": "/api/pet/adoption/preview?session_id=adopt-...&candidate_id=candidate-..."
      }
    ]
  },
  "pet": {
    "name": { "zh": "小蜡烛", "en": "Candle" },
    "species": "candle",
    "behavior": "idle",
    "visible": true,
    "activity": "quiet",
    "size": "medium",
    "stats": {
      "companion_seconds": 123,
      "touch_count": 3,
      "poke_count": 5
    }
  }
}
```

实际 DTO 使用 `pet: null` 或完整宠物对象，使用 `adoption: null` 或完整领养会话对象；上面的非空结构只展示字段形状。`behavior` 返回一个实际枚举值：`idle|watching|walking|sleeping|poked|touched|dragging`。未领养时 `pet` 为 `null`；没有进行中的领养时 `adoption` 为 `null`。`session_id` 和 `candidate_id` 都是不可预测的 opaque ID，不把 `PetSeed` 编码进 URL。候选 DTO 只包含前端预览所需的 candidate ID、名称和受控外观数据，不返回 root seed、内部路径或资源路径；MVP 预览由服务端按 candidate ID 生成受控 PNG。首版不返回 `last_interaction_at`，因为现有存档未记录该字段。设置类接口使用 `state_revision`；领养类接口使用 `adoption_revision`。

### 6.2 写入

除只读的 `reset/prepare` 外，MVP 写操作采用异步 operation：HTTP 请求只负责校验并入队，返回 `202 Accepted` 和 `operation_id`；页面通过 operation 查询最终结果，不根据宠物状态变化猜测成功或失败。`reset/prepare` 只创建短期确认凭据，不修改状态，可同步返回 `200`。

```text
POST /api/pet/adoption/start      { "species": "candle", "request_id": "..." }
POST /api/pet/adoption/reroll     { "session_id": "...", "adoption_revision": 4, "request_id": "..." }
POST /api/pet/adoption/select     { "session_id": "...", "adoption_revision": 4, "candidate_id": "...", "request_id": "..." }
POST /api/pet/adoption/confirm    { "session_id": "...", "state_revision": 12, "adoption_revision": 4, "request_id": "..." }
PUT  /api/pet/visibility          { "visible": true|false, "state_revision": 12, "request_id": "..." }
PATCH /api/pet/settings           { "activity": "quiet|normal|lively", "size": "small|medium|large", "state_revision": 12, "request_id": "..." }
POST /api/pet/notice              { "kind": "new_result|needs_attention|warning", "request_id": "..." }
POST /api/pet/reset/prepare       { "state_revision": 12, "request_id": "..." }
POST /api/pet/reset/confirm       { "confirmation_id": "...", "state_revision": 12, "request_id": "..." }
GET  /api/operations/<operation-id>
```

如果当前已经存在领养会话，`adoption/start` 复用当前 session 和候选，不生成新候选并返回 `200 OK`；如果已经正式领养，返回 `409 Conflict`，不创建新会话；只有未领养且没有会话时才创建异步 operation。这样第二个浏览器标签页和 GPUI 会复用同一会话。`200 OK` 返回当前 adoption 快照；新建会话的 `202` operation 完成后，页面再读取 `/api/state` 获取候选。

请求格式或已知快照校验失败返回 `400` / `409`，不创建 operation；请求入队后如果主循环发现 revision 在排队期间已变化，operation 进入 `failed`，错误码为 `conflict`，页面刷新快照后重试。未知 operation 返回 `404`。

所有写入必须转换为现有 `EngineAction` 或其等价的引擎动作。由于现有动作队列没有请求回执，实现时新增 Web 专用命令信封（例如 `WebCommand { operation_id, request_id, expected_revision, action }`），由主循环在同一线程完成 revision 校验、执行动作和回报 operation；HTTP handler 只负责认证、参数校验和入队，不得直接持有或修改 `PetStore`、原生窗口管理器或存档文件。

API 使用两种 revision：全局 `state_revision` 用于已领养宠物的设置、可见性和重置；领养候选另有 `adoption_revision`，并始终和 `session_id` 一起校验。领养 `reroll` / `select` 只校验并递增 adoption revision；`confirm` 同时校验两种 revision，因为它会创建正式个体并改变全局宠物状态。模拟通知不要求 state revision，因为它不修改持久状态，只使用 request 去重。`PetSave.version` 是存档 schema 版本，不参与并发校验。revision 或 session 不匹配时返回 `409 Conflict`，不入队。

`PUT /api/pet/visibility` 使用目标状态而不是 toggle，保证浏览器重试和重复提交幂等。引擎侧应提供等价的 `SetPetVisibility(bool)` 动作，或在动作适配层发现目标状态已满足时不再发送 `TogglePetVisibility`。所有会改变状态的请求还应携带客户端 `request_id`；同一控制台 session、同一 endpoint、同一 `request_id` 在 5 分钟内重复提交时返回原 operation，不重复入队。服务端必须先查 request 去重表，再校验 revision；否则原请求成功后 revision 已变化，重试会错误返回 `409`。去重记录还要绑定请求体摘要；同一 request ID 搭配不同请求体返回 `409 request_reuse`，不能复用原 operation。

设置接口只接受列出的枚举字段；未提供字段保持不变；未知字段和非法枚举返回 `400`；visibility 和 reset 不通过 settings 接口修改。

### 6.3 异步 operation

统一返回：

```json
{ "operation_id": "op-...", "status": "pending" }
```

查询返回：

```json
{
  "operation_id": "op-...",
  "status": "pending|succeeded|failed",
  "error": null,
  "state_revision": 12,
  "adoption_revision": 4
}
```

`succeeded` 表示引擎动作已在主循环提交，且该动作要求的存档写入已经成功；仅内存变更或已标记 dirty 但尚未落盘不能报告 succeeded。若现有动作采用延迟节流写盘，Web 适配层必须等待对应 flush 结果，或使用专门的同步 Web 动作路径。成功或失败结果至少保留 5 分钟；读取不会立即删除结果。Gesso 重启后不恢复未完成的 operation。失败必须保留原有宠物数据和当前候选（若适用）。operation 只在当前 Gesso 进程和当前控制台会话下可查询；操作终态必须包含稳定错误码和用户可读 message，不能依赖日志文本。

MVP 使用短轮询：操作状态 250–500ms 查询一次，普通 `/api/state` 以 500–1000ms 刷新；页面在浏览器后台时应退到约 2–5 秒，避免每个标签页长期保持高频轮询；不在第一版引入 WebSocket 或 SSE。

## 7. 安全与生命周期

### 7.1 监听边界

只监听：

```text
127.0.0.1:<随机端口>
```

第一版禁止监听 `0.0.0.0`，不提供局域网访问开关。

### 7.2 访问令牌

- 每次 Gesso 启动使用操作系统安全随机源生成高熵 bootstrap code 和 session 标识；
- 打开浏览器的 URL 只携带一次性、短时有效的 bootstrap code fragment，例如 `/#code=...`，不直接放 bearer token；
- 页面加载后立即用 `POST /api/session/exchange` 交换 bootstrap code；由于 fragment 不会自动进入 HTTP 请求，前端只将 code 放入该次请求体，服务端消费 code，并通过仅当前浏览器会话有效的 `HttpOnly`、`SameSite=Strict` session cookie 建立控制台会话，再用 `history.replaceState` 清除 fragment；bootstrap code 只能使用一次并在 60 秒后失效；同一浏览器的多个标签页共享该 cookie；
- 除静态 HTML / CSS / JS 外，所有 `/api/*` 读写请求都必须带有效 session cookie；无 cookie 或错误 cookie 统一返回 `401`；
- session cookie 不写入日志、不写入配置、不持久化、不放入 URL；
- HTTP Server 不读取 fragment，因此 code 不出现在 HTTP 请求 URL 中；
- 对所有写请求和 `/api/session/exchange` 强制校验精确的 `Origin: http://127.0.0.1:<port>`；对 API 请求校验精确 `Host: 127.0.0.1:<port>`。不匹配直接返回 `403`；不能只依赖 SameSite cookie；API 不设置宽松的 `Access-Control-Allow-Origin`；
- Gesso 退出时 bootstrap code、session cookie 对应的服务端会话和 Server 一并失效。

### 7.3 数据保护

- 禁止任意路径读取；
- 资源只能通过资源 ID 映射到 Gesso 已知缓存；MVP 预览优先使用 `state` 参数返回受控 PNG，不先开放通用 asset manifest；
- 禁止执行命令、访问 shell 或代理任意 URL；
- API 返回数据不包含绝对路径、root seed、完整存档和内部调试秘密；
- 所有破坏性操作必须在引擎侧再次校验，不能只依赖前端按钮状态；
- 已领养宠物的持久状态操作携带当前 `state_revision`；领养 `reroll` / `select` 携带 `adoption_revision` 和 `session_id`，`confirm` 携带两种 revision。模拟通知只携带 request ID。revision 不匹配返回 `409 Conflict`，页面必须刷新快照后重试；`state_revision` 只在对应持久变更成功提交后递增，不因行为 tick、鼠标注视或气泡倒计时递增。操作结果的 `error` 使用稳定机器码（例如 `conflict`、`request_reuse`、`not_adopted`、`storage_failed`、`invalid_state`）和面向用户的 message，前端不解析日志文本。
- 不把 bootstrap code 或 session 标识放入资源 URL、日志或错误消息。

### 7.3.1 重置确认

重置使用两阶段确认，不能由单个 `POST /api/pet/reset` 直接触发：

```text
POST /api/pet/reset/prepare
→ { "confirmation_id": "...", "expires_in": 30, "state_revision": 12 }

POST /api/pet/reset/confirm
{ "confirmation_id": "...", "state_revision": 12, "request_id": "..." }
```

`confirmation_id` 短期有效、只能使用一次，并绑定生成它时的 `state_revision`。state revision 已变化时拒绝确认；第一次请求不修改数据。`reset/prepare` 和 `reset/confirm` 都必须在当前 session cookie 下执行，不能跨 Gesso 进程复用。

### 7.4 打开浏览器

新增托盘入口：

```text
在浏览器中管理宠物…
```

点击后：

1. 若服务未启动，启动 loopback Server；
2. 生成或获取当前进程的 bootstrap code；
3. 打开系统默认浏览器；
4. 若服务已启动，只激活或重新打开现有 URL；
5. Gesso 退出时关闭 Server。

## 8. 视觉方向

> **v0.3（2026-10-31 换世界）**：本节由 v0.2「伴宠机」整体替换为「**像素图鉴**」——用户改选方向 3「图鉴观测站」并钉死「Web 页面本体像素风」约束（决议记录见文档头部视觉基线 v0.3）。机器可读 token 记录在 `.impeccable/design.json`（开发辅助，不入产品事实源），权威实现是 `crates/app/web/src/styles.css`。

Web 控制台是一部**桌面生物图鉴的观测台**：纸墨档案 × 全页像素语法。两层语法严格分离——

- **图鉴层（纸面）**：米纸底 `#f3edde` 系 + 墨字 + **朱红 `#c4452b` 唯一强调色**；零圆角、2px 硬边框、硬像素投影（4px 4px 0，世界自带）；页签 = 图鉴文件夹标签（当前页与纸面连通）；观测角标（朱红 L 形四角）圈定观测台；条目编号 No.0X；背景 2px 棋盘抖动纹；字体 = Fusion Pixel 12px Proportional zh_hans（缝合像素字体，OFL 许可，TakWolf/fusion-pixel-font，woff2 内嵌 styles.css；仅 400 一档字重，层级靠字号 12/24/36 阶梯与颜色，禁 font-synthesis 伪粗）。
- **观测台层（LCD）**：深色像素液晶屏 `#1b1d16` 系（恒定不随主题），扫描线 + 像素地板 + 火光余晖；宠物 `image-rendering: pixelated` 的服务端受控 PNG 站在台面；屏内控件是 LCD UI 语法（`.lbtn` / `.lseg`），朱红提亮为 `#ff7a52`。**观测台永远只读，操作永远在图鉴层或明确标注的屏内控件上**——「预览不扰真」做进空间语法。
- **主题 = 灯下/夜巡**：OS 深浅主题只切换纸面（灯下米纸 / 夜巡深纸），观测台恒定深色。
- **色彩承诺（Committed）**：朱红 = 唯一强调色，只落在可动作、选中、当前页签、观测角标与行为语义上；连接状态绿只作 LED；危险操作同为朱红系（靠双线框与文案区分）；界面不做蓝色强调；火光余晖的暖橙属宠物本体的内容色，不是界面色。
- **动效**：全部步进化——待机沉降 4s steps(2)、火光余晖明灭 steps(2)、按下一律 0ms 瞬时（位移 3px + 投影消失）；`prefers-reduced-motion` 下全部静止（只影响 Web 预览，不改变桌面宠物）。
- **状态覆盖**：所有控件具备 hover / focus-visible（朱红虚线描边）/ disabled（去色降透明）/ 连接失败横幅 / 409 冲突横幅 / 选中 / 保存中态；文字对比度 ≥4.5:1（像素字体同样执行）。
- **签名元素**：**观测角标 + 图鉴页签**——纸面永远档案，屏内永远活体；该元素只属于 Web 控制台，不改变原生桌面窗口。
- **光标**：默认箭头与指向手均为手工点阵 PNG（data URI 内嵌 styles.css，16×20 / 14×20，热点对齐像素网格），调色与图鉴层同源（墨描边/纸白填充）；禁用态回退默认箭头；生成脚本与 10x 自检图存 `.impeccable/cursors/`。
- **不做**：圆角与平滑阴影（像素世界无柔影）、emoji/Unicode 代替图标（图标为内联像素 SVG）、任务/积分等 gamification 机制（治愈纪律）、把内部 seed/路径/资源 URL 暴露给浏览器。

## 9. MVP 验收标准

### 能力

- 从托盘打开系统浏览器并进入控制台；
- 未领养时可以完成角色选择、三候选、reroll、确认领养；
- 已领养时可以查看宠物、显示 / 隐藏、修改尺寸和活跃度；
- Web 预览遵循 `prefers-reduced-motion`，并可单独切换预览减少动态；该开关不修改桌面宠物设置；
- 可以查看统计；
- Web 预览只提供 `idle`、`sleep`、`blink` 三种 MVP 状态，并遵循 `prefers-reduced-motion`；
- 可以发送三类模拟通知；
- 可以执行二次确认的重置流程；
- 现有 GPUI、托盘和原生窗口路径仍然可用。

### 状态一致性

- Web 的领养结果与托盘 / GPUI 读取到的状态一致；
- Web 修改设置后原生宠物窗口及时反映；设置类 operation 只有对应的节流存档 flush 成功后才报告 succeeded；
- GPUI 或托盘修改后 Web 页面在下一次快照更新中反映；
- Web 关闭、刷新或断开不损坏候选和正式存档；
- 生成或写盘失败时原有数据不被覆盖。

### 安全

- Server 只绑定 loopback；
- 无有效 session cookie 请求不能访问任何 `/api/*` 数据或执行任何操作；bootstrap code 只能用于一次 `/api/session/exchange`；
- 不能通过 API 读取任意本地文件；
- bootstrap code、session cookie 对应的服务端会话不进入日志和持久化配置；
- Gesso 退出后端口和所有控制台会话失效。

## 10. 并发与实现顺序

### 10.1 领养会话

> **2026-10-30 落地差异**：本节为草案期并发模型，实际契约以《技术方案》§9.1 为准——请求携带 `request_id`（去重幂等）+ `adoption_revision`（乐观锁），无 `adoption_session_id` 字段（候选由服务端领养状态机持有）；「GPUI 再次进入开始领养」已随入口收敛移除，引擎侧仅存演练钩子通道。

引擎只保留一个 Web / GPUI 共享的领养会话。领养候选状态本身不写入 `pet.json`，但在当前进程内由引擎持有。

- 相同 session 且 `adoption_revision` 正确：执行操作并递增 `adoption_revision`；
- session 或 revision 过期：返回 `409 Conflict`，不改变候选；
- 第二个浏览器标签页或 GPUI 再次进入“开始领养”时复用当前 session 和候选，不生成新候选；
- GPUI 和 Web 同时操作时遵循同一规则，不允许静默覆盖；
- 页面刷新只重新读取当前会话，不从浏览器本地存储恢复候选；
- 只有当前领养会话关闭、确认成功或明确取消后，才允许创建下一轮 session。

### 10.2 实现顺序

1. 在本文件和产品规格中冻结 Web 能力边界；
2. 定义 Web 快照 DTO、`revision`、operation 和 EngineAction 映射；
3. 增加 loopback Server 生命周期、bootstrap code 和控制台 session；
4. 先实现认证后的 `/api/state` 与 `/api/pet` 只读页面；
5. 实现单会话领养 API 和页面；
6. 实现幂等 visibility、尺寸、活跃度和模拟通知；
7. 实现两阶段重置；
8. 实现 MVP 受控 PNG 预览；
9. 增加托盘“在浏览器中管理宠物…”入口；
10. 做 macOS / Windows 的流程回归与安全检查；
11. 正式精灵资源接入后，再扩展完整宠物实验室和 manifest。

不在第一轮引入 WebSocket、前端框架、局域网访问或嵌入式 WebView。