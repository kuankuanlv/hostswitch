# hostswitch：全量 Rust 规范重构 + 系统 hosts 写入冲突检测（乐观锁）

- **状态：⑥ 执行中**（2026-11-08 用户批准执行）
- **日期**：2026-11-08
- **来源**：用户 2026-11-08 对话（同意全量重构并一并实现“乐观锁”，要求走 SOP 先出计划）
- **决策**：14 项待讨论已全部收敛（见 §4.1），用户 2026-11-08 确认
- **归档去向**：`docs/plans/archive/`（⑦ 时 `git mv`，文件名与内容不变）

## 1. 背景与意图

对 hostswitch 做一次彻底重构，核心遵循本地 Rust 规范：`~/.agents/CODING_AGENTS.md`、`~/.agents/RUST_AGENTS.md`；同时把前一轮讨论的“乐观锁”（写系统 hosts 前检测外部程序修改）一并实现。流程走 `~/.agents/PLAN_SOP_AGENTS.md`：先出计划、打磨定稿后再实施。

**前序已完成（当前工作区未提交，4 个 src 文件 M）**：

- 修复“保存激活环境不会更新 /etc/hosts”的缺陷：改为单一收敛（desired 与系统 hosts 比较，不同才写）。
- 写盘在途时不再丢编辑（攒同步、写完补一次）；启动发现漂移只提示 + Apply 手动同步。
- 本次改动范围内已做风格整理：`app.rs` 成员排列（pub 前、私有后）、`poll_pending` 拆分为 3 个函数、`toggle_env` 拆 enable/disable、`env_manager` 单测改表驱动、`build_merged` 收为私有。

**本次范围**：

1. 全仓库存量违规一次性修复：超长函数拆分、成员排列、可见性、嵌套、重复实现、注释/格式化。
2. 乐观锁：写盘前检测系统 hosts 被外部修改，冲突不静默覆盖，需用户点 Overwrite 确认。

## 2. 合并统筹

- 扫 `docs/plans/`：本次刚初始化（`AGENTS.md`、`archive/README.md` 已建），无其它条目可合并。
- `docs/superpowers/plans/2026-08-03-windows-adaptation.md`：历史过程稿（Windows 适配已完成，未被 git 跟踪）。按 SOP §1 属过程稿，不登记、不维护；本计划平台层重构会触及它涉及的 `src/platform/windows.rs`，执行时要求命名管道协议行为字节级不变。
- 结论：新建本计划文件，无合并对象。

## 3. 核对结论表

| # | 疑问 | 代码事实（2026-11-08 核对） | 收敛去向 |
|---|---|---|---|
| 1 | 存量违规到底有哪些 | 16 处函数 >40 行（含 1 处测试用例表）；`EnvManager` 成员排列 1 处；嵌套 >3 层 2 处（`run_helper`=4、`draw_frame`=5）；3 个无调用方 API；仓库非 rustfmt-clean；4 个文件 12 处老注释含反引号/箭头 | §7 清单；拆到 P1 三批 |
| 2 | 是否已有 plans 体系 | 无 `docs/plans`；`docs/` 被 `.gitignore` 整目录忽略 | 已建骨架；是否纳入 git 见待讨论 #1 |
| 3 | 乐观锁现状 | 无基线/冲突检测；写系统 hosts 是整文件覆盖；`sync_plan` 每次已读 current，可低成本接入 | §5 设计，P2 实现 |
| 4 | 未提交改动 | `git status`：`src/service/env_manager.rs`、`service/mod.rs`、`ui/app.rs`、`ui/impls/egui/mod.rs` 已改未提交 | 待讨论 #4 |
| 5 | Windows 验证能力 | 已实测通过：`cargo check --target x86_64-pc-windows-msvc`（2026-11-08，2m04s）；运行时验证需 Windows 机器 | P1-B 每步加交叉 check；真机由用户验 |
| 6 | 单测是否会写真实系统 | 现有单测只读 `/etc/hosts`；写盘类用 FakePlatform；egui 单测临时改 HOME 后还原 | 新任务沿用，禁止写真实 hosts/`~/.hostswitch` |
| 7 | 老注释能否规范化 | 12 处含反引号/箭头，属用户注释，规则禁止删除/改写 | 待讨论 #3（需明确授权） |
| 8 | 项目有无根 AGENTS.md | 无 | 无需加 SOP 指针 |

## 4. 已锁定决策

