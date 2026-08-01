use std::net::{Ipv4Addr, Ipv6Addr};

/// 校验级别
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationLevel {
    /// 错误：阻止写入
    Error,
    /// 警告：不阻止写入，但提示用户
    Warning,
}

/// 单条校验结果
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    /// 行号（从 1 开始）
    pub line_number: usize,
    /// 级别
    pub level: ValidationLevel,
    /// 错误/警告信息
    pub message: String,
    /// 该行原始内容
    pub line_content: String,
}

/// hosts 文件校验器
pub struct HostsValidator;

impl HostsValidator {
    /// 校验整个 hosts 内容，返回所有问题
    pub fn validate(content: &str) -> Vec<ValidationIssue> {
        let mut issues = Vec::new();

        for (idx, line) in content.lines().enumerate() {
            let line_number = idx + 1;
            let trimmed = line.trim();

            // 空行跳过
            if trimmed.is_empty() {
                continue;
            }

            // 注释行跳过
            if trimmed.starts_with('#') {
                continue;
            }

            // 解析行
            let parts: Vec<&str> = trimmed.split_whitespace().collect();

            if parts.len() < 2 {
                issues.push(ValidationIssue {
                    line_number,
                    level: ValidationLevel::Error,
                    message: "格式错误：每行至少需要 IP 和 域名 两列".to_string(),
                    line_content: line.to_string(),
                });
                continue;
            }

            let ip_str = parts[0];
            let domain = parts[1];

            // 校验 IP 格式
            if !Self::is_valid_ip(ip_str) {
                issues.push(ValidationIssue {
                    line_number,
                    level: ValidationLevel::Error,
                    message: format!("IP 地址格式无效: {}", ip_str),
                    line_content: line.to_string(),
                });
                continue;
            }

            // 校验域名格式（warning 级别）
            if !Self::is_valid_domain(domain) {
                issues.push(ValidationIssue {
                    line_number,
                    level: ValidationLevel::Warning,
                    message: format!("域名格式可能有问题: {}", domain),
                    line_content: line.to_string(),
                });
            }
        }

        issues
    }

    /// 检查是否有错误级别的问题
    pub fn has_errors(issues: &[ValidationIssue]) -> bool {
        issues.iter().any(|i| i.level == ValidationLevel::Error)
    }
    /// 校验 IP 地址格式（IPv4 或 IPv6）
    fn is_valid_ip(ip: &str) -> bool {
        // 尝试解析为 IPv4
        if ip.parse::<Ipv4Addr>().is_ok() {
            return true;
        }
        // 尝试解析为 IPv6
        if ip.parse::<Ipv6Addr>().is_ok() {
            return true;
        }
        false
    }

    /// 简单校验域名格式
    fn is_valid_domain(domain: &str) -> bool {
        if domain.is_empty() {
            return false;
        }

        // 不能以点开头或结尾
        if domain.starts_with('.') || domain.ends_with('.') {
            return false;
        }

        // 只能包含字母、数字、点、连字符
        let valid_chars = domain
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');

        if !valid_chars {
            return false;
        }

        // 至少有一个点（localhost 等特殊情况除外）
        if domain == "localhost" {
            return true;
        }

        // 不强制要求有点，因为可能有单标签主机名
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_hosts() {
        let content = "\
# comment
127.0.0.1 localhost
::1 localhost
192.168.1.100 example.com www.example.com
";
        let issues = HostsValidator::validate(content);
        assert!(!HostsValidator::has_errors(&issues));
    }

    #[test]
    fn test_invalid_ip() {
        let content = "999.999.999.999 example.com";
        let issues = HostsValidator::validate(content);
        assert!(HostsValidator::has_errors(&issues));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].line_number, 1);
    }

    #[test]
    fn test_missing_domain() {
        let content = "127.0.0.1";
        let issues = HostsValidator::validate(content);
        assert!(HostsValidator::has_errors(&issues));
    }

    #[test]
    fn test_empty_and_comment_lines() {
        let content = "

# just a comment
   # indented comment

";
        let issues = HostsValidator::validate(content);
        assert!(issues.is_empty());
    }

    #[test]
    fn test_ipv6() {
        let content = "::1 localhost";
        let issues = HostsValidator::validate(content);
        assert!(!HostsValidator::has_errors(&issues));
    }
}
