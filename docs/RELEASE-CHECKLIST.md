# 0.1.2 Release Checklist

Gesso 0.1.2 修复版本的发布清单。按顺序执行；流程与 0.1.1 相同（GitHub Actions
自动打包发布），这里只跟踪本版本的状态。

> 发布由 **GitHub Actions 自动完成**：推送 `v*` tag → `release.yml` 在两平台
> matrix 跑打包脚本 → 汇总自动创建 GitHub Release，附产物 + `checksums.txt` + release notes。
>
> 目标产物：
> - **macOS**：`Gesso-<version>-arm64.dmg`
> - **Windows**：`Gesso-<version>-x64-portable.zip` + `Gesso-<version>-x64-setup.exe`（NSIS 安装包）
> - 一个 GitHub Release（`.dmg` + `.zip` + `.exe` + `checksums.txt` + notes）
>
> **不做代码签名 / 公证**：两平台产物均为未签名，release notes 写清首次打开方式。

## 阶段 0 — 发布决策

- [x] 沿用 0.1.1 的决策：macOS 仅 arm64、Windows 安装包 + 便携 zip、产物不签名不公证。

## 阶段 1 — 代码冻结与质量门

- [x] 合入 0.1.2 全部内容：填充模式、桌面桥接临时覆盖与台账自愈、随机换一张修复、
      显示器页打磨（v0.1.1..HEAD）。
- [x] `cargo fmt --all --check` / clippy（core+app，`-D warnings`）/ build / test 全绿
      （fmt 与 Windows clippy 红灯已修：`ed547f3`、`d0b4a37`；CI run 38348937763 双平台绿）。
- [x] 发布收口提交：`chore(release): 0.1.2`（版本号 + CHANGELOG 冻结 + notes + README；本提交）。
- [x] 收口提交推送后 CI 复验全绿。

## 阶段 2 — 实机冒烟

- [x] 填充模式三态（macOS AVLayerVideoGravity / Windows quad 管线）实机验证（2026-10-13 用户确认）。
- [x] 桌面桥接临时覆盖：指派/切换/退出/「恢复系统壁纸」全路径实机验证（同上）。
- [ ] Windows 多显示器重配实机验证（沿袭 0.1.1 已知限制，不阻塞本版本）。

## 阶段 3 — 版本号与变更记录

- [x] `Cargo.toml` workspace version → `0.1.2`（NSIS/打包脚本注入，无散落硬编码）。
- [x] `CHANGELOG.md`：`[Unreleased]` → `[0.1.2] - 2026-10-13`，重建空 Unreleased。
- [x] release notes：`packaging/release-notes/v0.1.2.md`（workflow 按 tag 名读取）。
- [x] README 双语状态行 → v0.1.2 发布候选。

## 阶段 6 — 触发 CI 发布

- [x] **干跑**：手动触发 Release workflow（`workflow_dispatch`，只打包不发布），确认三产物齐全
      （[run 38351122277](https://github.com/zhanglun/gesso/actions/runs/38351122277)，2026-10-13：dmg / setup.exe / portable.zip 全部出齐）。
- [x] 打 tag 并推送（2026-10-13，`v0.1.2` → `9cebdaf`）：
  ```bash
  git tag -a v0.1.2 -m "Release 0.1.2"
  git push origin v0.1.2
  ```
- [x] CI 自动创建 Release 并上传产物；Actions 页确认三 job 全绿；确认正式发布
      （[run 38351683395](https://github.com/zhanglun/gesso/actions/runs/38351683395)；
      https://github.com/zhanglun/gesso/releases/tag/v0.1.2，2026-10-13T05:32Z）。

## 阶段 7 — 发布后

- [ ] 从 Release 页面下载产物，干净机器各装一遍。
- [ ] 公告渠道。
- [ ] 0.2 规划（0.1.x 反馈整理进 ROADMAP）。

---

## 0.1.1 发布记录（2026-10-12，已完成）

- tag `v0.1.1` → [release run 38196885084](https://github.com/zhanglun/gesso/actions/runs/38196885084) 三 job 全绿 →
  [Release v0.1.1](https://github.com/zhanglun/gesso/releases/tag/v0.1.1) 附 `.dmg` / `.zip` / `.exe` / `checksums.txt`。
- 干跑记录：[run 38195110710](https://github.com/zhanglun/gesso/actions/runs/38195110710)。
- 遗留：干净机安装验证、公告、0.2 规划（并入上方阶段 7）。

> 原则：0.1.x 先保证"两平台能下载、安装、跑起来"；不做签名/公证。未签名产物
> 在 release notes 写清打开方式即可。
