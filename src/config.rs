use crate::types::*;
use anyhow::{Context, Result};
use std::path::Path;
use std::time::Duration;

pub fn load(path: &Path) -> Result<ForumConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config: {}", path.display()))?;
    let config: ForumConfig =
        toml::from_str(&content).with_context(|| "Failed to parse meta.toml")?;
    validate(&config)?;
    Ok(config)
}

pub fn save(config: &ForumConfig, path: &Path) -> Result<()> {
    let content =
        toml::to_string_pretty(config).with_context(|| "Failed to serialize config")?;
    std::fs::write(path, content)
        .with_context(|| format!("Failed to write config: {}", path.display()))?;
    Ok(())
}

pub fn validate(config: &ForumConfig) -> Result<()> {
    validate_id(&config.forum.id, "forum ID")?;
    if config.participants.names.is_empty() {
        anyhow::bail!("At least one participant required");
    }
    for name in &config.participants.names {
        validate_id(name, "participant name")?;
        if !config.participants.configs.contains_key(name) {
            anyhow::bail!("Missing config for participant: {}", name);
        }
        let pc = &config.participants.configs[name];
        if pc.participant_type == "command" && pc.command.is_none() {
            anyhow::bail!("Command participant '{}' requires a command", name);
        }
        if !["command", "manual"].contains(&pc.participant_type.as_str()) {
            anyhow::bail!(
                "Unknown participant type '{}' for '{}'",
                pc.participant_type,
                name
            );
        }
    }
    // Reject reserved filenames as participant names
    for name in &config.participants.names {
        if ["prompt", "synthesis", "claims"].contains(&name.as_str()) {
            anyhow::bail!(
                "Participant name '{}' conflicts with reserved filename",
                name
            );
        }
    }
    if config.forum.max_rounds == 0 {
        anyhow::bail!("max_rounds must be > 0");
    }
    if config.convergence.threshold == 0 || config.convergence.threshold > 10 {
        anyhow::bail!("convergence threshold must be 1-10");
    }
    Ok(())
}

/// Validate an identifier (participant name or forum ID) to prevent path traversal.
/// Must match [a-z0-9_-], max 64 chars, no path separators.
fn validate_id(id: &str, label: &str) -> Result<()> {
    if id.is_empty() {
        anyhow::bail!("{} cannot be empty", label);
    }
    if id.len() > 64 {
        anyhow::bail!("{} too long (max 64 chars): {}", label, id);
    }
    if id.contains('/') || id.contains('\\') || id.contains("..") || id.contains('\0') {
        anyhow::bail!("{} contains illegal characters: {}", label, id);
    }
    if id == "." || id == ".." {
        anyhow::bail!("{} cannot be '.' or '..'", label);
    }
    // Participant names have stricter rules (used as filenames)
    if label == "participant name"
        && !id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        anyhow::bail!(
            "{} must be [a-z0-9_-] only: {}",
            label,
            id
        );
    }
    Ok(())
}

pub fn parse_duration(s: &str) -> Result<Duration> {
    let s = s.trim();
    if let Some(mins) = s.strip_suffix('m') {
        let n: u64 = mins
            .parse()
            .with_context(|| format!("Invalid duration: {}", s))?;
        Ok(Duration::from_secs(n * 60))
    } else if let Some(secs) = s.strip_suffix('s') {
        let n: u64 = secs
            .parse()
            .with_context(|| format!("Invalid duration: {}", s))?;
        Ok(Duration::from_secs(n))
    } else if let Some(hours) = s.strip_suffix('h') {
        let n: u64 = hours
            .parse()
            .with_context(|| format!("Invalid duration: {}", s))?;
        Ok(Duration::from_secs(n * 3600))
    } else {
        anyhow::bail!(
            "Invalid duration format '{}' (expected e.g. '5m', '30s', '1h')",
            s
        );
    }
}

/// Parse a CLI participant spec like "name:type:command" or "name:manual"
pub fn parse_participant_spec(spec: &str) -> Result<(String, ParticipantConfig)> {
    let parts: Vec<&str> = spec.splitn(3, ':').collect();
    match parts.len() {
        2 => {
            let name = parts[0].to_string();
            let ptype = parts[1].to_string();
            if ptype != "manual" {
                anyhow::bail!(
                    "Participant '{}' of type '{}' requires a command (use name:type:command)",
                    name,
                    ptype
                );
            }
            Ok((
                name,
                ParticipantConfig {
                    participant_type: ptype,
                    command: None,
                },
            ))
        }
        3 => {
            let name = parts[0].to_string();
            let ptype = parts[1].to_string();
            let cmd = parts[2].to_string();
            Ok((
                name,
                ParticipantConfig {
                    participant_type: ptype,
                    command: Some(cmd),
                },
            ))
        }
        _ => anyhow::bail!(
            "Invalid participant spec '{}' (expected name:type or name:type:command)",
            spec
        ),
    }
}

