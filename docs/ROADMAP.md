# Roadmap

> 本文只记录当前未完成事项、验证缺口和下一步优先级。已完成能力见 [CHANGELOG.md](../CHANGELOG.md)。

## 当前状态

- ✅ macOS 壁纸窗口：AppKit 原生窗口钉在桌面图标层之下，支持全空间、点击穿透和多屏独立指派。
- ✅ Windows 壁纸窗口：WorkerW 挂载、Explorer 重启自愈、PMv2 DPI 和原生视频呈现已落地。
- ✅ 内容类型：视频、静态/动图图片、Shader、HTML，以及用户主动导入的 WE `video` / `web` 项目。
- ✅ 运行时：事件驱动显示器重配收敛、全屏/电源/锁屏/空闲策略、光标/时间桥、填充模式和应用内更新。
- ✅ 桌面过渡：运行时不写系统桌面；几何过时窗口由 Gesso 自己的 AppKit 静帧过渡窗覆盖；旧版系统桌面桥接帧只在启动时清理。
- ✅ 发布：`v0.1.3` 已通过 GitHub Actions 生成 macOS DMG、Windows 安装包、便携包和校验文件；更新器本地化修复与 Windows 代理修复在后续维护提交中继续验证。

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

## 明确不做

- Linux 后端；
- WE `application` 原生程序；
- Steam/Workshop 磁盘扫描；
- 内容下载器或再分发；
- 运行时写入系统桌面壁纸；
- 静默替换正在运行的应用。

## 相关文档

- 当前架构：[ARCHITECTURE.md](ARCHITECTURE.md)
- 开发与调试：[DEVELOPMENT.md](DEVELOPMENT.md)
- 工程纪律：[ENGINEERING-NOTES.md](ENGINEERING-NOTES.md)
- WE 兼容边界：[WALLPAPER-ENGINE.md](WALLPAPER-ENGINE.md)
- 发布流程：[RELEASE-CHECKLIST.md](RELEASE-CHECKLIST.md)
