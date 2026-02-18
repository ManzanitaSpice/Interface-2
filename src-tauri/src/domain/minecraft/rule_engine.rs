use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsName {
    Windows,
    Linux,
    Macos,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleContext {
    pub os_name: OsName,
    pub arch: String,
}

impl RuleContext {
    pub fn current() -> Self {
        let os_name = if cfg!(target_os = "windows") {
            OsName::Windows
        } else if cfg!(target_os = "linux") {
            OsName::Linux
        } else if cfg!(target_os = "macos") {
            OsName::Macos
        } else {
            OsName::Unknown
        };

        Self {
            os_name,
            arch: std::env::consts::ARCH.to_string(),
        }
    }
}

pub fn evaluate_rules(rules: &[Value], context: &RuleContext) -> bool {
    if rules.is_empty() {
        return true;
    }

    let mut allowed = false;
    for rule in rules {
        let action = rule
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("allow");
        if rule_matches_context(rule, context) {
            allowed = action == "allow";
        }
    }

    allowed
}

fn rule_matches_context(rule: &Value, context: &RuleContext) -> bool {
    let Some(os_rule) = rule.get("os") else {
        return true;
    };

    let Some(os_obj) = os_rule.as_object() else {
        return false;
    };

    if let Some(name) = os_obj.get("name").and_then(Value::as_str) {
        if !os_name_matches(name, context.os_name) {
            return false;
        }
    }

    if let Some(arch) = os_obj.get("arch").and_then(Value::as_str) {
        if !arch_matches(arch, &context.arch) {
            return false;
        }
    }

    true
}

fn os_name_matches(expected: &str, actual: OsName) -> bool {
    matches!(
        (expected, actual),
        ("windows", OsName::Windows)
            | ("linux", OsName::Linux)
            | ("osx", OsName::Macos)
            | ("macos", OsName::Macos)
    )
}

fn arch_matches(expected: &str, actual: &str) -> bool {
    if expected.eq_ignore_ascii_case(actual) {
        return true;
    }

    match (expected, actual) {
        ("x86", "i386") | ("x86", "i686") => true,
        ("x86_64", "amd64") | ("amd64", "x86_64") => true,
        ("arm64", "aarch64") | ("aarch64", "arm64") => true,
        _ => false,
    }
}