1. 同步模型＝收敛式：`desired = merge(local, active envs)` 与系统 hosts 比较，不同才写；不做事件枚举。
2. 行为基线（前几轮已定）：保存激活环境自动写盘（每进程首次弹一次授权）；保存未激活环境只存文件；启动发现漂移只提示 + Apply，不自动写；写盘失败时激活态回滚、保存内容不回滚。
3. 乐观锁语义：以“上次确认的系统 hosts 内容”为基线；提交前若当前内容 != 基线，判为外部修改，不自动覆盖；用户点 Overwrite 才写。检查与写入之间的竞态窗口接受（桌面工具，不引入 flock/原子替换）。
4. 重构顺序：先行为不变重构（P1），后功能改动（P2）；同一批不混行为变化。
5. 批次验证：每批 `cargo check --all-targets && cargo test` 全绿才进下一批；平台批追加 Windows 交叉 check。
6. blame 不重要，允许搬动老函数、重排成员。
7. 单测命名与结构按 RUST §1/§2（表驱动、`test_<对象>_<场景>_<断言>`）。

### 4.1 ③ 细化决策记录（14 项，用户 2026-11-08 确认）

| # | 决策点 | 结论 |
|---|---|---|
| 1 | `docs/plans` 版本管理 | A：`.gitignore` 改为忽略 `docs/superpowers/`、放行 `docs/plans/` |
| 2 | rustfmt 全量格式化 | A：P3 一次性 `cargo fmt`，之后 `cargo fmt --check` 纳入门槛 |
| 3 | 老注释规范化 | **A2**：格式规范化 + 改写本地约定/外部产品引用，逐条列 diff（不删信息、不改业务语义） |
| 4 | 现有未提交改动 | B：先单独提交这份修复（前置 commit），再按任务重构 |
| 5 | 三处无调用方 API | A：删除 `EnvManager::rename_env`、`ConfigStore::rename_env_file`、`HostsMerger::extract_env_content` 及对应单测 |
| 6 | README 行为说明 | A：同步更新 |
| 7 | 每任务 commit | A：授权按 SOP 每任务独立 commit（先读 `GIT_AGENTS.md`；push 禁止；只用明确路径 `git add`） |
| 8 | macOS CFString UB | A：本轮修，改 `CString` |
| 9 | Windows 白名单恒真 | A：本轮修（前缀 + 规范化 + 普通文件校验 + `win_protocol` 单测） |
| 10 | Windows SD 泄漏 | A：记录不修，写入计划备注 |
| 11 | Windows 锁错误文案 | A：去重时通过参数保留原文本 |
| 12 | 平台层整文件搬移 | A：确认全量搬移 |
| 13 | 硬编码配色改 token | A：P1 之后的独立任务；浅色观感由用户确认 |
| 14 | UI 函数重命名 | A：按计划改名 |

## 5. 乐观锁设计（P2）

### 5.1 EnvManager

- 新字段 `system_hosts_baseline: Option<String>`：本进程“上次确认的系统 hosts 内容”；`new()` 时读一次填入（读失败为 None，降级为不做冲突检测）。
- `sync_plan(&mut self)`：
  - 读 current、算 merged；
  - `merged == current` → 刷新 `baseline = current`，返回 `InSync`；
  - 否则校验激活环境后返回 `Write { merged, warning_count, system_changed }`，其中 `system_changed = baseline 存在且 baseline != current`。
- 新方法 `confirm_system_hosts(&mut self, content: &str)`：写盘成功后更新基线。
- `SyncPlan::Write` 增加 `system_changed: bool`。

### 5.2 HostswitchApp

- `PendingOp` 增加 `written: String`（写盘内容，成功后回填基线）。
- 新增 `pub conflict: bool`；`request_sync` / `try_sync` 增加 `force: bool`；`DeferredSync` 增加 `force`。
- `try_sync`：`Write && system_changed && !force` → 不写，置 `out_of_sync=true`、`conflict=true`、状态栏 Warning（提示点 Overwrite）。
- 写盘成功：`confirm_system_hosts(&written)`、`conflict=false`、`out_of_sync=false`。
- `apply_hosts` 固定 `force=true`：用户显式点 Apply/Overwrite 即视为确认覆盖。
- 启动检测沿用：基线初始化为 current，因此启动漂移只报 out_of_sync、不报 conflict。

### 5.3 UI

- 底栏按钮文案：`conflict ? "Overwrite" : "Apply"`；冲突时若无其它状态消息，显示 `System hosts changed externally`（Warning）。

### 5.4 测试

- `FakePlatform` 的系统 hosts 改为 `Arc<Mutex<String>>`，测试可模拟“外部程序改了 hosts”。
- 新增单测：外部修改后 `sync_plan` 返回 `system_changed=true`；`confirm_system_hosts` 更新基线后同样内容不再判冲突；`InSync` 会刷新基线（之后的外部修改仍能检测）。
- app 层冲突交互（按钮文案、冲突时不写盘）不做单测，真机 UI 验证（单测不是交付判据，规则允许）。

## 6. 文件结构（触及域）