/// Map model shorthand to actual model ID
pub fn resolve_model(model: &str) -> &str {
    match model {
        "claude-sonnet" => "claude-sonnet-4-6",
        "claude-opus" => "claude-opus-4-6",
        "claude-haiku" => "claude-haiku-4-5-20251001",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("5m").unwrap(), Duration::from_secs(300));
        assert_eq!(parse_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_duration("1h").unwrap(), Duration::from_secs(3600));
        assert!(parse_duration("abc").is_err());
        assert!(parse_duration("").is_err());
    }

    #[test]
    fn test_parse_participant_spec_manual() {
        let (name, config) = parse_participant_spec("human:manual").unwrap();
        assert_eq!(name, "human");
        assert_eq!(config.participant_type, "manual");
        assert!(config.command.is_none());
    }

    #[test]
    fn test_parse_participant_spec_command() {
        let (name, config) =
            parse_participant_spec("v0:command:claude -p '{prompt}'").unwrap();
        assert_eq!(name, "v0");
        assert_eq!(config.participant_type, "command");
        assert_eq!(config.command.unwrap(), "claude -p '{prompt}'");
    }

    #[test]
    fn test_parse_participant_spec_invalid() {
        assert!(parse_participant_spec("onlyname").is_err());
        assert!(parse_participant_spec("name:command").is_err()); // command type needs 3rd part
    }

    #[test]
    fn test_load_config_from_string() {
        let toml_str = r#"
[forum]
id = "test-001"
topic = "Test topic"
created = "2026-03-27T00:00:00Z"
max_rounds = 3

[participants]
names = ["alice", "bob"]

[participants.alice]
type = "command"
command = "echo 'hello'"

[participants.bob]
type = "manual"
"#;
        let config: ForumConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.forum.id, "test-001");
        assert_eq!(config.forum.topic, "Test topic");
        assert_eq!(config.forum.max_rounds, 3);
        assert_eq!(config.participants.names.len(), 2);
        assert_eq!(
            config.participants.configs["alice"].participant_type,
            "command"
        );
        assert_eq!(
            config.participants.configs["alice"].command.as_deref(),
            Some("echo 'hello'")
        );
        assert_eq!(
            config.participants.configs["bob"].participant_type,
            "manual"
        );
        assert!(config.participants.configs["bob"].command.is_none());
        // Defaults
        assert_eq!(config.timing.round_timeout, "5m");
        assert_eq!(config.convergence.threshold, 7);
        assert_eq!(config.synthesis.model, "claude-sonnet");
    }

    #[test]
    fn test_resolve_model() {
        assert_eq!(resolve_model("claude-sonnet"), "claude-sonnet-4-6");
        assert_eq!(resolve_model("claude-opus"), "claude-opus-4-6");
        assert_eq!(resolve_model("gpt-4o"), "gpt-4o"); // passthrough
    }

    #[test]
    fn test_validate_id_rejects_traversal() {
        assert!(validate_id("../etc", "test").is_err());
        assert!(validate_id("foo/../bar", "test").is_err());
        assert!(validate_id(".", "test").is_err());
        assert!(validate_id("..", "test").is_err());
        assert!(validate_id("foo/bar", "test").is_err());
        assert!(validate_id("", "test").is_err());
        assert!(validate_id("foo\0bar", "test").is_err());
    }

    #[test]
    fn test_validate_id_accepts_valid() {
        assert!(validate_id("agora-2026-03-27-001", "forum ID").is_ok());
        assert!(validate_id("alice", "participant name").is_ok());
        assert!(validate_id("agent-v0", "participant name").is_ok());
        assert!(validate_id("model_1", "participant name").is_ok());
    }

    #[test]
    fn test_validate_id_rejects_uppercase_participant() {
        assert!(validate_id("Alice", "participant name").is_err());
        assert!(validate_id("Agent-V0", "participant name").is_err());
    }

    #[test]
    fn test_validate_reserved_participant_names() {
        let toml_str = r#"
[forum]
id = "test-001"
topic = "Test"
created = "2026-03-27T00:00:00Z"
max_rounds = 3

[participants]
names = ["prompt"]

[participants.prompt]
type = "manual"
"#;
        let config: ForumConfig = toml::from_str(toml_str).unwrap();
        assert!(validate(&config).is_err());
    }

    #[test]
    fn test_validate_threshold_zero_rejected() {
        let toml_str = r#"
[forum]
id = "test-001"
topic = "Test"
created = "2026-03-27T00:00:00Z"
max_rounds = 3

[participants]
names = ["alice"]

[participants.alice]
type = "manual"

[convergence]
threshold = 0
"#;
        let config: ForumConfig = toml::from_str(toml_str).unwrap();
        assert!(validate(&config).is_err());
    }
}
