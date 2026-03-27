pub mod dedup;
pub mod filter;
pub mod normalize;
pub mod profile_stage;
pub mod stack;
pub mod strip;

/// A streaming pipeline stage.
///
/// Each stage receives one line at a time via `push` and returns zero or more
/// output lines. When all input is exhausted, `flush` must be called once to
/// drain any buffered state (e.g. the last dedup group, pending stack frames).
pub trait Stage: Send {
    fn push(&mut self, line: String) -> Vec<String>;
    fn flush(&mut self) -> Vec<String>;
}
