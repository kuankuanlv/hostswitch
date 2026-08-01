# hostswitch 产品规格说明书（Spec v1.0）

> 纯 Rust + egui 实现的跨平台 hosts 文件管理与切换工具

---

## 一、项目概述

### 1.1 项目信息

| 项 | 值 |
|---|---|
| 项目名 | hostswitch |
| 配置目录 | `~/.hostswitch/`（用户主目录下的隐藏目录） |
| 技术栈 | Rust + egui/eframe |
| 首发平台 | macOS（Windows / Linux 留 TODO） |
| 分层架构 | 中等分层：UI → Service → Platform |
| 开源协议 | 待定 |

### 1.2 目标

- 管理多套 hosts 配置方案（env），一键切换
- 纯 Rust 实现，无 WebView 依赖，轻量快速
- 跨平台（macOS 先行，后续扩展 Windows/Linux）
- 自动备份、DNS 刷新、格式校验

### 1.3 技术选型说明（egui）

| 维度 | 评估 | 说明 |
|---|---|---|
| 纯 Rust | ✅ 满足 | 全程 Rust，心智统一，不写 JS |
| 轻量快速 | ✅ 满足 | 启动快、内存占用小，比 WebView 轻很多 |
| 文本编辑能力 | ⚠️ 基础款 | egui::TextEdit 无行号、无语法高亮，hosts 编辑勉强够用 |
| 中文输入法 | ⚠️ 一般 | 选字窗不跟随光标，hosts 文件以英文/IP 为主，影响不大 |
| 无障碍 | ⚠️ 较弱 | 屏幕阅读器支持有限，自用工具可接受 |

**结论：对于 hosts 管理这种轻量工具，egui 完全够用。**

---

## 二、架构设计

### 2.1 分层架构

```
┌─────────────────────────────────┐
│        UI Layer (egui)          │
│  负责渲染、交互、状态展示         │
│  sidebar / editor / backup_dialog│
└──────────────┬──────────────────┘
               │
┌──────────────▼──────────────────┐
│      Service Layer (业务逻辑)    │
│  EnvManager                      │
│  HostsValidator（格式校验）       │
│  HostsMerger（合并逻辑）          │
└──────────────┬──────────────────┘
               │
┌──────────────▼──────────────────┐
│   Platform Layer (平台抽象)      │
│  trait HostsPlatform             │
│  ├─ macOS 实现                   │
│  ├─ Windows 实现（TODO）         │
│  └─ Linux 实现（TODO）           │
└──────────────┬──────────────────┘
               │
┌──────────────▼──────────────────┐
│    Storage Layer (存储层)        │
│  ConfigStore                     │
│  配置文件读写、备份管理           │
└─────────────────────────────────┘
```

### 2.2 模块划分

```
src/
├── main.rs                 # 入口，启动 egui 窗口
├── app.rs                  # App 主结构，串联所有模块
├── model/                  # 数据结构
│   ├── env.rs              # Env 结构体
│   └── app_state.rs        # AppState + BackupRecord
├── storage/                # 存储层
│   └── config_store.rs     # 配置文件读写、备份管理
├── platform/               # 平台抽象层
│   ├── mod.rs              # HostsPlatform trait 定义
│   └── macos.rs            # macOS 实现
├── service/                # 业务逻辑层
│   ├── env_manager.rs      # env 管理核心逻辑
│   ├── hosts_validator.rs  # hosts 格式校验
│   └── hosts_merger.rs     # hosts 合并逻辑
└── ui/                     # UI 层
    ├── sidebar.rs          # 左侧 env 列表
    ├── editor.rs           # 右侧文本编辑器
    └── backup_dialog.rs    # 备份历史弹窗
```

---

## 三、配置目录与文件结构

### 3.1 目录结构

```
~/.hostswitch/
├── state.json          # 全局状态：当前激活的env、设置、备份记录
├── envs/               # 每个env一个独立文件，内容就是纯hosts文本
│   ├── local           # 内置env，名字不可改
│   ├── dev             # 用户自建env
│   └── test
└── backups/            # 历史备份
    ├── local_20260801_221530
    ├── dev_20260801_230512
    └── ...
```

