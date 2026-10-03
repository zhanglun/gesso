<!-- 感谢贡献！先过一遍清单（详见 CONTRIBUTING.md） -->

## 改了什么 / 为什么

<!-- 一两句话说清行为变化与动机；非显而易见的设计取舍写在这里 -->

## 类型

- [ ] 引擎（core / pin / session / protocol）
- [ ] UI（ui/，仅经 crates/app/API.md 的接口）
- [ ] 文档
- [ ] 构建 / CI
- [ ] 其他

## 自查清单

- [ ] `cargo check -p gesso-app` 通过
- [ ] `cargo test -p gesso-core -p gesso-app` 通过（新逻辑带了测试）
- [ ] `cargo clippy -p gesso-core --all-targets -- -D warnings` 通过
- [ ] UI 改动没有越过 API.md 的接口边界
- [ ] 改变了界面行为的话：已在描述中说明新契约（对照冻结交互规格）
- [ ] 实机跑过改动的路径

## 如果改了界面行为（维护者核对）

<!-- 新契约是什么；规格文档是否需要同步更新 -->