- **P1-A**：`src/service/env_manager.rs`、`src/service/hosts_validator.rs`、`src/service/hosts_merger.rs`、`src/storage/config_store.rs`、`src/model/{app_state,env,mod}.rs`、`src/logs.rs`、`src/main.rs`
- **P1-B**：`src/platform/{macos,windows,win_protocol,traits,mod}.rs`
- **P1-C**：`src/ui/app.rs`、`src/ui/impls/egui/{mod,editor,sidebar,new_env_dialog,theme,wgpu_config}.rs`
- **P2**：`src/service/env_manager.rs`、`src/ui/app.rs`、`src/ui/impls/egui/mod.rs`（与 P1 同文件，故 P2 必须排在 P1 之后）
- **文档**：`docs/plans/*`、`.gitignore`（待讨论 #1）、`README.md`（待讨论 #6）

## 7. 存量违规清单（2026-11-08 审计，行号以当前工作区为准）

| 文件 | 违规 | 处理 |
|---|---|---|
| `platform/macos.rs` | `register_privilege_right` L175 44 行；`write_system_hosts` L233 50 行；`ensure_elevated` L325 69 行；私有 `next_marker` 在 pub struct 前、`register_privilege_right` 夹在 impl 之间；marker 轮询与回读校验各两份；`authorize` 名不达意 | P1-B 拆分 + 归位 |
| `platform/windows.rs` | `ensure_helper` L90 61 行；`run_helper` L487 145 行、嵌套 4 层；`pub run_helper` 排在私有函数之后且只需 `pub(super)`；写临时文件→Apply→删临时文件 的路径两份、提权分流三份、锁获取三份（错误文案不一）、回读校验三份；`pipe_available` / `write_hosts_elevated` / `read_exact` / `write_all` 名实不符 | P1-B 拆分 + 去重 |
| `platform/win_protocol.rs` | 私有 `decode_text`、`lex_normalize` 夹在 pub 之间；u32 LE 长度前缀在 encode_command / encode_response 各写一份 | P1-B 归位 + 抽 `encode_frame` |
| `platform/mod.rs` / `traits.rs` | `pub mod macos/windows` + 平台类型 re-export 过宽；`///` 文档含实现细节与不准确描述 | P1-B 收窄可见性、修文档 |
| `service/env_manager.rs` | `load_envs` L65 72 行；`create_env` L228 41 行；`rename_env` L299 68 行；成员排列（私有 `load_envs`/`ensure_local_env` 在 pub 方法之前）；测试 `sync_cases` L675 42 行；`rename_env` 无调用方 | P1-A；未用 API 见待讨论 #5 |
| `service/hosts_validator.rs` | `validate` L30 59 行 | P1-A |
| `service/hosts_merger.rs` / `storage/config_store.rs` | 无超长/排列违规；`extract_env_content`、`rename_env_file` 无调用方（`#[allow(dead_code)]`）；`create_env`/`rename_env` 环境名校验是两份近似实现 | P1-A 去重；未用 API 见待讨论 #5 |
| `ui/impls/egui/mod.rs` | `draw_frame` L56 229 行、嵌套 5 层；`run` 可见性过宽；`format!("{}:", "Active:")` 字面量套 format | P1-C |
| `ui/impls/egui/editor.rs` | `editor_ui` L13 92 行、嵌套 4 层；命名非动作；`pub` 过宽；21 行两语句挤一行 | P1-C |
| `ui/impls/egui/sidebar.rs` | `sidebar_ui` L105 102 行、嵌套约 7 层；`GreenCheckbox::ui` L29 64 行；私有 struct/impl 在 pub fn 之前；`pub` 过宽；硬编码十六进制配色（浅色主题实际是深色底） | P1-C（配色另见待讨论 #13） |
| `ui/impls/egui/new_env_dialog.rs` | `new_env_dialog` L5 52 行、嵌套 4 层；命名非动作；渲染层内直接改 `name.clear()` | P1-C |
| `ui/impls/egui/theme.rs` | `build_visuals` L68 47 行；私有函数夹在两个 pub fn 之间；`RADIUS_WINDOW` 应私有、其余可 `pub(super)`；`theme_colors` 39 行临界 | P1-C |
| `ui/impls/egui/wgpu_config.rs`、`ui/mod.rs`、`ui/impls/mod.rs`、`ui/app.rs` | 无长度/嵌套/排列违规；仅可见性可收紧（`run`、`UiError::new`） | P3 可选 |
| 仓库级 | 非 rustfmt-clean（`src/logs.rs` 起就有 diff） | 待讨论 #2 |
| 4 个文件 12 处老注释 | 反引号/箭头/Markdown 写法 | 待讨论 #3 |

## 8. 任务步骤

