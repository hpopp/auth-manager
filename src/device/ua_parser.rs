use woothee::parser::Parser;

use crate::storage::models::{DeviceInfo, DeviceKind};

/// Parse a User-Agent string and extract device information
pub fn parse_user_agent(user_agent: &str) -> DeviceInfo {
    let parser = Parser::new();

    match parser.parse(user_agent) {
        Some(result) => {
            let kind = match result.category {
                "pc" => DeviceKind::Desktop,
                "smartphone" => DeviceKind::Mobile,
                "mobilephone" => DeviceKind::Mobile,
                "tablet" => DeviceKind::Tablet,
                "crawler" => DeviceKind::Bot,
                _ => DeviceKind::Unknown,
            };

            DeviceInfo {
                kind,
                raw_user_agent: user_agent.to_string(),
                os: normalize_field(result.os),
                os_version: normalize_field(&result.os_version),
                browser: normalize_field(result.name),
                browser_version: normalize_field(result.version),
            }
        }
        None => DeviceInfo {
            kind: DeviceKind::Unknown,
            raw_user_agent: user_agent.to_string(),
            os: None,
            os_version: None,
            browser: None,
            browser_version: None,
        },
    }
}

/// Normalize a field value - return None if empty or "UNKNOWN"
fn normalize_field(value: &str) -> Option<String> {
    if value.is_empty() || value == "UNKNOWN" {
        None
    } else {
        Some(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_chrome_windows() {
        let ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
        let info = parse_user_agent(ua);

        assert_eq!(info.kind, DeviceKind::Desktop);
        assert_eq!(info.browser.as_deref(), Some("Chrome"));
        assert!(info.browser_version.is_some());
        assert_eq!(info.os.as_deref(), Some("Windows 10"));
    }

    #[test]
    fn test_parse_safari_ios() {
        let ua = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";
        let info = parse_user_agent(ua);

        assert_eq!(info.kind, DeviceKind::Mobile);
    }

    #[test]
    fn test_parse_bot() {
        let ua = "Googlebot/2.1 (+http://www.google.com/bot.html)";
        let info = parse_user_agent(ua);

        assert_eq!(info.kind, DeviceKind::Bot);
    }

    #[test]
    fn test_parse_unknown() {
        let ua = "SomeUnknownClient/1.0";
        let info = parse_user_agent(ua);

        assert_eq!(info.raw_user_agent, ua);
    }

    #[test]
    fn test_parse_empty() {
        let ua = "";
        let info = parse_user_agent(ua);

        assert_eq!(info.kind, DeviceKind::Unknown);
        assert!(info.os.is_none());
        assert!(info.os_version.is_none());
        assert!(info.browser.is_none());
        assert!(info.browser_version.is_none());
    }
}