### 3.2 设计理由

- **各 env 独立文件**：心智负担小，直接 cat 就能看内容
- **state.json 只存状态**：不存大文本，读写快
- **备份独立文件**：恢复方便，出问题直接拷回去就行

---

## 四、核心概念：Env（环境）

### 4.1 Env 定义

Env = 一套独立的 hosts 配置方案，用户可以创建多个，一键切换。

### 4.2 local 内置 Env

- 名字固定叫 `local`，不可修改
- 不可删除
- 内容可编辑
- 首次启动时，自动读取当前系统 `/etc/hosts` 作为 local 的初始内容
- 作为合并时的基础层（兜底）

### 4.3 用户自建 Env

- 用户可以新增/删除/重命名
- 数量不限
- 内容完全由用户编辑

### 4.4 互斥规则

同时只能有一个 env 处于激活状态（单选互斥）。

---

## 五、合并模式规则

### 5.1 策略：前置追加 + 先匹配生效

```
最终系统 hosts = 当前 env 的内容 + local 的内容
```

- env 的条目放前面 → 相同域名时 env 的 IP 先生效
- local 的条目放后面 → 兜底

### 5.2 为什么不做智能合并去重？

- hosts 文件的语义就是"先到先得"，前置追加天然实现了 env 优先
- 不需要解析每一行，实现简单
- 行为可预测，用户能直接看到完整文件内容

### 5.3 写入格式

```
# ===== hostswitch: 环境配置开始 =====
（env 的内容）
# ===== hostswitch: 环境配置结束 =====

# ===== hostswitch: 系统默认(local) =====
（local 的内容）
```

---

## 六、核心流程

### 6.1 启用某个 Env（完整流程）

```
用户点击「启用此环境」按钮
    ↓
1. 自动保存当前编辑内容
    ↓
2. 格式校验（检查 hosts 格式是否合法）
    ↓ 校验失败 → 弹窗提示错误，终止
3. 备份当前系统 hosts 到 backups/ 目录
    ↓
4. 合并内容：env内容 + local内容
    ↓
5. 提权写入系统 hosts 文件（弹系统授权框）
    ↓ 写入失败 → 弹窗提示，终止
6. 刷新 DNS 缓存
    ↓
7. 更新 state.json 中的 active_env
    ↓
8. 提示成功
```

### 6.2 切换选中 Env（UI 切换，不影响系统）

```
用户点击左侧 env 列表
    ↓
右侧编辑器加载该 env 的内容
    ↓
（只是 UI 切换，系统 hosts 文件不变）
```

### 6.3 保存 Env 内容（存配置，不改系统）

```
用户点击「保存」
    ↓
写入 ~/.hostswitch/envs/{env_id}
    ↓
提示保存成功
```

---

## 七、UI 设计

### 7.1 主界面布局

```
┌─────────────────────────────────────────────────┐
│  hostswitch                            [备份历史] │
├──────────┬──────────────────────────────────────┤
│ 环境     │  dev - 编辑中                         │
│          │  ┌──────────────────────────────────┐ │
│ ● local  │  │ # dev 环境                        │ │
│ ○ dev    │  │ 192.168.1.100  api.dev.com       │ │
│ ○ test   │  │ 192.168.1.101  web.dev.com       │ │
│          │  │                                  │ │
│ [+ 新增] │  │                                  │ │
│          │  └──────────────────────────────────┘ │
│          │                                        │
│          │  [保存]  [启用此环境]  [放弃修改]      │
└──────────┴──────────────────────────────────────┘
```

### 7.2 左侧 Env 列表

- 显示所有 env，单选互斥
- 当前激活的 env 用 ● 标记（绿色高亮）
- 当前选中的 env 有选中背景
- 右键 env → 删除菜单（local 不可删）
- 底部「+ 新增环境」按钮

### 7.3 右侧编辑器

