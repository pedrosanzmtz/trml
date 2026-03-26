use crate::pipeline::{ExplainLine, Stats};
use std::io::Write;

/// Write the compressed output lines to `writer`.
pub fn write_output(lines: &[String], writer: &mut impl Write) -> std::io::Result<()> {
    for line in lines {
        writeln!(writer, "{}", line)?;
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
    // Simple comma-separated thousands formatting
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
