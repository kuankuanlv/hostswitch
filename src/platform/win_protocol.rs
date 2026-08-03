//! Windows 提权助手的纯逻辑层：协议编解码、参数解析、路径白名单。
//! 不依赖任何 Windows API，可在任意平台编译运行（便于本机单测）。

use std::path::Path;

/// 客户端 → 助手的命令
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelperCommand {
    /// 将临时文件内容应用到系统 hosts（参数为临时文件路径）
    Apply(String),
    /// 刷新 DNS 缓存
    FlushDns,
    /// 通知助手退出
    Bye,
}

/// 助手 → 客户端的响应
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelperResponse {
    Ok,
    Err(String),
}

/// Windows 系统 hosts 精确路径
pub const WINDOWS_HOSTS_PATH: &str = r"C:\Windows\System32\drivers\etc\hosts";

/// 编码命令：u32 LE 长度前缀 + UTF-8 文本
pub fn encode_command(cmd: &HelperCommand) -> Vec<u8> {
    let text = match cmd {
        HelperCommand::Apply(path) => format!("APPLY {}\n", path),
        HelperCommand::FlushDns => "FLUSH_DNS\n".to_string(),
        HelperCommand::Bye => "BYE\n".to_string(),
    };
    let mut out = Vec::with_capacity(4 + text.len());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(text.as_bytes());
    out
}

/// 编码响应：u32 LE 长度前缀 + UTF-8 文本
pub fn encode_response(resp: &HelperResponse) -> Vec<u8> {
    let text = match resp {
        HelperResponse::Ok => "OK\n".to_string(),
        HelperResponse::Err(msg) => format!("ERR {}\n", msg),
    };
    let mut out = Vec::with_capacity(4 + text.len());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(text.as_bytes());
    out
}

/// 解码收到的命令消息（完整消息，含长度前缀）
pub fn decode_command(bytes: &[u8]) -> Option<HelperCommand> {
    let text = decode_text(bytes)?;
    if let Some(rest) = text.strip_prefix("APPLY ") {
        return Some(HelperCommand::Apply(rest.trim_end().to_string()));
    }
    match text.trim_end() {
        "FLUSH_DNS" => Some(HelperCommand::FlushDns),
        "BYE" => Some(HelperCommand::Bye),
        _ => None,
    }
}

/// 解码收到的响应消息（完整消息，含长度前缀）
pub fn decode_response(bytes: &[u8]) -> Option<HelperResponse> {
    let text = decode_text(bytes)?;
    if text.trim_end() == "OK" {
        return Some(HelperResponse::Ok);
    }
    if let Some(rest) = text.strip_prefix("ERR ") {
        return Some(HelperResponse::Err(rest.trim_end().to_string()));
    }
    None
}

fn decode_text(bytes: &[u8]) -> Option<&str> {
    if bytes.len() < 4 {
        return None;
    }
    let len = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    if bytes.len() != 4 + len {
        return None;
    }
    std::str::from_utf8(&bytes[4..]).ok()
}

/// 解析 helper 启动参数：`--helper --parent-pid <pid>`
/// args 为 std::env::args() 的完整收集（含程序名，自动跳过）
pub fn parse_helper_args(args: &[String]) -> Option<u32> {
    let mut iter = args.iter().skip(1);
    if iter.next().map(String::as_str) != Some("--helper") {
        return None;
    }
    if iter.next().map(String::as_str) != Some("--parent-pid") {
        return None;
    }
    iter.next()?.parse().ok()
}

/// 规范化路径用于比较：统一分隔符、去尾分隔符、小写（Windows 路径语义）
fn lex_normalize(p: &Path) -> String {
    p.to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_lowercase()
}

/// 两个路径是否等价：优先 canonicalize 精确比较，失败时词法比较
pub fn is_same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => lex_normalize(a) == lex_normalize(b),
    }
}