### P0 前置（文档与提交基线）

- [ ] `.gitignore` 改为忽略 `docs/superpowers/`、放行 `docs/plans/`；`git add` 仅加 `docs/plans/`（含 `AGENTS.md`、`archive/README.md`、本计划），提交为文档基线 commit。
- [ ] 读 `GIT_AGENTS.md`（执行前置，commit 规则）。
- [ ] 工作区现有修复改动跑绿后单独 commit（决策 #4 B）；提交信息只描述“保存激活环境写盘修复 + 收敛式同步”，不夹带后续重构。
- [ ] 更新 `docs/plans/AGENTS.md` 总表（状态行到 ⑤ 后登记）。

### P1-A 服务/存储/模型层重构（行为不变）

- [x] `env_manager.rs`
  - [x] 成员排列：`load_envs`、`ensure_local_env` 移到“内部”私有段。
  - [x] `load_envs` 拆为 `read_env_entries()` / `read_env_entry()` / `sort_envs()`，`load_envs` 只做加载与日志。
  - [x] `create_env` 抽出 `validate_env_name`，主函数只编排。
  - [x] 按决策 #5 删除 `rename_env`、`ConfigStore::rename_env_file`、`HostsMerger::extract_env_content` 及对应单测。
  - [x] 删除后 `cargo check` 无 `dead_code` 告警。
  - [x] 测试用例表改为 `static SYNC_CASES: [SyncCase; 6]`。
- [x] `hosts_validator.rs`：`validate` 拆出 `validate_line` 与 `validation_issue`，`has_errors` 归位到 pub 段。
- [x] `hosts_merger.rs` / `config_store.rs` / `model/*` / `logs.rs` / `main.rs`：复核完成，除删除未用 API 外无改动。
- [x] 验收：`cargo check --all-targets`（0 warning）与 `cargo test`（37 passed）全绿。

### P1-B 平台层重构（行为不变，风险最高）

按以下顺序做，每步后跑宿主 + Windows target 的 `cargo check`（Windows 无法跑单测，`win_protocol` 单测可在本机跑）：

- [ ] B1 `win_protocol.rs`（最安全起点）：pub 全前置、私有（`decode_text` / `lex_normalize` / 新增 `encode_frame`）后置；`encode_command` / `encode_response` 的 u32 LE 长度前缀抽 `encode_frame`；`decode_command` 对空 APPLY 路径的行为保持不变；12 个现有单测全绿。
- [ ] B2 可见性/排列纯搬移：`platform/mod.rs` 的 `pub mod macos/windows` 收为私有、删平台类型 re-export（`HostsPlatform`、`create_platform`、`take_over_entrypoint` 保留，后两者可降 `pub(crate)`）；`run_helper` 改 `pub(super)`；macos 的 `next_marker`、`register_privilege_right` 归位到私有尾部；traits.rs 修 `///` 里不准确的“需要提权”与实现细节。
- [ ] B3 `windows.rs` 拆分：
  - `ensure_helper` → `launch_helper()` + `wait_helper_ready()`，日志文案与先后顺序逐字保留。
  - `run_helper`（目标 ≤40 行）→ `init_helper_logging()` / `ensure_single_helper()` / `start_idle_watchdog()` / `pipe_security()` / `try_accept_connection()` / `trusted_client()` + `serve_client()`；`SECURITY_ATTRIBUTES` 指针与 `CreateNamedPipeW`/`ConnectNamedPipe` 关闭路径原样保留。
- [ ] B4 `windows.rs` 去重（行为敏感，单独一步）：`apply_via_helper` / `flush_via_helper` / `write_hosts_direct` 收敛两条写临时文件路径与三处提权分流；锁错误文案通过参数保留原文本（`write lock poisoned` / `flush lock poisoned` 不合并措辞）；回读校验复用。
- [ ] B5 `macos.rs` 拆分：`write_system_hosts` → `prepare_temp_hosts` / `copy_hosts_as_root`（命令串逐字不动）/ `wait_for_marker` / `verify_hosts_written`；`flush_dns_cache` 复用 `wait_for_marker`；`ensure_elevated` → `register_right_once` / `copy_admin_rights` / `invalidate_auth_ref`，顶层 loop 与错误码分支逐字保留；`register_privilege_right` 抽 `cf_string` / `cf_release` 成对。
- [ ] B6 重命名（仅命名，不改逻辑）：macos `authorize` → 体现“取缓存的授权引用”；windows `pipe_available` / `write_hosts_elevated` / `read_exact` / `write_all`；win_protocol `parse_helper_args` 是否改 `is_helper_invocation`（低优先，可跳过）。
- [ ] B7 修复 macOS `CFStringCreateWithCString` 的 NUL 结尾 UB（决策 #8）：`cf_string` 内部用 `CString::new` 传指针并保证生命周期覆盖调用；行为不变。
- [ ] B8 修复 Windows `apply_temp_file` 白名单恒真（决策 #9）：校验客户端 `tmp_path`（前缀 + 规范化 + 普通文件）后只接受约定临时文件；`win_protocol` 补对应单测；主进程/helper 同版本一起构建，无跨版本兼容问题。
- [ ] B9 记录不修项（决策 #10）：Windows 管道 SD 泄漏写入本计划备注，不引入 `LocalFree`；存量 `unwrap`（macos 121/122/341/342、windows 249）按 RUST §1 不动。
- [ ] 验收：宿主与 Windows target 的 `cargo check`；`cargo test`；macOS 提权链路只能真机人工验收（单次授权、hosts 写入、DNS flush）。

