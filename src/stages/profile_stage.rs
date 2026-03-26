use crate::profile::Profile;

/// Apply profile noise/signal patterns.
///
/// Signal patterns override noise: if a line matches a signal pattern it is
/// always kept. If a line matches only noise patterns it is dropped.
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
