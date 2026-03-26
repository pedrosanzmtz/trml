use crate::stages::filter::is_stack_frame;
use regex::Regex;
use std::sync::LazyLock;

static EXCEPTION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(Exception|Error:|Traceback|panic at|caused by)").unwrap()
});

/// Returns true if a line is the start of a stack trace block.
fn is_stack_trigger(line: &str) -> bool {
    EXCEPTION_RE.is_match(line)
}

/// Compress stack trace blocks.
///
/// When an exception/error line is followed by indented frame lines, we:
///   - Keep the trigger line
///   - Keep the first `keep_head` frames
///   - Keep the last frame
///   - Replace the middle with `[N frames hidden]`
pub fn process(lines: Vec<String>, keep_head: usize) -> Vec<String> {
    let mut result: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;

    while i < lines.len() {
        let line = &lines[i];

        if is_stack_trigger(line) {
            // Collect the block of stack frames that follows
            let trigger = line.clone();
            let mut frames: Vec<String> = Vec::new();
            i += 1;
            while i < lines.len() && is_stack_frame(&lines[i]) {
                frames.push(lines[i].clone());
                i += 1;
            }

            result.push(trigger);

            if frames.len() <= keep_head + 1 {
                // Small enough — keep all frames
                result.extend(frames);
            } else {
                // Keep first `keep_head` frames
                for f in frames.iter().take(keep_head) {
                    result.push(f.clone());
                }
                let hidden = frames.len() - keep_head - 1;
                result.push(format!("    ... [{} frames hidden]", hidden));
                // Keep last frame
                result.push(frames.last().unwrap().clone());
            }
            // Don't increment i again — we already advanced past the block
        } else {
            result.push(line.clone());
            i += 1;
        }
    }

    result
}
