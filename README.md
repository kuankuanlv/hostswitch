# hostswitch

macOS hosts 文件管理与切换工具，纯 Rust + egui 实现，无 WebView 依赖。

## 1. 项目介绍

hostswitch 帮助开发者管理多套 hosts 配置方案（环境），支持一键启用与停用。每个环境对应一份独立的 hosts 文本；内置的 Current Machine（local）环境恒为兜底，启用时系统 hosts 由 Current Machine 内容打底、各激活环境内容依次拼接而成。

**核心特性：**

- 多套环境管理，勾选即启用，支持多选叠加（各环境内容合并写入）
- 内置 Current Machine（local）环境恒为兜底，不可删除、不可取消，但可编辑
- 合并策略：Current Machine 内容在最前，各激活环境依次在后，重复域名时后面的覆盖前面的（环境配置优先生效）
- 写入前格式校验（错误阻断、警告放行）
- 写入后自动刷新 DNS 缓存
- 写 hosts / 刷 DNS 在后台线程执行，不卡界面；失败自动回滚激活状态
- macOS 按需提权（Authorization Services 授权框，授权一次进程内复用），平时以普通权限运行
- 主题跟随系统，支持 Auto / Dark / Light 循环切换
- 内置 Console 日志窗口，实时查看应用日志

**技术栈：** Rust、egui/eframe（glow 后端，内存占用小）、serde/chrono、tracing、security-framework-sys

**配置目录：** `~/.hostswitch/`

## 2. 功能状态

### 已实现

- egui 主界面（左侧环境列表 + 右侧文本编辑器 + 顶栏 + 底部状态栏）
- 环境增删（local 内置环境不可删）
- 环境启用 / 停用（勾选开关，支持多选叠加，合并写入系统 hosts）
- 配置文件持久化（独立 env 文件 + state.json，兼容旧版单激活配置迁移）
- 格式校验（错误阻断启用，警告提示不阻断）
- DNS 缓存自动刷新
- macOS 平台支持（其他平台编译直接报错）
- 三模式主题（跟随系统 / Dark / Light）
- Console 日志窗口（tracing 环形缓冲 500 条）
- 快捷键 Cmd+S 保存；Current Machine 保存后立即写系统 hosts

### 待实现

**v0.2 规划**

- 语法高亮（IP / 域名 / 注释着色）
- 系统托盘常驻
- 快捷键切换环境
- 环境重命名 UI（服务层已实现，UI 未接入）

**v1.0 规划**

- Windows / Linux 平台支持（需新增 platform 实现）
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
└── src/
    ├── main.rs             # 入口，初始化 tracing，启动 egui 窗口
    ├── app.rs              # 主应用结构，串联各模块，后台写入调度
    ├── theme.rs            # 主题：Dark/Light 双套视觉 + 配色常量
    ├── logs.rs             # 日志：tracing Layer + 500 条环形缓冲（Console 数据源）
    ├── model/              # 数据结构
    │   ├── env.rs          # Env 结构体 + GLOBAL_ENV_ID 常量
    │   └── app_state.rs    # AppState（激活列表、兼容旧版迁移）
    ├── storage/            # 存储层
    │   └── config_store.rs # 配置文件读写
    ├── platform/           # 平台抽象层
    │   ├── traits.rs       # HostsPlatform trait
    │   ├── macos.rs        # macOS 实现（Authorization Services 提权）
    │   └── factory.rs      # 平台工厂，非 macOS 编译报错
    ├── service/            # 业务逻辑层
    │   ├── env_manager.rs  # 环境管理核心逻辑（激活/停用/合并/回滚）
    │   ├── hosts_validator.rs  # hosts 格式校验
    │   └── hosts_merger.rs     # hosts 合并（兼容旧中文标记）
    └── ui/                 # UI 层（纯渲染，返回 Action 由 app 执行）
        ├── sidebar.rs      # 左侧环境列表（含 + 新建按钮）
        ├── editor.rs       # 右侧文本编辑器
        └── new_env_dialog.rs   # 新增环境弹窗
```

**用户配置目录：**

```
~/.hostswitch/
├── state.json          # 全局状态（选中环境、激活环境列表）
└── envs/               # 各环境 hosts 内容
    ├── local           # 内置 Current Machine 环境
    └── dev             # 用户自建环境
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

**打包 .app / dmg：**

```bash
./build_macos.sh        # 构建 hostswitch.app（ad-hoc 签名，本机使用）
./build_macos.sh dmg    # 构建 .app 并打包成 hostswitch.dmg
./build_macos.sh clean  # 清理打包产物
```

- 产物在 `target/release/` 下；本机使用 ad-hoc 签名即可
- 公开分发需要 Developer ID 证书 + notarytool 公证（具体命令见 `build_macos.sh` 头部注释）
- 调试日志：终端 `RUST_LOG=debug ./target/release/hostswitch.app/Contents/MacOS/hostswitch` 可开启详细日志

## 5. 使用说明

1. 首次启动会自动创建 Current Machine（local）环境，内容来自当前系统 `/etc/hosts`
2. 左侧列表选择环境，右侧编辑器修改 hosts 内容；内置环境显示为 "Current Machine"，不可删除
3. **保存**（按钮或 Cmd+S）：普通环境只写入本地配置；Current Machine 保存后立即合并写入系统 hosts
4. **启用**：勾选环境旁的绿色复选框，校验格式 → 合并 → 写入系统 hosts → 刷新 DNS（首次需输入管理员密码，之后授权复用不再弹框）；多选可叠加启用，取消勾选即停用
5. 顶栏右侧可开关 **Console** 日志窗口、循环切换 **Auto / Dark / Light** 主题
6. 底部状态栏显示最近操作结果与未保存（Unsaved）标记

## 6. 架构说明

```
UI Layer (egui) —— 纯渲染，返回 Action
    ↓
Service Layer (EnvManager / HostsValidator / HostsMerger)
    ↓
Platform Layer (HostsPlatform trait → macOS 实现)
    ↓
Storage Layer (ConfigStore → ~/.hostswitch/)
```

- UI 层不直接碰应用状态：sidebar / editor / dialog 渲染后返回动作，由 app.rs 统一执行
- 写系统 hosts / 刷新 DNS 在后台线程执行，主线程每帧轮询结果，失败回滚激活状态
- 依赖单向：main → app → service/ui/theme/logs；叶子为 model / platform / theme / logs
- 环境排序：Current Machine 恒在最前，其余按创建时间（新增的放最后）

## 7. 许可证

MIT
