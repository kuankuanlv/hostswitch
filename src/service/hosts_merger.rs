/// hosts 文件合并器
///
/// 合并规则：
/// - 最终系统 hosts = 当前 env 的内容 + local 的内容
/// - env 的条目放前面 → 相同域名时 env 的 IP 先生效
/// - local 的条目放后面 → 兜底
pub struct HostsMerger;

const ENV_START_MARKER: &str = "# ===== hostswitch: 环境配置开始 =====";
const ENV_END_MARKER: &str = "# ===== hostswitch: 环境配置结束 =====";
const LOCAL_MARKER: &str = "# ===== hostswitch: 系统默认(local) =====";

impl HostsMerger {
    /// 合并 env 内容和 local 内容
    ///
    /// 如果 env_id 是 "local"，直接返回 local 内容（不添加分隔标记）
    pub fn merge(env_id: &str, env_content: &str, local_content: &str) -> String {
        if env_id == "local" {
            return local_content.to_string();
        }

        let mut result = String::new();

        // env 部分
        result.push_str(ENV_START_MARKER);
        result.push('\n');
        result.push_str(env_content.trim_end());
        result.push('\n');
        result.push_str(ENV_END_MARKER);
        result.push_str("\n\n");

        // local 部分
        result.push_str(LOCAL_MARKER);
        result.push('\n');
        result.push_str(local_content.trim_end());
        result.push('\n');

        result
    }

    /// 从系统 hosts 内容中提取 env 部分（去掉标记和 local 部分）
    /// 用于从系统 hosts 恢复 env 内容
    #[allow(dead_code)]
    pub fn extract_env_content(system_content: &str) -> Option<String> {
        let start = system_content.find(ENV_START_MARKER)?;
        let end = system_content.find(ENV_END_MARKER)?;

        if start >= end {
            return None;
        }

        let after_start = &system_content[start + ENV_START_MARKER.len()..];
        let env_part = after_start.strip_prefix('\n').unwrap_or(after_start);
        let end_in_part = env_part.find(ENV_END_MARKER)?;

        let env_content = &env_part[..end_in_part];
        Some(env_content.trim_end().to_string())
    }

    /// 从系统 hosts 内容中提取 local 部分
    pub fn extract_local_content(system_content: &str) -> Option<String> {
        let marker = system_content.find(LOCAL_MARKER)?;

        // 提取标记后的内容
        let after_marker = &system_content[marker + LOCAL_MARKER.len()..];
        let local_part = after_marker.strip_prefix('\n').unwrap_or(after_marker);

        Some(local_part.trim_end().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_non_local() {
        let env_content = "192.168.1.100 dev.example.com";
        let local_content = "127.0.0.1 localhost\n::1 localhost";

        let merged = HostsMerger::merge("dev", env_content, local_content);

        assert!(merged.contains(ENV_START_MARKER));
        assert!(merged.contains(ENV_END_MARKER));
        assert!(merged.contains(LOCAL_MARKER));
        assert!(merged.contains(env_content));
        assert!(merged.contains(local_content));

        // env 内容应该在 local 内容前面
        let env_pos = merged.find(env_content).unwrap();
        let local_pos = merged.find(local_content).unwrap();
        assert!(env_pos < local_pos);
    }

    #[test]
    fn test_merge_local() {
        let local_content = "127.0.0.1 localhost\n::1 localhost";

        let merged = HostsMerger::merge("local", "", local_content);

        // 激活 local 时，不应该有分隔标记
        assert!(!merged.contains(ENV_START_MARKER));
        assert!(!merged.contains(LOCAL_MARKER));
        assert_eq!(merged, local_content);
    }

    #[test]
    fn test_extract_env_content() {
        let env_content = "192.168.1.100 dev.example.com";
        let local_content = "127.0.0.1 localhost";
        let merged = HostsMerger::merge("dev", env_content, local_content);

        let extracted = HostsMerger::extract_env_content(&merged).unwrap();
        assert_eq!(extracted, env_content);
    }

    #[test]
    fn test_extract_local_content() {
        let env_content = "192.168.1.100 dev.example.com";
        let local_content = "127.0.0.1 localhost";
        let merged = HostsMerger::merge("dev", env_content, local_content);

        let extracted = HostsMerger::extract_local_content(&merged).unwrap();
        assert_eq!(extracted, local_content);
    }
}
