use regex::Regex;
use serde::Deserialize;
use std::path::Path;

/// Raw profile definition as parsed from YAML.
#[derive(Debug, Clone, Deserialize)]
pub struct ProfileDef {
    pub name: String,
    /// Strings that identify this service in log lines.
    #[serde(rename = "match", default)]
    pub match_strings: Vec<String>,
    #[serde(default)]
    pub noise_patterns: Vec<String>,
    #[serde(default)]
    pub signal_patterns: Vec<String>,
    #[serde(default)]
    pub stack_collapse: bool,
}

/// Compiled profile ready for use.
#[derive(Debug, Clone)]
pub struct Profile {
    pub name: String,
    pub match_strings: Vec<String>,
    pub noise_patterns: Vec<Regex>,
    pub signal_patterns: Vec<Regex>,
    pub stack_collapse: bool,
}

impl Profile {
    pub fn from_def(def: ProfileDef) -> Self {
        let noise_patterns = def
            .noise_patterns
            .iter()
            .filter_map(|p| Regex::new(p).ok())
            .collect();
        let signal_patterns = def
            .signal_patterns
            .iter()
            .filter_map(|p| Regex::new(p).ok())
            .collect();
        Self {
            name: def.name,
            match_strings: def.match_strings,
            noise_patterns,
            signal_patterns,
            stack_collapse: def.stack_collapse,
        }
    }

    /// Returns true if any of the sample lines contain a match string.
    pub fn matches_service(&self, sample_lines: &[String]) -> bool {
        for line in sample_lines.iter().take(200) {
            for s in &self.match_strings {
                if line.contains(s.as_str()) {
                    return true;
                }
            }
        }
        false
    }

    pub fn is_noise(&self, line: &str) -> bool {
        self.noise_patterns.iter().any(|re| re.is_match(line))
    }

    pub fn is_signal(&self, line: &str) -> bool {
        self.signal_patterns.iter().any(|re| re.is_match(line))
    }
}

// Bundled profiles embedded at compile time.
const BUNDLED: &[(&str, &str)] = &[
    ("nifi", include_str!("../profiles/nifi.yml")),
    ("kafka", include_str!("../profiles/kafka.yml")),
    ("clickhouse", include_str!("../profiles/clickhouse.yml")),
    ("kubernetes", include_str!("../profiles/kubernetes.yml")),
    ("redis", include_str!("../profiles/redis.yml")),
    ("mongodb", include_str!("../profiles/mongodb.yml")),
    ("nginx", include_str!("../profiles/nginx.yml")),
    ("gc", include_str!("../profiles/gc.yml")),
    ("elasticsearch", include_str!("../profiles/elasticsearch.yml")),
];

/// Load all bundled profiles.
pub fn bundled_profiles() -> Vec<Profile> {
    BUNDLED
        .iter()
        .filter_map(|(_, yaml)| parse_profile(yaml))
        .collect()
}

/// Load user profiles from ~/.trml/profiles/.
pub fn user_profiles(dir: &Path) -> Vec<Profile> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map(|ext| ext == "yml" || ext == "yaml")
                .unwrap_or(false)
        })
        .filter_map(|e| {
            let content = std::fs::read_to_string(e.path()).ok()?;
            parse_profile(&content)
        })
        .collect()
}

pub fn parse_profile(yaml: &str) -> Option<Profile> {
    let def: ProfileDef = serde_yaml::from_str(yaml).ok()?;
    Some(Profile::from_def(def))
}

/// Auto-detect which profile best matches the log sample.
pub fn detect_profile<'a>(profiles: &'a [Profile], sample: &[String]) -> Option<&'a Profile> {
    profiles.iter().find(|p| p.matches_service(sample))
}

/// Load a profile by name (checks user profiles first, then bundled).
pub fn load_by_name(name: &str, user_dir: &Path) -> Option<Profile> {
    // Try user profiles first
    let user = user_profiles(user_dir);
    if let Some(p) = user.into_iter().find(|p| p.name == name) {
        return Some(p);
    }
    // Fall back to bundled
    bundled_profiles().into_iter().find(|p| p.name == name)
}
