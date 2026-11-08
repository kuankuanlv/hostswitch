/// hosts 文件合并器
///
/// 合并规则：
/// - 最终系统 hosts = local 垫底 + 各激活 env 依次追加
/// - local 放最前面 → 环境条目在后面，重复域名时后面的覆盖前面的
/// - 多个 env 按传入顺序合并，靠后的覆盖靠前的
pub struct HostsMerger;

const ENV_START_MARKER: &str = "# ===== hostswitch: env section start =====";
const ENV_END_MARKER: &str = "# ===== hostswitch: env section end =====";
const LOCAL_MARKER: &str = "# ===== hostswitch: system default =====";

impl HostsMerger {
    /// 合并多个 env 内容和 local 内容
    ///
    /// - env_contents：激活的 env 内容，按列表顺序，后面的覆盖前面的
    /// - local_content：Some 时作为垫底放最前面，None 时不写 local 段
    pub fn merge_many(env_contents: &[&str], local_content: Option<&str>) -> String {
        let mut result = String::new();

        // local 垫底，放最前面
        if let Some(local) = local_content {
            result.push_str(LOCAL_MARKER);
            result.push('\n');
            result.push_str(local.trim_end());
            result.push_str("\n\n");
        }

        // 各 env 依次追加，后面的覆盖前面的
        for env_content in env_contents {
            result.push_str(ENV_START_MARKER);
            result.push('\n');
            result.push_str(env_content.trim_end());
            result.push('\n');
            result.push_str(ENV_END_MARKER);
            result.push_str("\n\n");
        }

        result
    }

    /// 从系统 hosts 内容中提取 local 部分（到下一个 env 标记或结尾为止）
    pub fn extract_local_content(system_content: &str) -> Option<String> {
        let (marker, marker_str) = find_marker(system_content, LOCAL_MARKER)?;

        // 提取标记后的内容
        let after_marker = &system_content[marker + marker_str.len()..];
        let local_part = after_marker.strip_prefix('\n').unwrap_or(after_marker);

        // 截断到第一个 env 标记（新格式 local 在最前面）
        let end = local_part
            .find(ENV_START_MARKER)
            .or_else(|| local_part.find(OLD_ENV_START_MARKER))
            .unwrap_or(local_part.len());
        Some(local_part[..end].trim_end().to_string())
    }
}

/// 旧版本（中文标记时期）的标记，用于解析历史写入的 hosts 文件
const OLD_ENV_START_MARKER: &str = "# ===== hostswitch: 环境配置开始 =====";
const OLD_ENV_END_MARKER: &str = "# ===== hostswitch: 环境配置结束 =====";
const OLD_LOCAL_MARKER: &str = "# ===== hostswitch: 系统默认(local) =====";

/// 找标记，新标记找不到时回退旧中文标记；返回位置和实际匹配的标记
fn find_marker<'a>(content: &'a str, marker: &'a str) -> Option<(usize, &'a str)> {
    if let Some(pos) = content.find(marker) {
        return Some((pos, marker));
    }
    let old = match marker {
        ENV_START_MARKER => OLD_ENV_START_MARKER,
        ENV_END_MARKER => OLD_ENV_END_MARKER,
        LOCAL_MARKER => OLD_LOCAL_MARKER,
        _ => return None,
    };
    content.find(old).map(|pos| (pos, old))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_multi_env() {
        let env1 = "192.168.1.100 dev.example.com";
        let env2 = "10.0.0.1 prod.example.com";
        let local = "127.0.0.1 localhost\n::1 localhost";

        let merged = HostsMerger::merge_many(&[env1, env2], Some(local));

        assert!(merged.contains(ENV_START_MARKER));
        assert!(merged.contains(LOCAL_MARKER));
        assert!(merged.contains(env1));
        assert!(merged.contains(env2));
        assert!(merged.contains(local));

        // local 在最前面，env1 在 env2 前面
        let local_pos = merged.find(local).unwrap();
        let env1_pos = merged.find(env1).unwrap();
        let env2_pos = merged.find(env2).unwrap();
        assert!(local_pos < env1_pos);
        assert!(env1_pos < env2_pos);
    }

    #[test]
    fn test_merge_multi_repeat_domain() {
        // 重复域名：后面的覆盖前面的（保留所有行，顺序即优先级）
        let env1 = "1.1.1.1 api.example.com";
        let env2 = "2.2.2.2 api.example.com";
        let local = "127.0.0.1 localhost";

        let merged = HostsMerger::merge_many(&[env1, env2], Some(local));

        let first = merged.find("1.1.1.1 api.example.com").unwrap();
        let second = merged.find("2.2.2.2 api.example.com").unwrap();
        assert!(first < second, "env1 should be before env2");
    }

    #[test]
    fn test_merge_empty_envs() {
        let local = "127.0.0.1 localhost";

        let merged = HostsMerger::merge_many(&[], Some(local));

        // 只有 local 时带标记写入，方便之后提取
        assert!(merged.contains(LOCAL_MARKER));
        assert!(merged.contains(local));
        assert!(!merged.contains(ENV_START_MARKER));
    }

    #[test]
    fn test_merge_no_local_no_envs() {
        // local 不激活且没有 env，结果为空（此时不会调用写入）
        assert_eq!(HostsMerger::merge_many(&[], None), "");
    }

    #[test]
    fn test_extract_local_content() {
        // 新格式：local 在最前面，后面跟着 env 段
        let env1 = "192.168.1.100 dev.example.com";
        let local = "127.0.0.1 localhost";
        let merged = HostsMerger::merge_many(&[env1], Some(local));

        let extracted = HostsMerger::extract_local_content(&merged).unwrap();
        assert_eq!(extracted, local);
    }

    #[test]
    fn test_extract_local_content_legacy() {
        // 旧格式：local 在最后面（老版本写入的），也能提取
        let local = "127.0.0.1 localhost";
        let legacy = format!(
            "{} \n192.168.1.100 dev.example.com\n{}\n{}\n",
            ENV_START_MARKER, ENV_END_MARKER, LOCAL_MARKER
        );
        let legacy = format!("{}{}\n", legacy, local);

        let extracted = HostsMerger::extract_local_content(&legacy).unwrap();
        assert_eq!(extracted, local);
    }
}