**P1-B 真缺陷处置（决策已定）**：

- macOS `CFStringCreateWithCString` 无 NUL 结尾 UB → B7 修。
- Windows `apply_temp_file` 白名单恒真 → B8 修。
- Windows 管道安全描述符 `sd` 泄漏 → 记录不修（`LocalFree` 时机在 `CreateNamedPipeW` 之后，改动风险大于收益）。

### P1-C UI 层重构（行为不变，视觉由用户确认）

按以下顺序做，每步 `cargo check --all-targets && cargo test`；唯一布局测试 `test_sidebar_layout_stable` 依赖 `draw_frame` 入口与 Panel id/顺序，不可改：

- [ ] C1 `theme.rs`（纯视觉、无布局）：`apply_theme` 移到 `build_visuals` 之前；`build_visuals` 拆 `base_visuals` / `apply_surface_colors` / `apply_state_colors` / `apply_widget_style`（写覆盖顺序不变）；`RADIUS_WINDOW` 私有、其余 `pub(super)`。
- [ ] C2 `new_env_dialog.rs`：改名 `show_new_env_dialog`；拆 `draw_name_field` / `draw_dialog_buttons`（返回动作枚举）；`Window::open`、`request_focus`、Create 先于 Close 的处理顺序保持；`pub(super)`。
- [ ] C3 `editor.rs`：改名 `draw_editor`；拆 `draw_editor_header` / `draw_text_editor` / `draw_save_row`；Frame 与 `set_min_size` 留原位；TextEdit 自动 id 依赖 widget 调用顺序，header/间距/滚动区顺序不得变。
- [ ] C4 `sidebar.rs`：`GreenCheckbox::ui` 拆 `fill_color` / `border_color` / `paint_tick` / `paint`；`sidebar_ui` 改名 `draw_sidebar`，拆 `draw_sidebar_header` / `draw_env_list` / `draw_env_row`（行内再分显示名 / 右键菜单 / 激活开关）；`ui.id().with("plus_env_btn")` 的取存位置与 `actions` push 顺序（Switch / Delete / Toggle）保持。
- [ ] C5 小共享项（只动 sidebar + mod 的小段）：`"Current Machine"` 提为 ui 层常量；顶栏/底栏共用 `bar_frame`（仅 margin 不同）。
- [ ] C6 `egui/mod.rs`（最大，最后）：`draw_frame` 拆 `draw_top_bar` / `draw_status_bar` / `draw_sidebar_panel` / `draw_editor_panel` / `draw_new_env_dialog` / `draw_console_window`；保持 Panel 顺序 top→bottom→left→Central、闭包外 handle、`c` 每帧只取一次、BUFFER 锁不跨 `app.handle`（LogLayer 会再锁同一 std Mutex，会死锁）。
- [ ] 验收：`cargo check --all-targets && cargo test`；UI 视觉交用户确认（按全局规则不做截图自检）。

### P2 乐观锁（行为变更，独立一批）

- [ ] `env_manager.rs`：加 `system_hosts_baseline`、`sync_plan(&mut self)` 返回 `system_changed`、`confirm_system_hosts`；补 3 条单测（FakePlatform 共享 `Arc<Mutex<String>>`）。
- [ ] `app.rs`：`PendingOp.written`、`conflict`、`force` 贯通（request/try/deferred）、成功回填基线、冲突提示。
- [ ] `egui/mod.rs`：底栏 `Apply`/`Overwrite` 文案 + 冲突提示。
- [ ] `README.md`：补充“保存激活环境即应用、外部修改需确认覆盖”的说明（待讨论 #6 决定）。
- [ ] 验收：`cargo check && cargo test`；真机步骤：手动往 `/etc/hosts` 加一行 → 改 `dev` 条目保存 → 应提示冲突且不写盘 → 点 Overwrite → 弹一次授权并写入。

### P3 收尾（决策已定）

