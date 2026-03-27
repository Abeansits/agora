use anyhow::{Context, Result};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub fn sessions_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".agora").join("sessions")
}

pub fn forum_dir(id: &str) -> PathBuf {
    sessions_dir().join(id)
}

pub fn create_forum_dir(id: &str) -> Result<PathBuf> {
    let dir = forum_dir(id);
    fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create forum directory: {}", dir.display()))?;
    Ok(dir)
}

pub fn create_round_dir(forum: &Path, round: u32) -> Result<PathBuf> {
    let dir = forum.join(format!("round-{}", round));
    fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create round directory: {}", dir.display()))?;
    Ok(dir)
}

pub fn create_final_dir(forum: &Path) -> Result<PathBuf> {
    let dir = forum.join("final");
    fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create final directory: {}", dir.display()))?;
    Ok(dir)
}

/// Write a file atomically: write to .tmp, then rename
pub fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let tmp_path = path.with_extension("md.tmp");
    fs::write(&tmp_path, content)
        .with_context(|| format!("Failed to write temp file: {}", tmp_path.display()))?;
    fs::rename(&tmp_path, path)
        .with_context(|| format!("Failed to rename {} -> {}", tmp_path.display(), path.display()))?;
    Ok(())
}

/// Write a TOML file atomically
pub fn write_atomic_toml(path: &Path, content: &str) -> Result<()> {
    let tmp_path = path.with_extension("toml.tmp");
    fs::write(&tmp_path, content)
        .with_context(|| format!("Failed to write temp file: {}", tmp_path.display()))?;
    fs::rename(&tmp_path, path)
        .with_context(|| format!("Failed to rename {} -> {}", tmp_path.display(), path.display()))?;
    Ok(())
}

pub fn read_file(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("Failed to read: {}", path.display()))
}

pub fn read_response(forum: &Path, round: u32, participant: &str) -> Result<Option<String>> {
    let path = forum
        .join(format!("round-{}", round))
        .join(format!("{}.md", participant));
    if path.exists() {
        Ok(Some(read_file(&path)?))
    } else {
        Ok(None)
    }
}

pub fn read_all_responses(
    forum: &Path,
    round: u32,
    participants: &[String],
) -> Result<HashMap<String, String>> {
    let mut responses = HashMap::new();
    for name in participants {
        if let Some(content) = read_response(forum, round, name)? {
            responses.insert(name.clone(), content);
        }
    }
    Ok(responses)
}

/// Watch a directory for expected participant response files using notify.
/// Returns collected responses when all are present or timeout is reached.
pub fn watch_for_responses(
    round_dir: &Path,
    expected: &[String],
    timeout: Duration,
) -> Result<HashMap<String, String>> {
    let mut responses = HashMap::new();
    let start = Instant::now();

    // Start watcher BEFORE scanning for existing files to avoid race condition
    // (file could arrive between scan and watch registration)
    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = RecommendedWatcher::new(tx, Config::default())
        .with_context(|| "Failed to create filesystem watcher")?;
    watcher
        .watch(round_dir, RecursiveMode::NonRecursive)
        .with_context(|| format!("Failed to watch directory: {}", round_dir.display()))?;

    // Now check for files already present
    for name in expected {
        let path = round_dir.join(format!("{}.md", name));
        if path.exists() {
            let content = read_file(&path)?;
            responses.insert(name.clone(), content);
        }
    }

    if responses.len() == expected.len() {
        return Ok(responses);
    }

    loop {
        let elapsed = start.elapsed();
        if elapsed >= timeout {
            break;
        }
        let remaining = timeout - elapsed;

        match rx.recv_timeout(remaining) {
            Ok(Ok(event)) => {
                for path in &event.paths {
                    if let Some(filename) = path.file_name().and_then(|f| f.to_str()) {
                        if let Some(name) = filename.strip_suffix(".md") {
                            if expected.contains(&name.to_string())
                                && !responses.contains_key(name)
                                && !name.ends_with(".tmp") // ignore temp files
                            {
                                // Retry with bounded backoff for atomic rename
                                let mut read_ok = false;
                                for delay_ms in [10, 50, 100, 200] {
                                    std::thread::sleep(Duration::from_millis(delay_ms));
                                    if path.exists() {
                                        if let Ok(content) = read_file(path) {
                                            if !content.is_empty() {
                                                eprintln!("  Received response from: {}", name);
                                                responses.insert(name.to_string(), content);
                                                read_ok = true;
                                                break;
                                            }
                                        }
                                    }
                                }
                                if !read_ok {
                                    eprintln!("  Warning: could not read response from {}", name);
                                }
                            }
                        }
                    }
                }
            }
            Ok(Err(e)) => eprintln!("Watch error: {}", e),
            Err(mpsc::RecvTimeoutError::Timeout) => break,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if responses.len() == expected.len() {
            break;
        }
    }

    Ok(responses)
}

/// List all forum IDs and their directory paths
pub fn list_forums() -> Result<Vec<(String, PathBuf)>> {
    let dir = sessions_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut forums = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let meta_path = entry.path().join("meta.toml");
            if meta_path.exists() {
                if let Some(name) = entry.file_name().to_str() {
                    forums.push((name.to_string(), entry.path()));
                }
            }
        }
    }
    forums.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(forums)
}

