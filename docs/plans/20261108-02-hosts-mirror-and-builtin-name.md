# 内置环境改为系统 hosts 只读镜像 + 启动漂移仅提示

- **状态：⑥ 执行中**（2026-11-08 用户确认“按推荐”后开工）
- **日期**：2026-11-08
- **来源**：黑盒测试反馈（用户 2026-11-08 三条需求）
- **归档去向**：`docs/plans/archive/`（⑦ 时 `git mv`，文件名与内容不变）

## 1. 背景与意图

黑盒测试后用户提出三点：

1. 内置项名称从 Current Machine 改成系统 hosts 的真实路径（macOS `/etc/hosts`，Windows `C:\Windows\System32\drivers\etc\hosts`）。
2. 内置项要准确反映当前系统 hosts 文件内容，每次写盘后同步体现，方便一眼看出当前配置。
3. 启动发现不一致时不要 Apply 流程，只在底栏提示。

## 2. 已锁定决策

- Q1 = A：内置项改为**只读镜像**，启动、切到该项、写盘成功后都重读系统 hosts 刷新；用户仍可在其他环境里编辑。
- 内置项显示名统一为 `hosts`（各平台一致），不再显示完整路径。
- 写入系统 hosts 时环境段注释带上环境 id（`# ===== hostswitch: env <id> start/end =====`），用于区分每块属于哪个环境；旧格式注释继续兼容解析。
- Q2 = A：启动漂移只显示提示文字，不出现 Apply 按钮，不做自动写；用户后续保存激活环境 / 启停 / 删除时自动同步。
- Q3 = A：运行期发现系统 hosts 被外部改过时仍保留 Overwrite 确认（乐观锁），只有“启动漂移”免 Apply。
- 展示名：侧栏、顶栏、编辑器统一显示 `hosts`；侧栏按钮 hover 显示完整路径由后续需要再定。

## 3. 实现要点

- `EnvManager`：新增 `hosts_file_path()`、`refresh_local_mirror()`（读系统 hosts 写入内置环境文件并返回内容）、`sync_local_mirror(content)`（写盘成功后回填镜像）；启动时刷新镜像并作为基线。
- `build_merged`：内置环境文件可能存整份 hosts，先 `extract_local_content` 取系统默认段再参与合并。
- `HostswitchApp`：切到内置项前刷新镜像；内置项保存动作改为刷新；写盘成功后同步镜像并更新编辑器内容；冲突时刷新镜像。
- UI：内置项编辑器只读（无 Save 按钮、TextEdit 不可编辑）；底栏只在 conflict 时显示 Overwrite，不再有启动 Apply。
- 测试：镜像刷新、镜像回填后仍 InSync；既有 42 条单测全绿。

## 4. 验收

- `cargo fmt --check && cargo check --all-targets && cargo test` 全绿（含 Windows target 交叉 check）。
- 真机：内置项显示 `/etc/hosts` 且只读；启动漂移只有提示；写盘后切回内置项内容等于系统文件；外部修改后保存 env 出现 Overwrite。
