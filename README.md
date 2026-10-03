# Gesso

跨平台（Windows / macOS）动态壁纸引擎。Rust · GPUI · wry。

> 设计与工程基线见 [`../gesso-design/`](../gesso-design/)：技术方案（14 章）、界面与交互规格 v1.0（冻结）、DESIGN.md、可交互原型。

## 结构

```
crates/
├── core/   # 纯领域逻辑：ContentSpec / 配置 / 内容库 / 会话状态机（零平台依赖，全量单测）
└── app/    # GPUI 应用：管理窗口、壁纸窗口壳、贴壁层、事件桥、协议层
```

## 前置要求

- macOS：Command Line Tools 即可（gpui-kit 走 `runtime_shaders`：Metal 着色器运行时经驱动编译，构建期不调 `metal` 工具，无需完整 Xcode）
- Windows：VS Build Tools（标准 Rust MSVC 工具链）

## 命令

```bash
cargo test -p gesso-core    # 秒级全量单测
cargo run -p gesso-app      # 管理窗口空壳（M0）
cargo clippy -p gesso-core --all-targets -- -D warnings
```

## 里程碑（节选，全文见技术方案 §12）

- [x] M0 骨架：workspace + 三页签空壳 + CI
- [x] **M0.5 GPUI 集成 spike（判定点0）** ✅ 四连验全过（无 Xcode，runtime_shaders）
- [x] M2 视频壁纸端到端 ✅（钉桌面 + 暂停/恢复 + 配置持久化验证）
- [ ] M3 会话/配置/UI（引擎侧完成，UI 细节见 HANDOFF.md）
- [ ] M1 Windows 贴壁（判定点1，需 Windows 机器）
- [x] M1.5 macOS 压层 spike（全过：纯 AppKit 壁纸窗口架构定型，level=-2147483604）
