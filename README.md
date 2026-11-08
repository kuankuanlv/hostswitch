# hostswitch

hosts 文件管理与切换工具，纯 Rust 实现，无 WebView。支持 macOS、Windows。

## 项目目录架构

```
hostswitch/
├── Cargo.toml
├── build_macos.sh           # macOS 打包（产物在 dist/）
├── build_windows.bat        # Windows 本机 MSVC 打包（产物在 dist/）
└── src/
    ├── main.rs              # 入口：平台分流 + 日志 + ui::run（不碰 egui API）
    ├── logs.rs              # 应用内日志缓冲（Console 用）
    ├── model/               # Env、AppState
    ├── storage/             # 配置读写（~/.hostswitch）
    ├── platform/            # 系统 hosts 与提权（不含 UI 库）
    ├── service/             # 环境管理、合并、校验
    └── ui/
        ├── app.rs           # 框架无关 App + UiCommand
        └── impls/egui/      # egui 具体实现（主题、侧栏、编辑器等）
```

双端构建脚本产物目录统一为仓库下的 `dist/`：

- macOS：`dist/hostswitch.app`（可选 `dist/hostswitch.dmg`）
- Windows：`dist/hostswitch.exe`

## 配置目录

用户数据在家目录下的 `.hostswitch`（常量名见 storage）：

```
~/.hostswitch/
├── state.json          # 选中环境、激活列表等
└── envs/               # 各环境 hosts 正文
    ├── local           # 内置 Current Machine
    └── <env_id>        # 用户自建环境
```

## 行为说明

- 系统 hosts = 内置 Current Machine 正文垫底 + 当前激活环境依次追加；只有目标内容与系统现状不一致时才写盘。
- 保存激活环境或 Current Machine 的正文后自动写盘；保存未激活环境只写环境文件，不动系统 hosts。
- 启动时若系统 hosts 与激活集合不一致，只提示并等待手动 Apply，不自动覆盖。
- 写盘前会比对上次确认过的系统 hosts 内容：若被其他程序改过，不自动覆盖，底栏按钮变为 Overwrite，确认后才写。
- 写盘失败时：启停操作回滚激活状态；正文保存不回滚（编辑内容不丢），底栏报错后可重试。

## 关于权限

写系统 hosts、刷 DNS 需要管理员权限。设计是：**平时普通权限跑 UI**，真正写盘时再提权；提权后复用高权限的子进程，未设计成直接管理员权限。

### 共同点

- 读系统 hosts：一般不需要提权
- 写系统 hosts / 刷 DNS：走平台层 `ensure_elevated`，失败会反映到界面状态栏
- UI（`ui::app`）不直接碰提权细节，只调 `EnvManager` → `HostsPlatform`

### macOS

- 机制：Authorization Services，核心是已废弃但仍可用的 `AuthorizationExecuteWithPrivileges`（AEWP）
- 流程：首次需要写盘时弹出系统授权框；授权句柄缓存在进程内，**同一次启动里后续写盘不再弹框**
- 兜底：若 AEWP 在当前系统不可用，回退到 `osascript` 提权（这种路径每次都可能再弹框）
- 实现位置：`src/platform/macos.rs`

### Windows

- 机制：UAC 拉起自身的 `--helper` 子进程（管理员），主进程与 helper 用**命名管道**通信（按用户 SID 隔离）
- 流程：
  1. 主进程仍是普通权限，负责界面
  2. 首次写 hosts / flush DNS 时，`runas` 弹 UAC；用户同意后 helper 常驻监听管道
  3. 之后同会话内复用已有 helper，**不再弹 UAC**
  4. helper 空闲一段时间会自己退出，下次写盘再拉起（再弹一次 UAC）
- 护栏：helper 只接受约定协议（如 APPLY / FLUSH_DNS）；写目标固定为系统 hosts 路径，来源只接受系统临时目录下的 hostswitch 临时文件
- DNS：优先进程内 `DnsFlushResolverCache`，避免弹出 `ipconfig` 黑窗
- 实现位置：`src/platform/windows.rs`、`src/platform/win_protocol.rs`；入口分流在 `platform::take_over_entrypoint`

## 构建与打包

```bash
# 编译 / 运行
cargo build --release
cargo run --release

# macOS：.app，或再打 dmg（产物在 dist/）
./build_macos.sh
./build_macos.sh dmg

# Windows：本机 MSVC 单 exe（产物在 dist/）
build_windows.bat
```

构建结束会打印产物绝对路径，并暂停等按键（方便双击脚本时看清结果）。

macOS 需要 Xcode Command Line Tools；Windows 需要 Rust（msvc）+ VS Build Tools（含 C++ 工具链；签名可选 Windows SDK）。

## 环境要求

- macOS 或 Windows
- Rust 1.85+

## License

MIT
