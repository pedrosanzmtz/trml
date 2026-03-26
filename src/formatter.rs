use crate::pipeline::{ExplainLine, Stats};
use std::io::Write;

// ANSI color codes
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

/// Apply ANSI colors to a single output line based on its content.
/// ERROR/FATAL/CRITICAL → red, WARN → yellow, dedup annotations → dim.
pub fn colorize(line: &str) -> String {
    // Dedup annotations (dim gray)
    if line.contains("[repeated x") || line.contains("] [x") {
        // Find the annotation start
        if let Some(pos) = line.rfind(" [repeated x").or_else(|| line.rfind(" [x")) {
            let (body, annotation) = line.split_at(pos);
            return format!("{}{}{}{}", body, DIM, annotation, RESET);
        }
    }

    // Severity-based coloring (look for level keyword)
    let upper = line.to_uppercase();
    // Check for fatal/error keywords before warn to pick strongest signal
    if upper.contains(" ERROR ")
        || upper.contains("[ERROR]")
        || upper.contains("FATAL")
        || upper.contains("CRITICAL")
        || upper.contains("SEVERE")
    {
        return format!("{}{}{}", RED, line, RESET);
    }
    if upper.contains(" WARN ") || upper.contains("[WARN]") || upper.contains("WARNING") {
        return format!("{}{}{}", YELLOW, line, RESET);
    }

    line.to_string()
}

/// Write the compressed output lines to `writer`, with optional color.
pub fn write_output(
    lines: &[String],
    writer: &mut impl Write,
    use_color: bool,
) -> std::io::Result<()> {
    for line in lines {
        if use_color {
            writeln!(writer, "{}", colorize(line))?;
        } else {
            writeln!(writer, "{}", line)?;
        }
    }
    Ok(())
}

/// Write stats to stderr.
pub fn write_stats(stats: &Stats, writer: &mut impl Write) -> std::io::Result<()> {
    writeln!(
        writer,
        "[logslim] {:>10} lines → {:>6} lines ({:.1}% reduction, ~{:.0}% token reduction)",
        format_number(stats.input_lines),
        format_number(stats.output_lines),
        stats.line_reduction_pct(),
        stats.token_reduction_pct(),
    )
}

/// Write explain output to `writer`.
pub fn write_explain(explain: &[ExplainLine], writer: &mut impl Write) -> std::io::Result<()> {
    for entry in explain {
        let marker = if entry.kept { "KEEP" } else { "DROP" };
        let reason = entry.reason.as_deref().unwrap_or("");
        let stage = entry.stage.as_deref().unwrap_or("");
        if stage.is_empty() && reason.is_empty() {
            writeln!(writer, "[{}] {}", marker, entry.original)?;
        } else {
            writeln!(
                writer,
                "[{}] [{}:{}] {}",
                marker, stage, reason, entry.original
            )?;
        }
    }
    Ok(())
}

fn format_number(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    result.chars().rev().collect()
}
