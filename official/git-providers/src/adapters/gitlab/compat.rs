//! GitLab version gates (design §2.3): what a self-managed GitLab of a
//! given version supports, from `GET /version`. An unknown version takes
//! the path every supported GitLab has, so an old or unreadable server
//! still works, only with more requests.

/// A GitLab version, `major.minor.patch` (`16.11.2-ee` reads as 16.11.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        let core = text
            .trim()
            .split(|c: char| c == '-' || c == '+' || c.is_whitespace())
            .next()?;
        let mut parts = core.split('.');
        let mut next = || -> Option<u32> {
            let part = parts.next()?;
            (!part.is_empty() && part.len() <= 6 && part.bytes().all(|b| b.is_ascii_digit()))
                .then(|| part.parse().ok())
                .flatten()
        };
        let major = next()?;
        let minor = next()?;
        let patch = next().unwrap_or(0);
        Some(Self::new(major, minor, patch))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// `reviewer_username` on merge request lists: GitLab 13.8.
pub const REVIEWER_FILTER: Version = Version::new(13, 8, 0);
/// The `draft` field (before: `work_in_progress` only): GitLab 14.0.
pub const DRAFT_FIELD: Version = Version::new(14, 0, 0);
/// `GET /personal_access_tokens/self`: GitLab 15.5.
pub const TOKEN_SELF: Version = Version::new(15, 5, 0);
/// A pipeline's `name`: GitLab 15.11.
pub const PIPELINE_NAME: Version = Version::new(15, 11, 0);

/// What the adapter does on a server of `version` (`None`: unknown).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gates {
    pub version: Option<Version>,
}

impl Gates {
    pub fn new(version: Option<Version>) -> Self {
        Self { version }
    }

    fn at_least(&self, min: Version) -> bool {
        self.version.is_some_and(|v| v >= min)
    }

    /// Whether review requests can be asked for directly; otherwise the
    /// open merge requests are read and filtered by their reviewers.
    pub fn reviewer_filter(&self) -> bool {
        self.at_least(REVIEWER_FILTER)
    }

    /// Whether a merge request's `draft` is reliable; otherwise
    /// `work_in_progress` is read as well (both are read either way).
    pub fn draft_field(&self) -> bool {
        self.at_least(DRAFT_FIELD)
    }

    /// Whether to ask the token about itself. An unknown version asks, and
    /// a 404 then means "scopes unknown".
    pub fn token_self(&self) -> bool {
        self.version.is_none_or(|v| v >= TOKEN_SELF)
    }

    pub fn pipeline_name(&self) -> bool {
        self.at_least(PIPELINE_NAME)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_read_from_what_gitlab_reports() {
        assert_eq!(Version::parse("16.11.2-ee"), Some(Version::new(16, 11, 2)));
        assert_eq!(Version::parse("13.12.15"), Some(Version::new(13, 12, 15)));
        assert_eq!(Version::parse("17.0"), Some(Version::new(17, 0, 0)));
        assert_eq!(Version::parse(" 15.5.0+abc "), Some(Version::new(15, 5, 0)));
        for bad in ["", "v16.1", "sixteen", "16", "16.x.1", "1234567.1"] {
            assert_eq!(Version::parse(bad), None, "{bad}");
        }
        assert_eq!(Version::new(16, 11, 2).to_string(), "16.11.2");
    }

    #[test]
    fn every_gate_has_both_branches() {
        let at = |text: &str| Gates::new(Version::parse(text));
        let unknown = Gates::new(None);
        // reviewer_username: 13.8.
        assert!(!at("13.7.9").reviewer_filter());
        assert!(at("13.8.0").reviewer_filter());
        assert!(!unknown.reviewer_filter());
        // draft: 14.0.
        assert!(!at("13.12.15").draft_field());
        assert!(at("14.0.0").draft_field());
        assert!(!unknown.draft_field());
        // personal_access_tokens/self: 15.5; unknown asks.
        assert!(!at("15.4.6").token_self());
        assert!(at("15.5.0").token_self());
        assert!(unknown.token_self());
        // pipeline name: 15.11.
        assert!(!at("15.10.8").pipeline_name());
        assert!(at("16.0.1").pipeline_name());
        assert!(!unknown.pipeline_name());
    }
}
