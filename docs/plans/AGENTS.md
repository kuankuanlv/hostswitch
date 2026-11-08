# hostswitch 计划管理（AI 侧）

> 变更流程唯一事实源：`~/.agents/PLAN_SOP_AGENTS.md`；本文件只放本地参数与进度总表。

## 草案与状态行

- 一个变更一个文件，全程在 `docs/plans/` 平铺，不移动目录。
- 文件头状态行标阶段：①~⑤ 想法/草案（未承诺执行）、⑥ 执行中、⑦ 已归档。
- 命名 `YYYYMMDD-NN-slug.md`（NN=当日序号，01 起按创建先后）；全程不改名。
- 落新草案前先扫 plans 同类条目合并统筹；否决/过时标「已否决/已过时 + 原因 + 日期」原地留档、不删除。
- 流程唯一事实源：`~/.agents/PLAN_SOP_AGENTS.md`。

## 本地声明

- plans / archive 路径：`docs/plans`、`docs/plans/archive`
- 全量测试命令：`cargo check --all-targets && cargo test`（本机沙箱只允许写工作区，cargo 缓存需指向工作区外临时目录：`CARGO_HOME=/tmp/hostswitch-cargo-home`，`target/` 留在仓库内且已被 .gitignore 忽略）
- 测试隔离要求：单测不得写真实 `/etc/hosts`、不得写真实 `~/.hostswitch`；平台写盘单测只允许用 FakePlatform，egui 单测临时改 HOME 后还原
- archive 归档清单：见 `archive/README.md`（已完成计划 mv 列表）
- 项目硬约束指针：`~/.agents/CODING_AGENTS.md`、`~/.agents/RUST_AGENTS.md`；重构兼容策略＝先行为不变地重构、后做功能改动，同一批不混行为变化

## 进度总表

| 名称 | 阶段 | 触及域 | 说明 |
|---|---|---|---|
| `20261108-01-rust-compliance-refactor` | ⑥ 执行中 | `src/service`、`src/platform`、`src/ui`、`src/storage`、`docs/plans`、`README.md`、`.gitignore` | 全量 Rust 规范重构 + 系统 hosts 写入冲突检测（乐观锁） |

（只登记 ⑤⑥⑦；完成即归档，滚动保留：全部未完成 + 最近 5 条 ⑦）

## 并行协商区（无协商即空）

| 提案方 | 受理方 | 争议文件 | 提案 | 回应 | 结论 |
|---|---|---|---|---|---|
| — | — | — | — | — | — |
