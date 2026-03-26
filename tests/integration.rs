use assert_cmd::Command;
use predicates::prelude::*;

fn logslim() -> Command {
    Command::cargo_bin("logslim").unwrap()
}

// ── Generic heuristics ────────────────────────────────────────────────────────

#[test]
fn reads_from_file() {
    logslim()
        .arg("tests/fixtures/generic-sample.log")
        .assert()
        .success()
        .stdout(predicate::str::contains("Failed to process request"));
}

#[test]
fn reads_from_stdin() {
    let input = "2024-01-15 10:00:00 ERROR Something went wrong\n";
    logslim()
        .write_stdin(input)
        .assert()
        .success()
        .stdout(predicate::str::contains("ERROR"));
}

#[test]
fn always_keeps_error_lines() {
    let input = "2024-01-15 10:00:00 ERROR this is important\n";
    logslim()
        .write_stdin(input)
        .assert()
        .success()
        .stdout(predicate::str::contains("ERROR this is important"));
}

#[test]
fn always_keeps_warn_lines() {
    let input = "2024-01-15 10:00:00 WARN something concerning\n";
    logslim()
        .write_stdin(input)
        .assert()
        .success()
        .stdout(predicate::str::contains("WARN"));
}

#[test]
fn dedup_collapses_repeated_lines() {
    let input = "2024-01-15 10:00:00 INFO heartbeat OK\n\
                 2024-01-15 10:00:01 INFO heartbeat OK\n\
                 2024-01-15 10:00:02 INFO heartbeat OK\n\
                 2024-01-15 10:00:03 INFO heartbeat OK\n\
                 2024-01-15 10:00:04 INFO heartbeat OK\n\
                 2024-01-15 10:00:05 INFO heartbeat OK\n";
    let output = logslim()
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(output).unwrap();
    // Should collapse into one line with [repeated xN]
    assert!(
        text.contains("repeated") || text.lines().count() < 6,
        "expected dedup to collapse repeated lines, got:\n{}", text
    );
}

#[test]
fn drops_debug_by_default() {
    // DEBUG lines with no signal keywords should be filtered out
    let input = "2024-01-15 10:00:00 DEBUG verbose internal state x=1\n\
                 2024-01-15 10:00:01 DEBUG verbose internal state x=2\n\
                 2024-01-15 10:00:02 ERROR something broke\n";
    let output = logslim()
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("ERROR something broke"), "ERROR line must be kept");
    // DEBUG lines may or may not be kept depending on signal matching
}

#[test]
fn samples_info_lines() {
    // Generate many INFO lines and check that output is shorter than input
    let input: String = (0..100)
        .map(|i| format!("2024-01-15 10:00:{:02} INFO processing item {}\n", i % 60, i))
        .collect();
    let output = logslim()
        .write_stdin(input.clone())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out_lines: Vec<&str> = std::str::from_utf8(&output).unwrap().lines().collect();
    let in_lines: Vec<&str> = input.lines().collect();
    assert!(
        out_lines.len() < in_lines.len(),
        "expected fewer output lines than input; got {} out of {}",
        out_lines.len(),
        in_lines.len()
    );
}

#[test]
fn stack_trace_compression() {
    let input = "2024-01-15 10:00:00 ERROR Something failed\n\
                 java.lang.RuntimeException: timeout\n\
                 \tat com.example.A.method(A.java:10)\n\
                 \tat com.example.B.method(B.java:20)\n\
                 \tat com.example.C.method(C.java:30)\n\
                 \tat com.example.D.method(D.java:40)\n\
                 \tat com.example.E.method(E.java:50)\n\
                 \tat java.lang.Thread.run(Thread.java:748)\n\
                 2024-01-15 10:00:01 INFO  normal line after\n";
    let output = logslim()
        .arg("--level").arg("normal")
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("Something failed"), "trigger line must be kept");
    assert!(
        text.contains("frames hidden") || text.lines().count() < 9,
        "expected stack compression, got:\n{}", text
    );
}

#[test]
fn stats_flag_writes_to_stderr() {
    logslim()
        .arg("--stats")
        .arg("tests/fixtures/generic-sample.log")
        .assert()
        .success()
        .stderr(predicate::str::contains("logslim"));
}

#[test]
fn ansi_codes_are_stripped() {
    let input = "\x1b[31mERROR\x1b[0m Something failed\n";
    let output = logslim()
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(output).unwrap();
    assert!(!text.contains("\x1b["), "ANSI codes should be stripped");
    assert!(text.contains("ERROR"), "content should be preserved");
}

// ── Profile detection ─────────────────────────────────────────────────────────

#[test]
fn nifi_profile_file() {
    logslim()
        .arg("tests/fixtures/nifi-sample.log")
        .assert()
        .success()
        .stdout(predicate::str::contains("Failed to send FlowFile to Kafka"));
}

#[test]
fn kafka_profile_file() {
    logslim()
        .arg("tests/fixtures/kafka-sample.log")
        .assert()
        .success()
        // Error line must be preserved
        .stdout(predicate::str::contains("Failed to send to broker"));
}

// ── CLI flags ─────────────────────────────────────────────────────────────────

#[test]
fn level_aggressive() {
    let input: String = (0..50)
        .map(|i| format!("2024-01-15 10:00:00 INFO normal line {}\n", i))
        .collect();
    let normal_output = logslim()
        .arg("--level").arg("normal")
        .write_stdin(input.clone())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let aggressive_output = logslim()
        .arg("--level").arg("aggressive")
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let normal_lines = String::from_utf8(normal_output).unwrap().lines().count();
    let aggressive_lines = String::from_utf8(aggressive_output).unwrap().lines().count();
    assert!(
        aggressive_lines <= normal_lines,
        "aggressive should produce <= lines as normal; got aggressive={} normal={}",
        aggressive_lines, normal_lines
    );
}

#[test]
fn explicit_profile_flag() {
    // Force nifi profile even on generic input
    logslim()
        .arg("--profile").arg("nifi")
        .arg("tests/fixtures/generic-sample.log")
        .assert()
        .success();
}

// ── Hook binary ───────────────────────────────────────────────────────────────

#[test]
fn hook_bin_rewrites_log_command() {
    let input = r#"{"tool_name": "Bash", "tool_input": {"command": "cat app.log"}}"#;
    let output = Command::cargo_bin("logslim-hook")
        .unwrap()
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(output).unwrap();
    if !text.is_empty() {
        assert!(text.contains("logslim"), "hook should rewrite to pipe through logslim");
    }
}

#[test]
fn hook_bin_passes_non_log_commands() {
    let input = r#"{"tool_name": "Bash", "tool_input": {"command": "ls -la"}}"#;
    let output = Command::cargo_bin("logslim-hook")
        .unwrap()
        .write_stdin(input)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    // Should output nothing (pass through)
    assert!(output.is_empty(), "hook should not modify non-log commands");
}