- 纯文本编辑（等宽字体）
- 顶部显示 env 名称 + 是否当前激活
- 底部操作按钮：
  - **保存**：保存到本地配置（不影响系统 hosts）
  - **启用此环境**：写入系统 hosts（需提权）
  - **放弃修改**：恢复上次保存的内容
- dirty 标记：有未保存修改时显示橙色圆点

### 7.4 新增 Env 弹窗

```
┌──────────────────┐
│  新增环境    [×]  │
├──────────────────┤
│  环境名称：       │
│  [__________]    │
│                  │
│  [确定]  [取消]   │
└──────────────────┘
```

### 7.5 备份历史弹窗

```
┌──────────────────────────────┐
│  备份历史                [×]  │
├──────────────────────────────┤
│  dev_20260801_230512   [恢复] │
│  local_20260801_221530 [恢复] │
│  dev_20260731_201005  [恢复] │
│  ...                         │
│                              │
│  最多保留 5 份，自动清理旧的    │
└──────────────────────────────┘
```

---

## 八、平台抽象层

### 8.1 HostsPlatform trait

```rust
pub trait HostsPlatform {
    // 读系统 hosts 文件内容
    fn read_system_hosts(&self) -> Result<String>;
    
    // 写系统 hosts 文件（需要提权）
    fn write_system_hosts(&self, content: &str) -> Result<()>;
    
    // 刷新 DNS 缓存（需要提权）
    fn flush_dns_cache(&self) -> Result<()>;
    
    // 检查当前进程有没有管理员权限
    fn has_admin_privileges(&self) -> bool;
    
    // 系统 hosts 文件路径
    fn hosts_file_path(&self) -> &str;
}
```

### 8.2 各平台差异点

| 差异点 | macOS | Windows | Linux |
|---|---|---|---|
| hosts 路径 | `/etc/hosts` | `C:\Windows\System32\drivers\etc\hosts` | `/etc/hosts` |
| 行尾符 | LF | CRLF | LF |
| 提权方式 | osascript + 系统授权框 | UAC 弹窗 | pkexec / sudo |
| DNS 刷新 | `dscacheutil -flushcache` + `killall -HUP mDNSResponder` | `ipconfig /flushdns` | 取决于 DNS 服务 |
| 文件权限 | root:wheel 644 | Administrators 可写 | root:root 644 |
| 首发状态 | ✅ 实现 | ⏳ TODO | ⏳ TODO |

### 8.3 macOS 提权实现方案

使用 `osascript` 调用系统授权框：
```bash
osascript -e 'do shell script "..." with administrator privileges'
```

- 点击「启用」时才弹密码框，平时普通权限运行
- 符合 macOS 安全规范
- 用户体验好（系统原生授权框）

---

## 九、数据结构

### 9.1 Env

```rust
pub struct Env {
    pub id: String,              // 唯一标识，也是文件名
    pub name: String,            // 显示名称
    pub is_builtin: bool,        // 是否内置（内置的不能删）
    pub created_at: DateTime<Local>,
    pub updated_at: DateTime<Local>,
}
```

### 9.2 AppState

```rust
pub struct AppState {
    pub version: u32,                // 配置版本
    pub active_env_id: String,       // 当前激活的 env
    pub selected_env_id: String,     // 当前选中的 env（UI上）
    pub backups: Vec<BackupRecord>,  // 备份记录
    pub max_backups: usize,          // 最多保留几份备份（默认5）
}
```

### 9.3 BackupRecord

```rust
pub struct BackupRecord {
    pub filename: String,        // 备份文件名
    pub from_env: String,        // 当时激活的 env
    pub created_at: DateTime<Local>,
}
```

---

## 十、格式校验规则

### 10.1 校验项

| 检查项 | 级别 | 说明 |
|---|---|---|
| 空行 / 注释行 | - | 跳过，不检查 |
| IP + 域名格式 | error | 每行至少两列：IP + 域名 |
| IP 格式合法性 | error | IPv4 四段 0-255，IPv6 含冒号 |
| 域名格式 | warning | 简单检查，不做严格校验 |

