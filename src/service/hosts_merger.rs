/// hosts 文件合并器
///
/// 合并规则：
/// - 最终系统 hosts = 内置段垫底 + 各激活环境依次追加
/// - 内置段放最前面，环境条目在后面，重复域名时后面的覆盖前面的
/// - 多个环境按传入顺序合并，靠后的覆盖靠前的
pub struct HostsMerger;

/// 内置环境（系统默认）段标记
const LOCAL_MARKER: &str = "# ===== hostswitch: system default =====";
/// 旧版不带环境名的段标记，仅用于解析历史文件
const LEGACY_ENV_START_MARKER: &str = "# ===== hostswitch: env section start =====";
/// 环境段标记的前缀与后缀：`# ===== hostswitch: env <id> start =====`
const ENV_MARKER_PREFIX: &str = "# ===== hostswitch: env ";
const ENV_START_SUFFIX: &str = " start =====";
const ENV_END_SUFFIX: &str = " end =====";

impl HostsMerger {
    /// 合并多个环境内容和内置环境内容
    ///
    /// - envs：激活环境的 (id, 正文)，按列表顺序，后面的覆盖前面的
    /// - local_content：Some 时作为垫底放最前面，None 时不写内置段
    pub fn merge_many(envs: &[(&str, &str)], local_content: Option<&str>) -> String {
        let mut result = String::new();

        // 内置段垫底，放最前面
        if let Some(local) = local_content {
            result.push_str(LOCAL_MARKER);
            result.push('\n');
            result.push_str(local.trim_end());
            result.push_str("\n\n");
        }

        // 各环境依次追加，后面的覆盖前面的；每段注释里带上环境 id
        for (env_id, env_content) in envs {
            result.push_str(ENV_MARKER_PREFIX);
            result.push_str(env_id);
            result.push_str(ENV_START_SUFFIX);
            result.push('\n');
            result.push_str(env_content.trim_end());
            result.push('\n');
            result.push_str(ENV_MARKER_PREFIX);
            result.push_str(env_id);
            result.push_str(ENV_END_SUFFIX);
            result.push_str("\n\n");
        }

        result
    }

    /// 从系统 hosts 内容中提取内置段（到第一个环境标记或结尾为止）
    ///
    /// 兼容旧版写下的不带环境名标记与中文标记
    pub fn extract_local_content(system_content: &str) -> Option<String> {
        let (marker, marker_str) = find_marker(system_content, LOCAL_MARKER)?;

        // 提取标记后的内容
        let after_marker = &system_content[marker + marker_str.len()..];
        let local_part = after_marker.strip_prefix('\n').unwrap_or(after_marker);

        // 截断到第一个环境标记
        let end = find_env_section_start(local_part)
            .map(|(position, _)| position)
            .unwrap_or(local_part.len());
        Some(local_part[..end].trim_end().to_string())
    }
}

/// 旧版本（中文标记时期）的标记，用于解析历史写入的 hosts 文件
const OLD_ENV_START_MARKER: &str = "# ===== hostswitch: 环境配置开始 =====";
const OLD_LOCAL_MARKER: &str = "# ===== hostswitch: 系统默认(local) =====";

/// 找标记，新标记找不到时回退旧中文标记；返回位置和实际匹配的标记
fn find_marker<'a>(content: &'a str, marker: &'a str) -> Option<(usize, &'a str)> {
    if let Some(pos) = content.find(marker) {
        return Some((pos, marker));
    }
    if marker == LOCAL_MARKER {
        return content
            .find(OLD_LOCAL_MARKER)
            .map(|pos| (pos, OLD_LOCAL_MARKER));
    }
    None
}

/// 找第一个环境段起始标记：兼容带环境名的新标记、旧英文标记与中文标记
fn find_env_section_start(content: &str) -> Option<(usize, usize)> {
    let mut offset = 0;
    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let is_env_start =
            trimmed.starts_with(ENV_MARKER_PREFIX) && trimmed.ends_with(ENV_START_SUFFIX);
        if is_env_start || trimmed == LEGACY_ENV_START_MARKER || trimmed == OLD_ENV_START_MARKER {
            return Some((offset, line.len()));
        }
        offset += line.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_multi_env() {
        let env1 = "192.168.1.100 dev.example.com";
        let env2 = "10.0.0.1 prod.example.com";
        let local = "127.0.0.1 localhost\n::1 localhost";

        let merged = HostsMerger::merge_many(&[("dev", env1), ("prod", env2)], Some(local));

        assert!(merged.contains("# ===== hostswitch: env dev start ====="));
        assert!(merged.contains("# ===== hostswitch: env prod end ====="));
        assert!(merged.contains(LOCAL_MARKER));
        assert!(merged.contains(env1));
        assert!(merged.contains(env2));
        assert!(merged.contains(local));

        // 内置段在最前面，env1 在 env2 前面
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

        let merged = HostsMerger::merge_many(&[("dev", env1), ("prod", env2)], Some(local));

        let first = merged.find("1.1.1.1 api.example.com").unwrap();
        let second = merged.find("2.2.2.2 api.example.com").unwrap();
        assert!(first < second, "env1 should be before env2");
    }

    #[test]
    fn test_merge_empty_envs() {
        let local = "127.0.0.1 localhost";

        let merged = HostsMerger::merge_many(&[], Some(local));

        // 只有内置段时带标记写入，方便之后提取
        assert!(merged.contains(LOCAL_MARKER));
        assert!(merged.contains(local));
        assert!(!merged.contains(ENV_MARKER_PREFIX));
    }

    #[test]
    fn test_merge_no_local_no_envs() {
        // 内置段不激活且没有环境，结果为空（此时不会调用写入）
        assert_eq!(HostsMerger::merge_many(&[], None), "");
    }

    #[test]
    fn test_extract_local_content() {
        // 新格式：内置段在最前面，后面跟着带环境名的段
        let env1 = "192.168.1.100 dev.example.com";
        let local = "127.0.0.1 localhost";
        let merged = HostsMerger::merge_many(&[("dev", env1)], Some(local));

        let extracted = HostsMerger::extract_local_content(&merged).unwrap();
        assert_eq!(extracted, local);
    }

    #[test]
    fn test_extract_local_content_legacy_en_marker() {
        // 旧英文标记：环境段没有环境名，也能定位内置段
        let local = "127.0.0.1 localhost";
        let legacy = format!(
            "{}\n{}\n\n{}\n10.0.0.1 old.example.com\n{}",
            LOCAL_MARKER,
            local,
            LEGACY_ENV_START_MARKER,
            "# ===== hostswitch: env section end ====="
        );

        let extracted = HostsMerger::extract_local_content(&legacy).unwrap();
        assert_eq!(extracted, local);
    }

    #[test]
    fn test_extract_local_content_legacy() {
        // 旧中文标记：内置段在最后面（老版本写入的），也能提取
        let local = "127.0.0.1 localhost";
        let legacy = format!(
            "{}\n192.168.1.100 dev.example.com\n{}\n{}\n{}",
            OLD_ENV_START_MARKER, "# ===== hostswitch: 环境配置结束 =====", OLD_LOCAL_MARKER, local
        );

        let extracted = HostsMerger::extract_local_content(&legacy).unwrap();
        assert_eq!(extracted, local);
    }
}
