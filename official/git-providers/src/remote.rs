//! Matching a local git remote URL with a provider's clone URLs
//! (`resolve-remote`, design §9.1): the record is returned only when one
//! of its clone URLs is the remote, after normalization. The remote's
//! user name or password never leaves this module.

/// A remote URL in one form: `host[:port]/path` in lower-case host,
/// without `.git`, a user name, a password or a trailing `/`. `https://`,
/// `ssh://` and the scp form `git@host:path` compare equal when they name
/// the same host and path (an ssh port is kept: `ssh://git@host:2222/p`
/// is another server address than `https://host/p` only by port, so it is
/// compared without it, as git hosts serve both).
pub fn normalize(url: &str) -> Option<String> {
    let url = url.trim();
    let (host, path) = if let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("ssh://"))
    {
        let (authority, path) = rest.split_once('/')?;
        let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        let host = host.split(':').next()?;
        (host, path)
    } else if url.contains("://") {
        return None;
    } else {
        // scp form: [user@]host:path
        let (authority, path) = url.split_once(':')?;
        if path.starts_with("//") {
            return None;
        }
        let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        (host, path)
    };
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    if host.is_empty()
        || path.is_empty()
        || path.contains(['?', '#', '\\', ' '])
        || path
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..")
        || !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '[' | ']'))
    {
        return None;
    }
    Some(format!("{}/{path}", host.to_ascii_lowercase()))
}

/// Whether `remote` is one of the clone URLs.
pub fn matches(remote: &str, clone_urls: &[Option<&str>]) -> bool {
    let Some(remote) = normalize(remote) else {
        return false;
    };
    clone_urls
        .iter()
        .flatten()
        .filter_map(|u| normalize(u))
        .any(|u| u == remote)
}

/// The path part of a remote (`group/sub/project`), to look it up.
pub fn path_of(remote: &str) -> Option<String> {
    normalize(remote).and_then(|n| n.split_once('/').map(|(_, p)| p.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_forms_compare_equal() {
        let want = Some("code.example.com/group/sub/proj".to_owned());
        for url in [
            "https://code.example.com/group/sub/proj.git",
            "https://Code.Example.com/group/sub/proj/",
            "https://user:fake-password@code.example.com/group/sub/proj.git",
            "ssh://git@code.example.com:2222/group/sub/proj.git",
            "git@code.example.com:group/sub/proj.git",
            "code.example.com:group/sub/proj",
        ] {
            assert_eq!(normalize(url), want, "{url}");
        }
        for bad in [
            "",
            "http://code.example.com/a/b",
            "file:///srv/repo",
            "https://code.example.com/",
            "https://code.example.com/a/../b",
            "git@code.example.com:a/b?x=1",
            "/local/path",
        ] {
            assert_eq!(normalize(bad), None, "{bad}");
        }
        assert!(matches(
            "git@gitlab.example.com:team/app.git",
            &[
                Some("https://gitlab.example.com/team/app.git"),
                Some("ssh://git@gitlab.example.com:2222/team/app.git")
            ]
        ));
        assert!(!matches(
            "git@gitlab.example.com:team/app.git",
            &[Some("https://gitlab.example.com/team/other.git"), None]
        ));
        assert_eq!(
            path_of("git@gitlab.example.com:team/app.git").as_deref(),
            Some("team/app")
        );
    }
}
