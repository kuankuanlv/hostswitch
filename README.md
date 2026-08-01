# hostswitch

跨平台 hosts 文件管理与切换工具，纯 Rust + egui 实现，无 WebView 依赖。

## 1. 项目介绍

hostswitch 帮助开发者管理多套 hosts 配置方案（环境），支持一键切换。每个环境对应一份独立的 hosts 文本，切换时将当前环境内容与内置 local 环境合并后写入系统 hosts 文件。

**核心特性：**

- 多套环境管理，单选互斥，一键启用
- 合并策略：当前环境内容在前、local 兜底在后，相同域名时环境配置优先生效
- 启用前自动备份系统 hosts，支持从历史备份恢复
- 写入前格式校验（IP、域名等）
- 写入后自动刷新 DNS 缓存
- macOS 按需提权（osascript 系统授权框），平时以普通权限运行

**技术栈：** Rust、egui/eframe、serde/chrono

**配置目录：** `~/.hostswitch/`

## 2. 功能状态

### 已实现（v0.1）

- egui 主界面（左侧环境列表 + 右侧文本编辑器）
- 环境增删（local 内置环境不可删）
- 环境切换与启用（合并写入系统 hosts）
- 配置文件持久化（独立 env 文件 + state.json）
- macOS 平台支持
- hosts 格式校验
- 自动备份（默认最多保留 5 份）
- 备份历史查看与恢复
- DNS 缓存自动刷新

### 待实现

**v0.2 规划**

- Windows 平台支持
- 语法高亮（IP / 域名 / 注释着色）
- 暗色 / 亮色主题切换
- 系统托盘常驻
- 快捷键切换环境
- 环境重命名 UI

**v1.0 规划**

- Linux 平台支持
- 结构化编辑视图（表格形式）
- 单条 hosts 行内启用 / 禁用
- import / export
- 配置项设置面板
- 批量操作

## 3. 项目目录结构

```
hostswitch/
├── Cargo.toml              # 项目依赖与元信息
├── README.md               # 项目说明
├── hostswitch-spec.md      # 产品规格说明书
└── src/
    ├── main.rs             # 入口，启动 egui 窗口
    ├── app.rs              # 主应用结构，串联各模块
    ├── model/              # 数据结构
    │   ├── env.rs          # Env 结构体
    │   └── app_state.rs    # AppState + BackupRecord
    ├── storage/            # 存储层
    │   └── config_store.rs # 配置文件读写、备份管理
    ├── platform/           # 平台抽象层
    │   ├── mod.rs          # HostsPlatform trait
    │   └── macos.rs        # macOS 实现
    ├── service/            # 业务逻辑层
    │   ├── env_manager.rs  # 环境管理核心逻辑
    │   ├── hosts_validator.rs  # hosts 格式校验
    │   └── hosts_merger.rs     # hosts 合并逻辑
    └── ui/                 # UI 层
        ├── sidebar.rs      # 左侧环境列表
        ├── editor.rs       # 右侧文本编辑器
        ├── new_env_dialog.rs   # 新增环境弹窗
        └── backup_dialog.rs    # 备份历史弹窗
```

**用户配置目录：**

```
~/.hostswitch/
├── state.json          # 全局状态（当前激活环境、备份记录等）
├── envs/               # 各环境 hosts 内容
│   ├── local           # 内置环境
│   └── dev             # 用户自建环境
└── backups/            # 历史备份
```

## 4. 快速开始

**环境要求：** macOS、Rust 1.85+

```bash
# 编译
cargo build --release

# 运行
cargo run --release
```

**Clippy 检查：**

```bash
cargo clippy --all-targets -- -D warnings
```

**运行测试：**

```bash
cargo test
```

## 5. 使用说明

1. 首次启动会自动创建 `local` 环境，内容来自当前系统 `/etc/hosts`
2. 左侧列表选择环境，右侧编辑器修改 hosts 内容
3. **保存**：将内容写入本地配置，不影响系统 hosts
4. **启用此环境**：校验格式 → 备份 → 合并写入系统 hosts → 刷新 DNS（需输入管理员密码）
5. **备份历史**：查看并恢复历史备份

## 6. 架构说明

```
UI Layer (egui)
    ↓
Service Layer (EnvManager / HostsValidator / HostsMerger)
    ↓
Platform Layer (HostsPlatform trait → macOS 实现)
    ↓
Storage Layer (ConfigStore → ~/.hostswitch/)
```

## 7. 许可证

MIT
