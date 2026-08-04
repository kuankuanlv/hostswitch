# hostswitch

macOS / Windows hosts 文件管理与切换工具，纯 Rust + egui（glow 后端）实现，无 WebView 依赖。

## 项目目录架构

```
hostswitch/
├── Cargo.toml
├── build_macos.sh           # macOS 打包脚本（.app / dmg）
├── build_windows.sh         # Windows 交叉编译脚本（cargo-xwin）
└── src/
    ├── main.rs              # 入口；Windows 下识别 --helper 提权助手模式
    ├── app.rs               # 应用主界面与状态管理
    ├── theme.rs             # 主题（Dark / Light / Auto）
    ├── logs.rs              # 应用内日志缓冲（Console 数据源）
    ├── model/               # 数据模型（Env、AppState）
    ├── storage/             # 配置持久化（config_store）
    ├── platform/            # 平台抽象层（trait 契约 + macOS/Windows 实现 + win_protocol 协议）
    ├── service/             # 核心业务（env_manager 环境管理、hosts_merger 合并、hosts_validator 校验）
    └── ui/                  # 界面组件（sidebar 侧边栏、editor 编辑器、new_env_dialog 弹窗）
```

## 配置目录

用户配置保存在 `~/.hostswitch/`：

```
~/.hostswitch/
├── state.json          # 全局状态（选中环境、激活环境列表）
└── envs/               # 各环境 hosts 内容
    ├── local           # 内置 Current Machine 环境
    └── dev             # 用户自建环境
```

## 权限机制

写入系统 hosts 需要管理员权限，按需提权、平时以普通权限运行：

- **macOS**：通过 Authorization Services（AEWP）申请授权，授权一次后进程内复用，不再弹框
- **Windows**：UAC 提权启动自身 `--helper` 提权助手子进程，经命名管道通信；会话内首次弹 UAC，后续静默复用

## 构建与打包

```bash
# 编译 / 运行
cargo build --release
cargo run --release

# macOS：构建 .app，或打包 dmg（产物在 dist/）
./build_macos.sh
./build_macos.sh dmg

# Windows：macOS 上 cargo-xwin 交叉编译单 exe（产物在 dist/）
./build_windows.sh
```

macOS 需安装 Xcode Command Line Tools；Windows 交叉编译需 `brew install llvm`、`rustup target add x86_64-pc-windows-msvc`、`cargo install cargo-xwin`。

## 环境要求

- macOS 或 Windows
- Rust 1.85+

## License

MIT
