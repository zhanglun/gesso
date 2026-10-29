# Gesso 桌面宠物 Web 控制台技术方案 v0.1

> 对应能力规格：[桌面宠物 Web 控制台设计.md](桌面宠物%20Web%20控制台设计.md)。
> 本阶段先实现只读竖切：启动 loopback Server、打开系统浏览器、完成 session exchange、读取宠物快照和受控预览。

## 1. 第一阶段边界

第一阶段只实现：

- 托盘入口“在浏览器中管理宠物…”；
- Gesso 内启动一个只绑定 `127.0.0.1` 的 HTTP server；
- 系统浏览器打开带一次性 bootstrap code 的 URL；
- `POST /api/session/exchange` 换取 `HttpOnly; SameSite=Strict` 会话 cookie；
- 认证后的 `GET /api/state`；
- 认证后的 `GET /api/pet/preview?state=idle|sleep|blink`；
- Gesso 退出时关闭 server，所有会话失效；
- 静态 HTML/CSS/JS 内嵌在 Rust 二进制中，不引入前端构建链。

本阶段不实现领养写操作、设置写操作、operation、WebSocket、SSE、局域网访问和完整资源 manifest。

## 2. 线程与生命周期

```text
GPUI / 引擎主线程
  ├─ WebControl::open_browser()
  │   ├─ bind 127.0.0.1:0
  │   ├─ 生成 bootstrap code
  │   └─ 打开系统浏览器
  ├─ 每拍 publish(snapshot)
  └─ Quit → WebControl::shutdown()

Web server 线程
  ├─ 接收 TCP 请求
  ├─ 认证静态页面 / API
  ├─ 读取最近一次快照
  └─ 不直接访问 PetStore、GPUI 或原生窗口
```

`WebControl` 持有：

- `Arc<Mutex<WebPublishedState>>`：最近一次公开快照；
- `Arc<Mutex<AuthState>>`：bootstrap code、session cookie 和过期时间；
- `Arc<AtomicBool>`：停止标志；
- `JoinHandle`：server 线程；
- 当前监听地址。

server 线程使用非阻塞 listener，每次请求设置读取超时；`shutdown()` 设置停止标志并连接一次本地 listener 唤醒 accept，随后等待线程退出。server 线程不能阻塞 Gesso 退出。

## 3. HTTP 最小实现

第一阶段使用 Rust 标准库 `TcpListener`，只实现本项目需要的 HTTP 子集，不新增 HTTP 框架依赖：

- 仅支持 `GET` 和 `POST`；
- 请求头大小上限 16 KiB；
- body 大小上限 64 KiB；
- 只接受 `Content-Length`，拒绝 chunked body；
- 请求行、Host、Origin、Cookie、Authorization 等按有限长度解析；
- 未知路由返回 404；
- 未知 method 返回 405；
- API 统一 JSON 响应；
- 静态页面统一 UTF-8 响应。

如果后续需要 SSE、文件流或更复杂的 HTTP 行为，再单独评估依赖；第一阶段不为未来需求提前引入框架。

## 4. 认证流程

1. 托盘点击入口；
2. server 绑定随机端口；
3. 用 OS 安全随机源生成 32 字节 bootstrap code；
4. 浏览器打开：`http://127.0.0.1:<port>/#code=<base64url>`；
5. 前端读取 fragment，把 code 放入 `POST /api/session/exchange` JSON body；
6. server 一次性消费 code，生成 32 字节 session ID；
7. 返回 `Set-Cookie: gesso_web_session=<id>; HttpOnly; SameSite=Strict; Path=/`；
8. 前端用 `history.replaceState` 清除 fragment；
9. 后续 API 只接受该 cookie。

bootstrap code 只在内存中保存，60 秒过期、只能使用一次。session 只在当前进程内保存，Gesso 退出时全部失效。

所有 API 请求要求：

- `Host` 精确匹配当前 `127.0.0.1:<port>`；
- `/api/session/exchange` 的 `Origin` 精确匹配当前 origin；
- 已认证 API 请求带有效 cookie；
- 不返回宽松 CORS 头。

## 5. 公开快照

Web server 只读取已投影的公开 DTO，不读取 `PetSave` 原文：

```rust
WebPublishedState {
    state_revision: u64,
    pet: Option<WebPet>,
}
```

`WebPet` 只包含：

- 名称；
- 角色；
- 行为状态；
- 可见性；
- 活跃度；
- 尺寸；
- 陪伴秒数；
- 戳 / 摸次数。

不返回 root seed、绝对路径、存档版本、缓存目录或内部错误详情。

第一阶段行为状态来自 `PetBehavior::state()`。系统自动隐藏（全屏 / 锁屏）不覆盖用户保存的 `visible` 偏好；公开快照可增加 `temporarily_hidden`，若页面需要区分两者。

## 6. 预览

第一阶段预览接口只接受白名单状态：

```text
GET /api/pet/preview?state=idle
GET /api/pet/preview?state=sleep
GET /api/pet/preview?state=blink
```

实现优先复用现有宠物模板和 Rust 软件合成路径，输出固定 PNG。状态值不进入文件路径，不接受路径参数，不接受任意尺寸；尺寸由服务端限制在固定档位。