- [ ] 全量 `cargo fmt`（决策 #2），之后把 `cargo fmt --check` 纳入验证命令；必须放在所有重构之后单独一步。
- [ ] 老注释规范化 A2（决策 #3）：12 处反引号/箭头/Markdown 去掉；本地约定/外部产品引用（「Pattern C」「SwitchHosts 同款」「QEMU 无 GPU」「便于本机单测」「对齐 macOS marker 校验语义」等）改写为“代码本身在做什么”，逐条列 diff；不删信息、不改业务语义。
- [ ] 可见性收紧（行为不变）：`ui::run`、`UiError::new` 改 `pub(super)`/`pub(crate)`；`HostswitchApp` 只被 `handle`/`tick` 外部调用的 7 个方法可改私有（外部实现层只调 `backend.handle` 与 `tick`）。
- [ ] 重复文案收敛（可选）：`format!("{}: {}", 字面量, ...)` 统一成 `fail_status()` 小助手；`"Current Machine"` 常量随 C5 落地。
- [ ] 主题 token 修正（决策 #13，独立任务）：sidebar/顶栏/状态栏硬编码配色改 `ThemeColors` token；若浅色 token 缺对应色值则补字段；改完交你在浅色主题下确认观感。
- [ ] 文档归档：全量测试 → `git mv` 计划到 `archive/` → 更新 `archive/README.md` 与总表 → ⑦。

## 9. 自检结论

- **覆盖度**：§7 审计清单每一项都有 P1 落点；乐观锁独立成 P2，不与重构混批；14 项决策全部落入 §4.1 与任务步骤。
- **一致性**：与已锁定决策 1~7 一致；沿用前序已定行为，不引入新交互（主题 token 属 P3 独立视觉改动，已隔离）。
- **取舍风险**：（1）平台层 unsafe/管道代码拆分风险最高，只做纯搬移与子函数抽取，不做逻辑改写；（2）UI 拆分无自动视觉验证，需用户确认；（3）rustfmt 全量格式化会重写所有行（已授权），必须最后单独做；（4）commit 已授权（每任务独立、不 push），但执行时仍只加明确路径。
- **grill 结论**：见 §11（顺序依赖、commit 粒度、验证命令、真机验证点、回滚与残留）。

## 10. 待讨论问题（已收敛，保留背景供追溯）

**10.1 `docs/plans` 是否纳入版本管理**
- 背景：SOP 要求计划文件进仓库并随阶段 commit；当前 `.gitignore` 是 `/docs/` 整目录忽略，`docs/plans` 无法被跟踪。`docs/superpowers/` 是历史过程稿，应继续忽略。
- 选项：A 改 `.gitignore` 为“忽略 `docs/superpowers/`、放行 `docs/plans/`”；B 保持整目录忽略，计划只留本地（偏离 SOP）；C 计划移到仓库外（不推荐）。
- 建议：**A**。代价是 `docs/` 下其它杂项可能被看到，需顺手确认忽略规则。

**10.2 是否采用 rustfmt 全量格式化**
- 背景：仓库当前非 rustfmt-clean（`cargo fmt --check` 从 `src/logs.rs` 起就有 diff，`editor.rs` 还有两条语句挤一行），说明历史代码手写/AI 格式混杂。
- 选项：A 重构全部完成后（P3）一次性 `cargo fmt`，之后把 `cargo fmt --check` 纳入门槛；B 不跑 rustfmt，只保证新代码手写一致；C 只格式改动过的文件（风格会更混杂，不推荐）。
- 建议：**A**，放在最后一批独立做，避免每批重构 diff 混进格式噪音。

**10.3 12 处老注释是否规范化**
- 背景：`sidebar.rs`、`win_protocol.rs`、`windows.rs`、`hosts_merger.rs` 共 12 处注释含反引号/箭头/Markdown，另有“Pattern C”“SwitchHosts 同款”“QEMU 无 GPU”等本地约定/外部产品引用，与 RUST §3 冲突；CODING §3 默认禁止改用户注释。
- 选项：A1 只改格式（反引号/箭头去掉，语义与措辞不动）；A2 按新规则改写（格式 + 删本地约定/外部产品引用，逐条列出 diff）；B 全部保持原样，只在新增注释里规避。
- 建议：**A1**（你已经授权注释可重构，A1 不丢信息、风险最小）；A2 可作为单独一步，但需你逐条确认改写文案。

**10.4 工作区现有未提交改动如何处置**
- 背景：`git status` 有 4 个 src 文件改动（保存激活环境不写盘的修复 + 收敛式同步 + 局部风格整理），尚未真机验证。
- 选项：A 直接叠加进本次 P1；B 先单独提交这份修复（前置 commit），再做重构任务；C stash/回退（会丢工作，不推荐）。
- 建议：**B（若 10.7 授权 commit）**，修复与重构分开更好审；不授权 commit 则选 A。

