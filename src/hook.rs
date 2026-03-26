use std::path::Path;

/// Install the logslim hook into Claude Code's settings.json.
pub fn install(settings_path: Option<&Path>) -> std::io::Result<()> {
    let path = if let Some(p) = settings_path {
        p.to_path_buf()
    } else {
        default_settings_path()?
    };

    let hook_bin = hook_binary_path();

    // Read existing settings or start fresh
    let content = if path.exists() {
        std::fs::read_to_string(&path)?
    } else {
        "{}".to_string()
    };

    let patched = patch_settings_json(&content, &hook_bin);

    // Create parent dirs if needed
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(&path, patched)?;
    eprintln!("[logslim] Hook installed in {}", path.display());
    eprintln!("[logslim] Hook binary: {}", hook_bin);
    Ok(())
}

fn hook_binary_path() -> String {
    dirs::home_dir()
        .map(|h| h.join(".cargo").join("bin").join("logslim-hook"))
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "~/.cargo/bin/logslim-hook".to_string())
}

fn default_settings_path() -> std::io::Result<std::path::PathBuf> {
    dirs::home_dir()
        .map(|h| h.join(".claude").join("settings.json"))
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "home dir not found"))
}

/// Inject the PreToolUse hook into a Claude Code settings.json string.
/// Returns the patched JSON string.
fn patch_settings_json(content: &str, hook_cmd: &str) -> String {
    // Try to parse as JSON object. We use a simple string manipulation approach
    // to avoid a JSON dependency — serde_json is not in scope for v0.1.
    let trimmed = content.trim();

    let hook_entry = format!(
        r#"{{
        "matcher": "Bash",
        "hooks": [
          {{
            "type": "command",
            "command": "{}"
          }}
        ]
      }}"#,
        hook_cmd.replace('\\', "\\\\").replace('"', "\\\"")
    );

    let pre_tool_use_block = format!(
        r#"  "hooks": {{
    "PreToolUse": [
      {}
    ]
  }}"#,
        hook_entry
    );

    // Check if hooks key already exists
    if trimmed.contains("\"hooks\"") {
        // Don't double-patch — warn the user
        eprintln!(
            "[logslim] Warning: 'hooks' key already exists in settings.json. \
            Please add the PreToolUse entry manually:\n{}\n",
            hook_entry
        );
        return content.to_string();
    }

    // Insert the hooks block before the closing brace
    if trimmed == "{}" || trimmed == "{\n}" {
        return format!("{{\n{}\n}}", pre_tool_use_block);
    }

    // Find the last closing brace and insert before it
    if let Some(pos) = content.rfind('}') {
        let (before, after) = content.split_at(pos);
        let separator = if before.trim_end().ends_with(',') {
            "\n"
        } else {
            ",\n"
        };
        format!("{}{}{}{}", before, separator, pre_tool_use_block, after)
    } else {
        // Fallback: just append
        format!("{{\n{}\n}}", pre_tool_use_block)
    }
}

/// The hook rewrite logic: given a bash command string, if it reads log files,
/// rewrite it to pipe through logslim.
pub fn rewrite_command(cmd: &str) -> Option<String> {
    // Patterns that read log files
    let is_log_reader = is_log_reading_command(cmd);
    if !is_log_reader {
        return None;
    }

    // Check if logslim is already in the pipeline
    if cmd.contains("logslim") {
        return None;
    }

    // Append | logslim to the command
    Some(format!("{} | logslim", cmd))
}

fn is_log_reading_command(cmd: &str) -> bool {
    let cmd_lower = cmd.to_lowercase();

    // kubectl/docker log commands always pipe through
    if cmd_lower.contains("kubectl logs") || cmd_lower.contains("docker logs") {
        return true;
    }

    // cat/tail/head/less on log files
    let log_extensions = [".log", ".logs", ".out", ".err", ".trace"];
    let read_cmds = ["cat ", "tail ", "head ", "less ", "more "];

    let has_read_cmd = read_cmds.iter().any(|c| cmd_lower.contains(c));
    let has_log_file = log_extensions.iter().any(|ext| cmd_lower.contains(ext));

    has_read_cmd && has_log_file
}

/// Process a Claude Code hook JSON payload from stdin.
/// Returns the (possibly modified) JSON to write to stdout.
pub fn process_hook_payload(json: &str) -> String {
    // Extract the command from the JSON payload using simple string parsing.
    // The payload looks like: {"tool_name": "Bash", "tool_input": {"command": "..."}}
    let cmd = extract_json_string(json, "command");
    let Some(cmd) = cmd else {
        // Not a bash command or no command field — pass through unchanged
        return String::new();
    };

    let Some(new_cmd) = rewrite_command(&cmd) else {
        // No rewrite needed — pass through
        return String::new();
    };

    // Return modified tool_input JSON
    format!(
        "{{\"tool_input\": {{\"command\": \"{}\"}}}}",
        new_cmd
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    )
}

/// Very simple JSON string extractor for a known key.
fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\"", key);
    let pos = json.find(&needle)?;
    let after_key = &json[pos + needle.len()..];
    // Skip whitespace and colon
    let after_colon = after_key.trim_start().strip_prefix(':')?.trim_start();
    if !after_colon.starts_with('"') {
        return None;
    }
    // Parse the quoted string (handle escape sequences)
    let mut result = String::new();
    let mut chars = after_colon[1..].chars();
    loop {
        match chars.next()? {
            '"' => break,
            '\\' => match chars.next()? {
                'n' => result.push('\n'),
                't' => result.push('\t'),
                'r' => result.push('\r'),
                '"' => result.push('"'),
                '\\' => result.push('\\'),
                c => {
                    result.push('\\');
                    result.push(c);
                }
            },
            c => result.push(c),
        }
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rewrite_command() {
        assert!(rewrite_command("cat app.log").is_some());
        assert!(rewrite_command("tail -f kafka.log").is_some());
        assert!(rewrite_command("kubectl logs my-pod").is_some());
        assert!(rewrite_command("kubectl logs my-pod").is_some());
        assert!(rewrite_command("ls -la").is_none());
        assert!(rewrite_command("cat app.log | logslim").is_none());
    }

    #[test]
    fn test_patch_settings_empty() {
        let result = patch_settings_json("{}", "/usr/local/bin/logslim-hook");
        assert!(result.contains("PreToolUse"));
        assert!(result.contains("logslim-hook"));
    }

    #[test]
    fn test_extract_json_string() {
        let json = r#"{"tool_name": "Bash", "tool_input": {"command": "cat app.log"}}"#;
        assert_eq!(
            extract_json_string(json, "command"),
            Some("cat app.log".to_string())
        );
    }
}
