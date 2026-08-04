# hostswitch

macOS / Windows hosts 文件管理与切换工具，纯 Rust + egui（glow 后端）实现，无 WebView 依赖。

## 核心功能

- 多套 hosts 配置（环境）管理，勾选即启用，支持多选叠加
- 内置 Current Machine（local）环境恒为兜底，不可删除，但可编辑
- 合并策略：Current Machine 内容在前，各激活环境内容依次拼接，重复域名后面覆盖前面
- 写入前格式校验（错误阻断、警告放行）
- 写入后自动刷新 DNS 缓存
- 写 hosts / 刷 DNS 在后台线程执行，不卡界面，失败自动回滚激活状态
- 按需提权：macOS 授权框、Windows UAC，会话内授权一次后复用
- 主题跟随系统，支持 Auto / Dark / Light 循环切换
- 内置 Console 日志窗口
- 快捷键 Cmd+S（Windows 为 Ctrl+S）保存

## 环境要求

- macOS 或 Windows
- Rust 1.85+

## 构建与运行

```bash
# 编译
cargo build --release

# 运行
cargo run --release
```

### macOS 打包

```bash
./build_macos.sh            # 构建 hostswitch.app（ad-hoc 签名，本机使用）
./build_macos.sh dmg        # 构建 .app 并打包成 hostswitch.dmg
./build_macos.sh clean      # 清理打包产物
```

产物位于 `dist/`。公开分发需使用 Developer ID 证书 + notarytool 公证。

### Windows 打包

macOS 上交叉编译 Windows 版本：

```bash
# 一次性安装依赖
brew install llvm
rustup target add x86_64-pc-windows-msvc
cargo install --locked cargo-xwin

./build_windows.sh          # 交叉编译出单 exe
./build_windows.sh sign     # 编译后用 osslsigncode 签名（需 cert/hostswitch.pfx）
./build_windows.sh clean    # 清理打包产物
```

产物位于 `dist/`。签名需要 Authenticode 证书。

## 使用说明

1. 首次启动自动创建 Current Machine（local）环境，内容来自当前系统 hosts 文件
2. 左侧列表选择环境，右侧编辑器修改 hosts 内容；内置环境不可删除
3. 保存（按钮或 Cmd+S / Ctrl+S）：普通环境只写本地配置；Current Machine 保存后立即生效
4. 启用：勾选环境复选框，写入系统 hosts 并刷新 DNS；支持多选叠加，取消勾选即停用
5. 顶栏可开关 Console 日志窗口、循环切换 Auto / Dark / Light 主题
6. 底部状态栏显示操作结果与未保存（Unsaved）标记

## 配置目录

用户配置保存在 `~/.hostswitch/`：

```
~/.hostswitch/
├── state.json          # 全局状态（选中环境、激活环境列表）
└── envs/               # 各环境 hosts 内容
    ├── local           # 内置 Current Machine 环境
    └── dev             # 用户自建环境
```

## 技术栈

Rust、egui / eframe（glow 后端，内存占用小）、serde、tracing、security-framework-sys（macOS）、windows-sys（Windows）

## License

MIT
