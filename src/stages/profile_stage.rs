use crate::{
    profile::Profile,
    stages::Stage,
};

/// Batch process (used in explain instrumentation).
pub fn process(lines: Vec<String>, profile: &Profile) -> Vec<String> {
    lines
        .into_iter()
        .filter(|line| {
            if profile.is_signal(line) {
                return true;
            }
            if profile.is_noise(line) {
                return false;
            }
            true
        })
        .collect()
}

/// Streaming stage for profile-based noise/signal filtering.
pub struct ProfileStage {
    profile: Profile,
}

impl ProfileStage {
    pub fn new(profile: Profile) -> Self {
        Self { profile }
    }
}

impl Stage for ProfileStage {
    fn push(&mut self, line: String) -> Vec<String> {
        if self.profile.is_signal(&line) {
            return vec![line];
        }
        if self.profile.is_noise(&line) {
            return vec![];
        }
        vec![line]
    }

    fn flush(&mut self) -> Vec<String> {
        vec![]
    }
}