### 10.2 校验时机

- 点击「启用此环境」时强制校验
- 校验不通过则阻止写入，提示具体错误

---

## 十一、备份策略

### 11.1 备份时机

- 每次点击「启用此环境」前，自动备份当前系统 hosts

### 11.2 备份命名

格式：`{env_name}_{YYYYMMDD}_{HHMMSS}`

示例：`dev_20260801_230512`

### 11.3 保留策略

- 默认最多保留 **5 份**
- 新备份插在最前面
- 超过数量自动删除最旧的
- 用户可在配置中修改保留数量（后续版本）

### 11.4 恢复

- 备份历史弹窗中点击「恢复」
- 直接将备份内容写回系统 hosts
- 刷新 DNS 缓存
- 激活状态切回 local（因为恢复后不确定对应哪个 env）

---

## 十二、错误处理

### 12.1 错误分类

| 错误类型 | 处理方式 |
|---|---|
| 格式校验失败 | 弹窗显示具体错误行，阻止写入 |
| 提权被取消 | 提示"已取消授权"，不做任何修改 |
| 写入失败 | 弹窗显示错误信息，回滚（从备份恢复） |
| DNS 刷新失败 | 警告级别，不影响主流程，打日志 |
| 配置文件读写失败 | 弹窗提示，尝试降级处理 |

### 12.2 UI 展示

- 编辑器底部状态栏显示操作结果（成功/失败）
- 成功用绿色，失败用红色
- 详细错误信息可展开查看

---

## 十三、MVP 版本范围（v0.1）

### ✅ 必做

- [x] egui 主界面（左侧 env 列表 + 右侧编辑器）
- [x] env 增删改（local 不可删、不可改名）
- [x] env 切换（合并写入系统 hosts）
- [x] 配置文件持久化（独立文件 + state.json）
- [x] macOS 平台支持
- [x] hosts 格式校验
- [x] 自动备份（最多 5 份）
- [x] 备份历史查看与恢复
- [x] DNS 缓存自动刷新

### 🔜 v0.2 规划

- [ ] Windows 平台支持
- [ ] 语法高亮（IP/域名/注释着色）
- [ ] 暗色/亮色主题切换
- [ ] 系统托盘常驻
- [ ] 快捷键切换 env

### 🔮 v1.0 规划

- [ ] Linux 平台支持
- [ ] 结构化编辑视图（表格形式）
- [ ] 单条 hosts 行内启用/禁用
- [ ] import/export
- [ ] 配置项设置面板
- [ ] 批量操作

---

## 十四、关键设计决策记录

| 决策 | 选择 | 理由 |
|---|---|---|
| GUI 框架 | egui | 纯 Rust，轻量，hosts 工具够用 |
| 合并模式 | 前置追加 | 实现简单，利用 hosts 天然语义，行为可预测 |
| 提权方式 | 按需提权（osascript） | 平时普通权限，只在写入时弹授权框，安全 |
| 配置存储 | env 独立文件 + state.json | 心智负担小，易排查，易备份 |
| 配置位置 | ~/.hostswitch/ | Unix 惯例，隐藏目录，不干扰用户 |
| 项目命名 | hostswitch | 简洁，一看就懂 |
| 备份数量 | 默认 5 份 | 够用又不占太多空间 |

---

## 附录：项目依赖清单

| 依赖 | 版本 | 用途 |
|---|---|---|
| eframe | 0.27 | GUI 框架（egui 的桌面封装） |
| egui | 0.27 | 即时模式 GUI 库 |
| serde | 1.x | 序列化/反序列化 |
| serde_json | 1.x | JSON 格式 |
| thiserror | 1.x | 自定义错误类型 |
| anyhow | 1.x | 便捷错误处理 |
| chrono | 0.4 | 时间处理 |
| dirs | 5.x | 获取系统目录路径 |
| tracing | 0.1 | 日志 |
| tracing-subscriber | 0.3 | 日志输出 |
