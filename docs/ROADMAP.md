# Roadmap

> 本文只记录当前未完成事项、验证缺口和下一步优先级。已完成能力见 [CHANGELOG.md](../CHANGELOG.md)。

## 当前状态

- ✅ macOS 壁纸窗口：AppKit 原生窗口钉在桌面图标层之下，支持全空间、点击穿透和多屏独立指派。
- ✅ Windows 壁纸窗口：WorkerW 挂载、Explorer 重启自愈、PMv2 DPI 和原生视频呈现已落地。
- ✅ 内容类型：视频、静态/动图图片、Shader、HTML，以及用户主动导入的 WE `video` / `web` 项目。
- ✅ 运行时：事件驱动显示器重配收敛、全屏/电源/锁屏/空闲策略、光标/时间桥、填充模式和应用内更新。
- ✅ 桌面过渡：运行时不写系统桌面；几何过时窗口由 Gesso 自己的 AppKit 静帧过渡窗覆盖；旧版系统桌面桥接帧只在启动时清理。
- ✅ 发布：`v0.1.3` 已通过 GitHub Actions 生成 macOS DMG、Windows 安装包、便携包和校验文件；更新器本地化修复与 Windows 代理修复在后续维护提交中继续验证。
- ✅ 桌面宠物（feat/desktop-pet 分支，待合入）：macOS AppKit 宠物窗口 + 领养/行为状态机 + Web 控制台（loopback Server + Svelte 前端）；2026-10-30 入口收敛后领养/设置/重置/模拟事件唯一管理面 = Web 控制台（GPUI 领养小窗已撤销）；双入口并发演练已自动化（`GESSO_PET_GPUI_DRILL` + `tools/pet-verify/web-drill.py`）。

## 下一步优先级

1. **Windows 多显示器真实硬件回归**
   - 插拔、分辨率变化、主屏切换、Explorer 重启；
   - 重点检查 WorkerW 重挂、窗口几何、点击穿透和配置承接；
   - 多机型冒烟（NVIDIA / Intel 核显 / 精简系统）覆盖视频渲染与退出后壁纸恢复——渲染三要素（拓扑 / 顶点缓冲 / 常量绑定）与 DWM 残尸行为随驱动组合变化。
2. **安装和升级冒烟**
   - 干净 macOS 机器安装未签名 DMG；
   - 干净 Windows 机器安装 NSIS 和便携包；
   - 验证应用内更新、系统代理和失败重试。
3. **Wallpaper Engine import II**
   - `scene` 类型解析和安全边界评估；
   - 继续保持用户主动导入，不扫描 Steam/工坊，不下载或再分发素材。
4. **HTML 壁纸兼容性**
   - 收集真实项目样本；
   - 保持 sandbox、CSP、无 IPC 和同源资源边界；
   - 评估 `fps_cap` 对第三方 RAF 的可控范围。
5. **桌面宠物收尾（feat/desktop-pet）**
   - ✅ 人工实机确认（2026-10-31，dev 实例）：模拟事件三类气泡正常弹出、帧序修复后眨眼/注视仅动眼睛且火苗亮度不变（f2deaf7）；Web 控制台视觉重设计 v0.3「像素图鉴 · 观测台」+ 前端栈 Preact/HTM 免构建落地（含设置档位乐观更新，CI 全绿）；
   - 日常使用体感反馈持续收集中（状态机层已由演练覆盖；2026-10-30 入口收敛后 GPUI 领养小窗已撤销，管理面唯一化为 Web 控制台）；
   - Windows press 悬挂挂账修复（设计稿 §10 P2 待完成②，同款已修见①）；
   - 宠物正式美术与首发角色设计定稿（物种选型已重启，进行中见《缝隙居民·物种族群设计稿.md》）；**物种形态拍板前不合 main**（用户约束 2026-10-31）。
6. **通知能力（桌面宠物 P3 立项中，2026-10-25）**：**线 A 出站**——把散在 `update.rs` 的系统通知（macOS osascript / Windows WinRT toast，均已 best-effort + 兜底）收敛为可复用 notify 模块，附一次精简系统 toast 可达性实测；**线 B 入站**——本地事件接口选型（来源认证先于线格式定稿，首版外部只传事件类型不传自由文本），首个生产者接 Gesso 内部氛围类事件 → 宠物气泡。分流/去重规则见《界面与交互设计.md》§3.1；明确不做系统级通知拦截/替代。

## 明确不做

- Linux 后端；
- WE `application` 原生程序；
- Steam/Workshop 磁盘扫描；
- 内容下载器或再分发；
- 运行时写入系统桌面壁纸；
- 静默替换正在运行的应用。

## 进行中交接（新 AI 会话提示词）

桌面宠物开发（feat/desktop-pet）开工提示词，复制即用：

> 继续 gesso 桌面宠物开发。进度和契约已写入仓库，先按顺序读：
> 1. docs/ROADMAP.md——本文「当前状态」桌面宠物行与「下一步优先级」第 5 项；
> 2. docs/design/桌面宠物设计.md v1.1（2026-10-30 入口收敛修订 + §10 分期挂账）；
> 3. docs/design/桌面宠物 Web 控制台技术方案.md §9（写通道契约 + §9.3 演练）；
> 4. 实现：crates/app/src/web_control.rs（Server+API）+ crates/app/web/（Preact/HTM 免构建静态前端 + vendor/ 运行时，改后必须 cargo build——产物编译期内嵌、无 npm）+ crates/app/src/pet_window.rs（桌面宠物窗）。
>
> 当前状态（2026-10-31）：领养/设置/重置/模拟事件唯一管理面 = Web 控制台（GPUI 领养小窗已撤销）；控制台视觉 = v0.3「像素图鉴 · 观测台」（Fusion Pixel 中文像素字体内嵌、Preact+HTM 免构建，视觉基线见控制台设计稿头部）；实机人工确认已过（模拟事件气泡、帧序观感）；CI 全绿。
> 待人工：日常使用体感持续收集。
> 待办：**物种形态拍板（拍板前不合 main，用户约束 2026-10-31）** → 合回 main → 0.2.0 发版（docs/RELEASE-CHECKLIST.md）；Windows press 悬挂挂账（设计稿 §10 P2 待完成②）。
> 遵守 AGENTS.md 硬性规则（原子提交、提交前 cargo check + cargo test 全绿、改界面先改设计文档）。

## 相关文档

- 当前架构：[ARCHITECTURE.md](ARCHITECTURE.md)
- 开发与调试：[DEVELOPMENT.md](DEVELOPMENT.md)
- 工程纪律：[ENGINEERING-NOTES.md](ENGINEERING-NOTES.md)
- WE 兼容边界：[WALLPAPER-ENGINE.md](WALLPAPER-ENGINE.md)
- 发布流程：[RELEASE-CHECKLIST.md](RELEASE-CHECKLIST.md)