/// 路径白名单：仅允许系统 hosts 精确路径（helper 复制目标校验）
pub fn is_allowed_hosts_path(p: &Path) -> bool {
    is_same_path(p, Path::new(WINDOWS_HOSTS_PATH))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_roundtrip() {
        let cmd = HelperCommand::Apply(
            r"C:\Users\test\AppData\Local\Temp\hostswitch_123.tmp".to_string(),
        );
        let bytes = encode_command(&cmd);
        assert_eq!(decode_command(&bytes), Some(cmd));
    }

    #[test]
    fn test_flush_dns_roundtrip() {
        let bytes = encode_command(&HelperCommand::FlushDns);
        assert_eq!(decode_command(&bytes), Some(HelperCommand::FlushDns));
    }

    #[test]
    fn test_bye_roundtrip() {
        let bytes = encode_command(&HelperCommand::Bye);
        assert_eq!(decode_command(&bytes), Some(HelperCommand::Bye));
    }

    #[test]
    fn test_response_ok_roundtrip() {
        let bytes = encode_response(&HelperResponse::Ok);
        assert_eq!(decode_response(&bytes), Some(HelperResponse::Ok));
    }

    #[test]
    fn test_response_err_roundtrip() {
        let resp = HelperResponse::Err("write failed: access denied".to_string());
        let bytes = encode_response(&resp);
        assert_eq!(decode_response(&bytes), Some(resp));
    }

    #[test]
    fn test_decode_rejects_garbage() {
        assert_eq!(decode_command(b"garbage"), None);
        // 长度前缀为 0 的空消息应解码为 None
        assert_eq!(decode_command(&[0u8, 0, 0, 0]), None);
        assert_eq!(decode_response(b"\x02\x00\x00\x00hi"), None);
        assert_eq!(decode_command(&[0x01, 0, 0, 0, b'X']), None); // 未知命令
    }

    #[test]
    fn test_decode_rejects_wrong_length() {
        // 长度前缀与实际数据不符
        assert_eq!(decode_command(&[0x05, 0, 0, 0, b'B', b'Y', b'E']), None);
    }

    #[test]
    fn test_parse_helper_args_valid() {
        let args = vec![
            "hostswitch.exe".to_string(),
            "--helper".to_string(),
            "--parent-pid".to_string(),
            "4242".to_string(),
        ];
        assert_eq!(parse_helper_args(&args), Some(4242));
    }

    #[test]
    fn test_parse_helper_args_missing_flag() {
        let args = vec!["hostswitch.exe".to_string(), "--other".to_string()];
        assert_eq!(parse_helper_args(&args), None);
    }

    #[test]
    fn test_parse_helper_args_missing_pid() {
        let args = vec![
            "hostswitch.exe".to_string(),
            "--helper".to_string(),
            "--parent-pid".to_string(),
        ];
        assert_eq!(parse_helper_args(&args), None);
    }

    #[test]
    fn test_parse_helper_args_bad_pid() {
        let args = vec![
            "hostswitch.exe".to_string(),
            "--helper".to_string(),
            "--parent-pid".to_string(),
            "abc".to_string(),
        ];
        assert_eq!(parse_helper_args(&args), None);
    }

    #[test]
    fn test_parse_helper_args_extra_args() {
        let args = vec![
            "hostswitch.exe".to_string(),
            "--helper".to_string(),
            "--parent-pid".to_string(),
            "7".to_string(),
            "extra".to_string(),
        ];
        assert_eq!(parse_helper_args(&args), Some(7));
    }

    #[test]
    fn test_whitelist_accepts_exact_path() {
        // 精确路径（macOS 上目标不存在 → canonicalize 失败 → 词法比较）
        assert!(is_allowed_hosts_path(Path::new(WINDOWS_HOSTS_PATH)));
        // Windows 路径大小写不敏感
        assert!(is_allowed_hosts_path(Path::new(r"c:\windows\system32\drivers\etc\HOSTS")));
        // 尾部分隔符差异（末尾反斜杠：raw string 中 \ 不转义，`\"` 即闭合）
        assert!(is_allowed_hosts_path(Path::new(r"C:\Windows\System32\drivers\etc\hosts\")));
    }

    #[test]
    fn test_whitelist_rejects_other_path() {
        assert!(!is_allowed_hosts_path(Path::new(r"C:\Windows\System32\drivers\etc\hosts2")));
        assert!(!is_allowed_hosts_path(Path::new("/etc/hosts")));
        assert!(!is_allowed_hosts_path(Path::new(r"C:\Windows\System32\drivers\etc")));
    }

    #[test]
    fn test_same_path_canonicalize_matches() {
        // 两边都存在时用 canonicalize 比较（tempdir 内）
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("hosts");
        std::fs::write(&a, b"x").unwrap();
        let b = dir.path().join("hosts");
        assert!(is_same_path(&a, &b));

        let other = dir.path().join("hosts2");
        std::fs::write(&other, b"y").unwrap();
        assert!(!is_same_path(&a, &other));
    }
}
