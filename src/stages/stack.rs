use crate::stages::{filter::is_stack_frame, Stage};
use regex::Regex;
use std::sync::LazyLock;

static EXCEPTION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(Exception|Error:|Traceback|panic at|caused by)").unwrap()
});

fn is_stack_trigger(line: &str) -> bool {
    EXCEPTION_RE.is_match(line)
}

/// Batch process (used in explain instrumentation and learn mode).
pub fn process(lines: Vec<String>, keep_head: usize) -> Vec<String> {
    let mut result: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;

    while i < lines.len() {
        let line = &lines[i];

        if is_stack_trigger(line) {
            let trigger = line.clone();
            let mut frames: Vec<String> = Vec::new();
            i += 1;
            while i < lines.len() && is_stack_frame(&lines[i]) {
                frames.push(lines[i].clone());
                i += 1;
            }

            result.push(trigger);

            if frames.len() <= keep_head + 1 {
                result.extend(frames);
            } else {
                for f in frames.iter().take(keep_head) {
                    result.push(f.clone());
                }
                let hidden = frames.len() - keep_head - 1;
                result.push(format!("    ... [{} frames hidden]", hidden));
                result.push(frames.last().unwrap().clone());
            }
        } else {
            result.push(line.clone());
            i += 1;
        }
    }

    result
}

/// Streaming stage for stack trace compression.
pub struct StackStage {
    keep_head: usize,
    trigger: Option<String>,
    frames: Vec<String>,
}

impl StackStage {
    pub fn new(keep_head: usize) -> Self {
        Self {
            keep_head,
            trigger: None,
            frames: Vec::new(),
        }
    }

    fn emit_block(&mut self) -> Vec<String> {
        let Some(trigger) = self.trigger.take() else {
            return vec![];
        };
        let mut out = vec![trigger];
        let frames = std::mem::take(&mut self.frames);
        if frames.len() <= self.keep_head + 1 {
            out.extend(frames);
        } else {
            for f in frames.iter().take(self.keep_head) {
                out.push(f.clone());
            }
            let hidden = frames.len() - self.keep_head - 1;
            out.push(format!("    ... [{} frames hidden]", hidden));
            out.push(frames.last().unwrap().clone());
        }
        out
    }
}

impl Stage for StackStage {
    fn push(&mut self, line: String) -> Vec<String> {
        if self.trigger.is_some() {
            if is_stack_frame(&line) {
                self.frames.push(line);
                return vec![];
            }
            // End of stack block — emit it, then handle current line.
            let mut out = self.emit_block();
            if is_stack_trigger(&line) {
                self.trigger = Some(line);
            } else {
                out.push(line);
            }
            return out;
        }

        if is_stack_trigger(&line) {
            self.trigger = Some(line);
            vec![]
        } else {
            vec![line]
        }
    }

    fn flush(&mut self) -> Vec<String> {
        self.emit_block()
    }
}
