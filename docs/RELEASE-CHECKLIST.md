# Release Checklist

> 当前发布流程。已完成版本的命令输出、旧 tag 和历史重发记录见 `docs/archive/releases/`。

## 发布前

- [ ] 确认 `git status --short` 中只有本次发布内容。
- [ ] 更新 workspace 版本号和对应 release notes。
- [ ] 更新 `CHANGELOG.md`：将 Unreleased 冻结到目标版本。
- [ ] 更新 README 和其他用户文案中引用的版本号。
- [ ] 运行质量门：
  ```bash
  cargo fmt --all --check
  cargo clippy -p gesso-core -p gesso-app --all-targets -- -D warnings
  cargo build -p gesso-app --all-targets
  cargo test -p gesso-core -p gesso-app
  git diff --check
  ```
- [ ] 完成对应平台实机冒烟；未完成的硬件验证写入 Roadmap，不隐瞒。

## 本地冒烟

- [ ] 双击和随机换壁纸不冻结。
- [ ] 分辨率变化、唤醒、合盖/开盖或外接屏插拔至少验证可用场景。
- [ ] （macOS）重配期间过渡窗显示静帧，真实壁纸窗稳定后过渡窗消失。
- [ ] （Windows）视频壁纸正常播放；托盘退出后桌面壁纸完整恢复；反复启动退出无新孤儿 WorkerW 堆积。
- [ ] 右键移除、“恢复系统壁纸”和退出只拆除 Gesso 窗口，不等待系统桌面换图。
- [ ] 设置页没有“桌面桥接”开关。
- [ ] 启动时旧版本桥接残留清理路径有日志且不会阻塞正常启动。

## GitHub Actions 发布

推送 `v*` tag 后，`.github/workflows/release.yml` 会：

1. 在 macOS 构建 `Gesso-<version>-arm64.dmg`；
2. 在 Windows 构建 portable zip 和 NSIS setup.exe；
3. 生成 `checksums.txt`；
4. 生成 `latest.json` 静态更新清单（版本、发布说明、各平台安装包名称/大小/下载地址）；
5. 创建 GitHub Release 并读取 `packaging/release-notes/<tag>.md`，同时上传安装包、校验文件和 `latest.json`。

应用日常检查优先读取 `https://github.com/zhanglun/gesso/releases/latest/download/latest.json`，不访问限流的 GitHub REST API；旧 Release 没有该资产时自动回退 API。

产物不签名、不公证，release notes 必须说明首次打开方式。

```bash
git tag -a vX.Y.Z -m "Release X.Y.Z"
git push origin vX.Y.Z
```

覆盖已有 Release 时，先明确确认无人依赖旧资产，再删除旧 Release 和远程 tag，重建 tag 后检查：

```bash
gh release view vX.Y.Z --json assets,tagName
```

## 当前版本记录

- `v0.1.2`：已发布。最终覆盖版本指向 `ace16c2`，包含过渡窗终局和同步后的 release notes。
- Windows 多显示器真实硬件验证仍是已知缺口，不阻塞当前 macOS/Windows 单屏产物发布，但必须保留在 Roadmap。

## 归档

- `docs/archive/releases/0.1.2-history.md`：0.1.2 首发、更新器修复和覆盖重发流水。