/// Determine current round number from existing round directories
pub fn current_round(forum: &Path) -> u32 {
    let mut round = 0;
    loop {
        let next = forum.join(format!("round-{}", round + 1));
        if next.exists() {
            round += 1;
        } else {
            break;
        }
    }
    round
}

/// Check if a forum has completed (final/synthesis.md exists)
pub fn is_completed(forum: &Path) -> bool {
    forum.join("final").join("synthesis.md").exists()
}

/// Invoke a participant command with timeout.
/// Replaces {prompt_file} with a temp file path. Sets AGORA_PROMPT and AGORA_PROMPT_FILE
/// as environment variables. Does NOT do `{prompt}` text substitution to prevent shell injection.
pub fn invoke_command(
    command_template: &str,
    prompt: &str,
    timeout: Duration,
) -> Result<String> {
    let tmp_file = std::env::temp_dir().join(format!("agora-{}.md", uuid::Uuid::new_v4()));
    fs::write(&tmp_file, prompt)
        .with_context(|| "Failed to write prompt temp file")?;

    let command = command_template
        .replace("{prompt_file}", &tmp_file.display().to_string());

    let prompt_owned = prompt.to_string();
    let tmp_display = tmp_file.display().to_string();
    let cmd_for_thread = command.clone();

    // Run in a thread so we can enforce a timeout
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd_for_thread)
            .env("AGORA_PROMPT", &prompt_owned)
            .env("AGORA_PROMPT_FILE", &tmp_display)
            .output();
        tx.send(result).ok();
    });

    let output = rx
        .recv_timeout(timeout)
        .map_err(|_| {
            anyhow::anyhow!(
                "Command timed out after {:?}: {}",
                timeout,
                command_template
            )
        })?
        .with_context(|| format!("Failed to execute: {}", command_template))?;

    fs::remove_file(&tmp_file).ok();

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Command failed ({}): {}", command_template, stderr);
    }

    String::from_utf8(output.stdout)
        .with_context(|| "Invalid UTF-8 in command output")
        .map(|s| s.trim().to_string())
}

/// Invoke the claude CLI with a prompt and return the response
pub fn invoke_model(model: &str, prompt: &str) -> Result<String> {
    let output = std::process::Command::new("claude")
        .arg("--model")
        .arg(model)
        .arg("-p")
        .arg(prompt)
        .arg("--output-format")
        .arg("text")
        .output()
        .with_context(|| {
            "Failed to invoke 'claude' CLI. Ensure Claude Code is installed and in PATH."
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Claude CLI failed: {}", stderr);
    }

    String::from_utf8(output.stdout)
        .with_context(|| "Invalid UTF-8 in model output")
        .map(|s| s.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_atomic() {
        let dir = std::env::temp_dir().join("agora-test-atomic");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.md");

        write_atomic(&path, "hello world").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello world");
        assert!(!path.with_extension("md.tmp").exists());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_write_atomic_toml() {
        let dir = std::env::temp_dir().join("agora-test-atomic-toml");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("claims.toml");

        write_atomic_toml(&path, "[test]\nkey = \"value\"").unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[test]\nkey = \"value\""
        );
        assert!(!path.with_extension("toml.tmp").exists());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_current_round() {
        let dir = std::env::temp_dir().join("agora-test-rounds");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        assert_eq!(current_round(&dir), 0);

        fs::create_dir_all(dir.join("round-1")).unwrap();
        assert_eq!(current_round(&dir), 1);

        fs::create_dir_all(dir.join("round-2")).unwrap();
        assert_eq!(current_round(&dir), 2);

        // Gap: round-3 missing, round-4 exists — should stop at 2
        fs::create_dir_all(dir.join("round-4")).unwrap();
        assert_eq!(current_round(&dir), 2);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_is_completed() {
        let dir = std::env::temp_dir().join("agora-test-completed");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        assert!(!is_completed(&dir));

        let final_dir = dir.join("final");
        fs::create_dir_all(&final_dir).unwrap();
        assert!(!is_completed(&dir)); // dir exists but no synthesis.md

        fs::write(final_dir.join("synthesis.md"), "done").unwrap();
        assert!(is_completed(&dir));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_read_all_responses() {
        let dir = std::env::temp_dir().join("agora-test-responses");
        let _ = fs::remove_dir_all(&dir);
        let round_dir = dir.join("round-1");
        fs::create_dir_all(&round_dir).unwrap();

        fs::write(round_dir.join("alice.md"), "Alice's response").unwrap();
        fs::write(round_dir.join("bob.md"), "Bob's response").unwrap();

        let participants = vec!["alice".to_string(), "bob".to_string(), "charlie".to_string()];
        let responses = read_all_responses(&dir, 1, &participants).unwrap();

        assert_eq!(responses.len(), 2);
        assert_eq!(responses["alice"], "Alice's response");
        assert_eq!(responses["bob"], "Bob's response");
        assert!(!responses.contains_key("charlie"));

        fs::remove_dir_all(&dir).ok();
    }
}