如果现有平台渲染器暂时没有无窗口 PNG 生成函数，第一阶段先让预览返回明确的 `501 preview_unavailable`，不要从磁盘读取任意缓存或复制平台窗口逻辑。

## 7. 主循环接入

第一阶段只需要一个动作：

```rust
OpenWebControl
```

动作由托盘 handler 入队。主循环执行时调用 `AppState.web.open_browser()`，并在每拍发布公开快照。退出动作执行 `web.shutdown()` 后再拆壁纸和退出。

Server 不能通过 `engine::enqueue` 伪造读操作；第一阶段没有 Web 写操作。后续写操作统一扩展为带 `operation_id` 的 `WebCommand`，由主循环在同一线程做 revision 校验、执行和回执。

## 8. 测试

纯单测：

- bootstrap code 一次性消费；
- 过期 code 拒绝；
- 错误 cookie 拒绝；
- Host / Origin 不匹配拒绝；
- API body、header 和请求行上限；
- 未知路径 / method 返回正确状态；
- 快照 DTO 不含内部字段；
- 预览 state 白名单。

进程级验收：

1. 启动 Gesso；
2. 点击托盘入口；
3. 浏览器自动打开；
4. `/api/session/exchange` 成功；
5. `/api/state` 返回当前宠物或 `pet: null`；
6. 刷新页面仍能读状态；
7. 无 cookie 的 API 返回 401；
8. 退出 Gesso 后连接关闭。

## 9. 后续写操作接入纪律

实现领养和设置前必须补齐：

- `WebCommand` 信封；
- `OperationStore`；
- `state_revision` / `adoption_revision`；
- `request_id` 去重和请求体摘要；
- 领养 session 复用；
- SetPetVisibility(bool) 等幂等动作；
- 两阶段 reset confirmation；
- 延迟存档 flush 与 operation 成功状态的绑定。

在这些机制完成前，不开放任何 Web POST 写操作。

### 9.1 已落地的领养写通道（2026-10-28）

领养五动作（start / reroll / select / confirm / back）已按上述纪律接入，统一走单端点：

```text
POST /api/adoption/op
  body: {request_id, revision, op, index?}
  403 invalid_origin（写操作强制 Origin 校验）
  400 missing_request_id / missing_revision / unknown_op
  409 stale_revision（乐观锁：revision ≠ 当前 adoption_revision）
  200 {operation_id, duplicate:true}（同 request_id 重放幂等）
  202 {operation_id}

GET /api/operations/{operation_id} → {status: pending|applied|unknown}
```

关键设计：

- **乐观锁用 `adoption_revision` 而非 `state_revision`**：state_revision 随行为状态（idle/blink）频繁跳动，会让写请求永远过期；adoption_revision 只在领养状态/候选变化时递增，才是领养操作的真正并发实体。
- **执行在引擎线程**：操作入队后在主循环 drain 为既有 EngineAction（StartPetAdoption / PetAdoption(AdoptionEvent::…)），与 GPUI 领养窗口共用同一状态机和持久化事务；Web 线程绝不直接触碰 AdoptionState / PetStore。
- **状态机天然幂等兑底**：非法转移（如未选候选就 confirm、重复 start）在 `adoption_transition` 中是无害 no-op；request_id 去重另外解决 reroll 这类非幂等操作的重放。
- **设置类写操作（2026-10-28）**：`pet.set_visible`（payload `{visible:bool}`）→ 幂等动作 `EngineAction::SetPetVisibility(bool)`（同值零副作用，不产生 revision 跳动）；托盘 `TogglePetVisibility` 也改为基于它实现，消除读改写 TOCTOU。设置页显示状态、概览显隐按钮均已接入。
- **候选预览（2026-10-28）**：`GET /api/pet/preview?state=idle&candidate=<candidate_id>` 从已发布快照的公开候选 DTO 查找外观并渲染（candidate_id 本就公开，无新增泄露面）；领养页三候选卡即见即所得。
- 尚未接入：尺寸/活跃度档位写、两阶段 reset、request body 摘要（当前 body 极小，暂无必要）。

### 9.2 实机验收记录（2026-10-28）

沙箱 HOME 全链路验收（不碰真实存档）：

- HTTP 驱动：领养五动作 + 显隐切换 + 幂等重放 + 409/403/400 负向路径，全部通过；
- 真实浏览器 UI：exchange → 领养动线（三候选真实 PNG）→ 确认 → 概览/实验室/设置，全部通过；
- 退出后端口关闭、session 失效；重启后领养持久化。

验收中发现并修复：**浏览器 preconnect 竞态**——Chrome 在发请求前预热 TCP 连接，空闲连接撞上服务器 2s 读超时会收到 400，且被同连接上的后续真实请求读到并渲染为文档。修复：未收到任何字节的连接超时后静默关闭，不回错误响应。另增 debug 钩子 `GESSO_PET_WEB_ECHO=1`（回显引导 URL）与 `GESSO_PET_WEB_NO_OPEN=1`（跳过系统浏览器，避免真实浏览器抢先消费一次性 code）。