**10.5 三处无调用方 API 去留**
- 背景：`EnvManager::rename_env`（68 行，无 UI 入口）、`ConfigStore::rename_env_file`（只被前者调用）、`HostsMerger::extract_env_content`（无调用，但有单测）均带 `#[allow(dead_code)]`。
- 选项：A 三处删除（含对应单测）；B 全部保留；C 删 `rename_env`+`rename_env_file`，保留 `extract_env_content`（“从系统 hosts 导入段”可能还会用）。
- 建议：**A**，重构期清理死代码最干净；若你想留着以后做“环境重命名/导入”，选 C。

**10.6 README 是否同步行为说明**
- 背景：README 的「关于权限」只写了提权机制，没写“保存激活环境即写盘”“未激活环境只存文件”“启动漂移只提示”“冲突需 Overwrite”。
- 选项：A 本轮更新；B 不动。
- 建议：**A**，这是给人看的唯一说明书，行为变了应同步。

**10.7 执行期是否授权按 SOP 每任务 commit**
- 背景：全局规则默认禁 commit；SOP ⑥ 要求每任务独立 commit（push 永远禁止）。
- 选项：A 授权（每批/每任务一个 commit，提交信息按 `GIT_AGENTS.md`）；B 不授权，只改文件，最后由你统一提交。
- 建议：**A**，否则回滚粒度差、SOP 的 ⑥ 无法完整执行；执行前我会先读 `GIT_AGENTS.md`，只用明确路径 `git add`，不做 `git add -A`。

**10.8 macOS CFString NUL 结尾 UB 修不修**
- 背景：`register_privilege_right` 把 `&str::as_ptr()` 传给 `CFStringCreateWithCString`，字符串没有 NUL 结尾 → 存量越界读（UB），位于每次启动首次写盘前的提权注册路径。
- 选项：A 本轮修（改用 `CString`，行为不变，真机需确认授权框仍正常）；B 记录另立。
- 建议：**A**，属于真 UB 且修复局部；提权链路改动后需你本机验收一次授权框。

**10.9 Windows `apply_temp_file` 白名单恒真修不修**
- 背景：白名单用常量 `WINDOWS_HOSTS_PATH` 构造 target 比较（恒真），来自管道的 `tmp_path` 未被校验；helper 以管理员权限运行，同用户进程理论上可让它读任意路径。
- 选项：A 本轮修（前缀 + 规范化 + 普通文件校验，配 `win_protocol` 单测；运行时仍需 Windows 真机验）；B 记录另立安全计划。
- 建议：**A**（安全相关、改动局限于一个函数与协议校验），接受“只能交叉编译 + 本机协议单测”的验证上限。

**10.10 Windows 管道安全描述符 `sd` 泄漏修不修**
- 背景：`ConvertStringSecurityDescriptorToSecurityDescriptorW` 分配的 SD 从不 `LocalFree`；只泄漏一次且进程退出即回收，但若以后 helper 改成可复用/重连就要处理。
- 选项：A 记录不修；B 本轮修（需调整生命周期，Windows 真机验证）。
- 建议：**A**，风险收益不划算，记入计划备注即可。

**10.11 Windows 三处提权分流去重后的锁错误文案**
- 背景：三处重复逻辑里锁错误文案不同（`write lock poisoned` / `flush lock poisoned`）；去重时若统一措辞，用户可见错误文案会变。
- 选项：A 通过参数保留原文本（推荐）；B 统一成一句。
- 建议：**A**，纯重构不动对外文案。

**10.12 平台层整文件搬移老行确认**
- 背景：CODING §2 要求老代码最小改动、不搬函数；彻底重构必然整文件搬移。你已明确 blame 无所谓。
- 选项：A 确认按 P1-B 全量搬移；B 只拆函数不搬位置（成员排列违规保留）。
- 建议：**A**。

**10.13 硬编码配色改主题 token**
- 背景：sidebar 复选框底色/对勾、状态栏警示黄、顶栏文字色等用 `from_rgb` 写死深色值（与 dark token 相同），浅色主题下这些控件仍是深色底/固定色。
- 选项：A 本轮改 `ThemeColors` token（浅色主题观感会变，需你看一眼）；B 记录另立（保持本轮视觉零变化）。
- 建议：**A**，改动小且属于规范化；但它不是纯重构，建议放在 P1 之后的独立任务。

**10.14 UI 函数重命名**
- 背景：`editor_ui`、`sidebar_ui`、`new_env_dialog` 等是名词命名；RUST §4.3 倾向动作名（`draw_editor` / `draw_sidebar` / `show_new_env_dialog`），且拆分后会新增一批 `draw_*`，不改名会新旧混杂。
- 选项：A 按计划改名（纯命名 diff）；B 保持原名。
- 建议：**A**。

