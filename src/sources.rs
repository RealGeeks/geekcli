//! Which local files geekcli will read and send to a site.
//!
//! Bodies (`--body-file`, `--data @file`, `api -d @file`) and uploads read
//! whatever path they are given and send it to the site, where it may end up
//! public. An agent talked into "uploading" `~/.ssh/id_rsa` would publish a
//! private key. So hidden files, files inside hidden directories of the home
//! directory (dotfiles, `~/.ssh`, `~/.aws`, …), well-known credential
//! directories and geekcli's own config file are refused unless the run says
//! otherwise with `--allow-any-source` or `GEEKCLI_ALLOW_ANY_SOURCE=1`.

use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::config;
use crate::error::{Error, Result};

pub const ENV: &str = "GEEKCLI_ALLOW_ANY_SOURCE";
pub const FLAG: &str = "--allow-any-source";

/// Paths under the home directory that hold credentials. Most are hidden
/// anyway; naming them gives a clearer error.
const CREDENTIAL_PATHS: &[&str] = &[
    ".ssh",
    ".aws",
    ".config/gcloud",
    ".gnupg",
    ".kube",
    ".docker",
    ".azure",
    ".netrc",
    ".git-credentials",
    ".config/gh",
];

static ALLOW_ANY: AtomicBool = AtomicBool::new(false);

/// Set once from the global flag (or its environment variable).
pub fn set_allow_any(allow: bool) {
    ALLOW_ANY.store(allow, Ordering::Relaxed);
}

pub fn allow_any() -> bool {
    ALLOW_ANY.load(Ordering::Relaxed)
}

/// Refuse to read `path` when it looks like a secret, unless overridden.
/// A path that does not exist passes: reading it reports that.
pub fn check(path: &Path) -> Result<()> {
    match why_refused(path) {
        Some(reason) => Err(Error::Usage(format!(
            "refusing to read {}: {reason}, and what geekcli reads is sent to the site. {}",
            path.display(),
            override_hint()
        ))),
        None => Ok(()),
    }
}

/// The reason `path` is refused this run, if it is.
pub fn why_refused(path: &Path) -> Option<String> {
    if allow_any() {
        return None;
    }
    let home = dirs::home_dir();
    let config_file = config::config_path().ok();
    refusal(path, home.as_deref(), config_file.as_deref())
}

pub fn override_hint() -> String {
    format!("If you really mean to send it, pass {FLAG} (or set {ENV}=1)")
}

/// Why `path` must not be read, if it must not. Both the path as given and
/// where it leads (symlinks resolved) are checked, so a link in an ordinary
/// folder does not reach into `~/.ssh`.
fn refusal(path: &Path, home: Option<&Path>, config_file: Option<&Path>) -> Option<String> {
    let given = absolute(path);
    let real = std::fs::canonicalize(path).ok();
    let home_real = home.and_then(|h| std::fs::canonicalize(h).ok());
    let config_real = config_file.and_then(|c| std::fs::canonicalize(c).ok());
    for candidate in std::iter::once(given).chain(real) {
        if config_file.is_some_and(|c| candidate == c)
            || config_real.as_deref().is_some_and(|c| candidate == c)
        {
            return Some("it is geekcli's config file, which holds your API keys".into());
        }
        if candidate
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with('.'))
        {
            return Some("it is a hidden file".into());
        }
        for home in [home, home_real.as_deref()].into_iter().flatten() {
            let Ok(inside) = candidate.strip_prefix(home) else {
                continue;
            };
            if let Some(dir) = CREDENTIAL_PATHS
                .iter()
                .find(|dir| inside.starts_with(Path::new(dir)))
            {
                return Some(format!("~/{dir} holds credentials"));
            }
            if let Some(hidden) = inside.components().find_map(|c| match c {
                Component::Normal(name) => name.to_str().filter(|n| n.starts_with('.')),
                _ => None,
            }) {
                return Some(format!(
                    "it is inside the hidden directory ~/{hidden}, where credentials and settings live"
                ));
            }
        }
    }
    None
}

/// The path made absolute against the working directory, with `.` and `..`
/// folded away lexically (symlinks are left to `canonicalize`).
fn absolute(path: &Path) -> PathBuf {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_or_else(|_| path.to_path_buf(), |cwd| cwd.join(path))
    };
    let mut out = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(path: &Path, home: &Path) -> Option<String> {
        refusal(
            path,
            Some(home),
            Some(&home.join(".config/geekcli/config.toml")),
        )
    }

    #[test]
    fn credentials_and_hidden_files_are_refused() {
        let home = Path::new("/home/agent");
        for (path, reason) in [
            ("/home/agent/.ssh/id_rsa", "~/.ssh holds credentials"),
            ("/home/agent/.aws/credentials", "~/.aws"),
            ("/home/agent/.config/gcloud/creds.json", "~/.config/gcloud"),
            ("/home/agent/.kube/config", "~/.kube"),
            ("/home/agent/.docker/config.json", "~/.docker"),
            ("/home/agent/.gnupg/secring.gpg", "~/.gnupg"),
            ("/home/agent/.config/geekcli/config.toml", "config file"),
            ("/home/agent/.zsh_history", "hidden file"),
            ("/srv/site/.env", "hidden file"),
            (
                "/home/agent/.local/share/notes.txt",
                "hidden directory ~/.local",
            ),
            ("/home/agent/work/../.ssh/id_ed25519", "~/.ssh"),
        ] {
            let why = refused(Path::new(path), home);
            assert!(
                why.as_deref().is_some_and(|w| w.contains(reason)),
                "{path}: {why:?}"
            );
        }
    }

    #[test]
    fn ordinary_files_are_read() {
        let home = Path::new("/home/agent");
        for path in [
            "/home/agent/work/post.md",
            "/home/agent/work/./images/hero.png",
            // hidden directories outside home (a temp dir) are fine
            "/tmp/.tmpAbC123/hero.png",
            "/var/folders/xy/T/.tmpQ/body.html",
        ] {
            assert_eq!(refused(Path::new(path), home), None, "{path}");
        }
    }

    #[test]
    fn the_config_file_is_refused_wherever_it_lives() {
        let config = Path::new("/srv/geekcli/config.toml");
        let why = refusal(config, Some(Path::new("/home/agent")), Some(config));
        assert!(why.is_some_and(|w| w.contains("API keys")));
        // its neighbours are not
        assert_eq!(
            refusal(
                Path::new("/srv/geekcli/hero.png"),
                Some(Path::new("/home/agent")),
                Some(config)
            ),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_into_a_credential_directory_is_refused() -> std::io::Result<()> {
        let home = tempfile::tempdir()?;
        let ssh = home.path().join(".ssh");
        std::fs::create_dir(&ssh)?;
        std::fs::write(ssh.join("id_rsa"), "secret")?;
        let work = tempfile::tempdir()?;
        let link = work.path().join("photo.png");
        std::os::unix::fs::symlink(ssh.join("id_rsa"), &link)?;
        let why = refusal(&link, Some(home.path()), None);
        assert!(why.is_some_and(|w| w.contains(".ssh")));
        Ok(())
    }
}
