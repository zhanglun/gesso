# 设计文档目录

## 当前事实源

- `界面与交互设计.md`：当前交互规格。
- `DESIGN.md`：当前视觉 token 与组件约束。
- `prototype/index.html`：当前可点击原型。
- `specs/*.json`：架构图、流程图和数据流图的源文件。
- `*.architecture.html` / `*.workflow.html` / `*.lifecycle.html` / `*.dataflow.html`：由 `specs/*.json` 生成的辅助图表，不是改代码前的必读文档。

早期产品定位和设计决策已归档到 `docs/archive/design-drafts/`，不作为当前实现事实源。当前 UI 变更以交互规格、视觉 token 和原型为准。

## 归档与生成物

- `docs/archive/design-visual-check/`：本地视觉检查截图与检查页，不是事实源。
- `docs/archive/design-drafts/`：未采用的图标和设计草稿。
- 修改图表时先改 `specs/*.json`，再用 Archify 重新生成 HTML；不要直接编辑生成的 HTML。