> 以上 14 项已于 2026-11-08 收敛（结论见 §4.1），本节仅保留背景与建议供追溯。

## 11. ④ grill 结论（边界 / 失败模式 / 更简等价方案）

**11.1 顺序与依赖**

- 执行顺序固定：P0（docs 基线 + 现有修复 commit）→ P1-A → P1-B → P1-C → P2 → P3；P2 与 P1-A/P1-C 改同一批文件，必须等 P1 完成；P3 的 rustfmt 必须最后，否则格式 diff 混入每一批。
- P1-A/B/C 三组之间文件不重叠，理论上可并行；实际串行执行，降低交叉验证成本。若改并行，必须文件级隔离且每批独立 commit。

**11.2 commit 粒度（决策 #7 授权）**

- P0：commit①`docs: 初始化 plans 管理`、commit②`fix: 保存激活环境同步系统 hosts`（现有改动，先跑 `cargo check --all-targets && cargo test`）。
- P1/P2/P3：每完成一个二级任务且验证命令全绿后独立 commit（预计 A 4 个、B 9 个、C 6 个、P2 2 个、P3 5 个左右）；不跨任务合并；`git add` 只用明确路径，禁止 `git add -A`。
- P2 按 TDD：先补单测（此时红）→ 实现 → 全绿 → 一个 commit；红测试不单独提交。
- 提交信息按 `GIT_AGENTS.md`；push 永不执行。

**11.3 验证命令（每步硬门槛）**

- P1-A/P1-C/P2：`cargo check --all-targets && cargo test`。
- P1-B：上述命令 + `cargo check --target x86_64-pc-windows-msvc --all-targets`（已实测可用，首次 2m04s，之后缓存命中）。
- P3：追加 `cargo fmt --check`。
- 本机沙箱只允许写工作区：`CARGO_HOME=/tmp/hostswitch-cargo-home`、`CARGO_TARGET_DIR=$PWD/target`；`target/` 已被忽略。

**11.4 真机人工验证点（必须用户配合，不做截图自检）**

- B5 之后：macOS 点一次保存激活环境 → 授权框只弹一次、hosts 写入、DNS flush 生效。
- B7 之后：授权框文案与授权流程仍正常（CFString 修复不改变显示）。
- P2 之后：手动往 `/etc/hosts` 加一行 → 改 `dev` 保存 → 提示冲突且不写盘 → 点 `Overwrite` → 授权一次并写入。
- P3 主题 token 之后：浅色/深色主题各看一眼。
- Windows 侧：本机只能交叉编译 + `win_protocol` 单测；`apply_temp_file` 修复（B8）与去重（B4）需在 Windows 机器上真机跑一次写盘/停用，否则标注“仅交叉编译通过”。

**11.5 失败模式与回滚**

- 每步一个 commit：任一步引入回归，`git revert` 该 commit 即可回退，不影响其它批。
- 拆分风险最高的是 `run_helper` 与 `draw_frame`：保持“只搬移、不改逻辑”，若 diff 中出现逻辑改动视为执行错误；UI 拆分以 `test_sidebar_layout_stable` 兜底，必要时让用户看一帧。
- 若某步无法做到 ≤40 行且行为不变（例如 `run_helper` 的 unsafe 生命周期强耦合），允许该函数保留 >40 行并在任务清单标注原因，不硬拆。

**11.6 边界与已知残留**

- 乐观锁的检查与写入之间仍有 TOCTOU 窗口（决策已接受）；多实例同时运行各自基线的行为未定义，不处理。
- 启动时改过的外部 hosts 不会判冲突（基线=启动时读到的内容），属预期：启动只报漂移。
- 删除三处无调用方 API 会减少 1 个单测（`extract_env_content`），commit 信息里注明。
- 存档主题 token 改动是行为/视觉变化，与 P1 的“行为不变”隔离，单独任务单独 commit。

**11.7 更简等价方案复核**

- 事件枚举 vs 收敛：已选收敛，不复核。
- 乐观锁用 flock/原子替换：比“基线对比 + 用户确认”复杂且桌面工具收益低，不采用。
- 无调用方 API 用 `#[allow(dead_code)]` 保留：违反最小可见性，不采用。
- 注释 A2 用脚本批量替换：12 处且文案需重写，手工 + 逐条 diff 更稳。

**11.8 执行安排建议**

- 建议分 3 个会话/批次：① P0+P1-A+P1-B；② P1-C；③ P2+P3。每批结束向用户汇报验证结果与未覆盖项（尤其 Windows 真机、UI 视觉）。

> **文档已定稿，是否开始执行？** 未获明确指令前不动代码。

