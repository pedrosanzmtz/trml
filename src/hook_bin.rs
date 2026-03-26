/// logslim-hook: Claude Code PreToolUse hook binary.
///
/// Reads hook JSON from stdin. If the Bash command reads log files,
/// rewrites it to pipe through logslim. Outputs modified JSON or nothing.
fn main() {
    // Modules are not shared between binaries without a library crate.
    // Re-implement the minimal hook logic here.
    use std::io::Read;

    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap_or(0);

    let output = process_hook_payload(&input);
    if !output.is_empty() {
        print!("{}", output);
    }
    // Exit 0 always — we never block, only rewrite
}

fn process_hook_payload(json: &str) -> String {
    let cmd = extract_json_string(json, "command");
    let Some(cmd) = cmd else {
        return String::new();
    };

    let Some(new_cmd) = rewrite_command(&cmd) else {
        return String::new();
    };

    format!(
        "{{\"tool_input\": {{\"command\": \"{}\"}}}}",
        new_cmd
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    )
}

fn rewrite_command(cmd: &str) -> Option<String> {
    if !is_log_reading_command(cmd) {
        return None;
    }
    if cmd.contains("logslim") {
        return None;
    }
    Some(format!("{} | logslim", cmd))
}

fn is_log_reading_command(cmd: &str) -> bool {
    let cmd_lower = cmd.to_lowercase();

    if cmd_lower.contains("kubectl logs") || cmd_lower.contains("docker logs") {
        return true;
    }

    let log_extensions = [".log", ".logs", ".out", ".err", ".trace"];
    let read_cmds = ["cat ", "tail ", "head "];

    let has_read_cmd = read_cmds.iter().any(|c| cmd_lower.contains(c));
    let has_log_file = log_extensions.iter().any(|ext| cmd_lower.contains(ext));

    has_read_cmd && has_log_file
}

fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\"", key);
    let pos = json.find(&needle)?;
    let after_key = &json[pos + needle.len()..];
    let after_colon = after_key.trim_start().strip_prefix(':')?.trim_start();
    if !after_colon.starts_with('"') {
        return None;
    }
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
