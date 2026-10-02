//! Where the CLI keeps site credentials and how a site is chosen for a run.
//!
//! Resolution order for the site and key, highest priority first:
//!
//! 1. `--site` / `--api-key` flags
//! 2. `GEEKCLI_SITE` / `GEEKCLI_API_KEY` environment variables
//! 3. the config file (`~/.config/geekcli/config.toml`), using `default_site`
//!    when only one site is stored or a default has been set
//!
//! `GEEKCLI_BASE_URL` (or `--base-url`) overrides the scheme/host the API is
//! reached at, which is how you point the CLI at a local dev server.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub const CONFIG_DIR_ENV: &str = "GEEKCLI_CONFIG_DIR";

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_site: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sites: BTreeMap<String, SiteConfig>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SiteConfig {
    pub api_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

pub fn config_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var(CONFIG_DIR_ENV) {
        if !dir.is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }
    // ~/.config on every platform (honouring XDG_CONFIG_HOME), like most
    // developer CLIs, rather than macOS's Library/Application Support.
    let base = match std::env::var("XDG_CONFIG_HOME") {
        Ok(xdg) if !xdg.is_empty() => PathBuf::from(xdg),
        _ => dirs::home_dir()
            .ok_or_else(|| Error::Config("cannot determine the home directory".into()))?
            .join(".config"),
    };
    Ok(migrate_legacy_dir(&base))
}

/// The CLI was called `realgeeks` before it was `geekcli`. Move a config left
/// under the old name the first time the new one is looked for, and keep using
/// the old directory if the move fails rather than losing the stored keys.
fn migrate_legacy_dir(base: &Path) -> PathBuf {
    let dir = base.join("geekcli");
    let legacy = base.join("realgeeks");
    if !dir.exists() && legacy.join("config.toml").is_file() && fs::rename(&legacy, &dir).is_err() {
        return legacy;
    }
    dir
}

pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(path)
            .map_err(|e| Error::Config(format!("cannot read {}: {e}", path.display())))?;
        toml::from_str(&raw)
            .map_err(|e| Error::Config(format!("cannot parse {}: {e}", path.display())))
    }

    pub fn save(&self) -> Result<PathBuf> {
        let path = config_path()?;
        self.save_to(&path)?;
        Ok(path)
    }

    /// Write the file atomically and privately: a new 0600 file next to it,
    /// renamed over the old one. The key is never readable by other users,
    /// a symlink planted at the path is replaced rather than followed, and a
    /// crash mid-write leaves the previous file intact.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        let raw = toml::to_string_pretty(self)
            .map_err(|e| Error::Config(format!("cannot serialize config: {e}")))?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        create_private_dir(parent)
            .map_err(|e| Error::Config(format!("cannot create {}: {e}", parent.display())))?;
        let tmp = parent.join(format!(
            ".config.toml.{}.{}.tmp",
            std::process::id(),
            rand::random::<u32>()
        ));
        let written = write_private(&tmp, raw.as_bytes()).and_then(|()| fs::rename(&tmp, path));
        if let Err(e) = written {
            let _ = fs::remove_file(&tmp);
            return Err(Error::Config(format!("cannot write {}: {e}", path.display())));
        }
        Ok(())
    }

    /// Store a key for a site, making it the default when it is the first.
    pub fn put_site(&mut self, domain: &str, site: SiteConfig) {
        let domain = normalize_domain(domain);
        if self.sites.is_empty() || self.default_site.is_none() {
            self.default_site = Some(domain.clone());
        }
        self.sites.insert(domain, site);
    }

    pub fn remove_site(&mut self, domain: &str) -> bool {
        let domain = normalize_domain(domain);
        let removed = self.sites.remove(&domain).is_some();
        if self.default_site.as_deref() == Some(domain.as_str()) {
            self.default_site = self.sites.keys().next().cloned();
        }
        removed
    }

    pub fn site(&self, domain: &str) -> Option<&SiteConfig> {
        self.sites.get(&normalize_domain(domain))
    }
}

/// Create a new file readable only by its owner and write `bytes` to it.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// `create_dir_all`, with new directories private to the owner on Unix.
/// (On Windows the user profile's ACL already keeps others out.)
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)
}

/// Accept `www.example.com`, `https://www.example.com/`, `example.com/blog`
/// and always keep just the host.
pub fn normalize_domain(input: &str) -> String {
    let trimmed = input.trim();
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    without_scheme
        .split('/')
        .next()
        .unwrap_or_default()
        .trim_end_matches('.')
        .to_ascii_lowercase()
}

/// Everything a command needs to talk to one site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub domain: String,
    pub base_url: String,
    pub api_key: Option<String>,
}

impl Target {
    pub fn api_root(&self) -> String {
        format!("{}/api/v3/", self.base_url.trim_end_matches('/'))
    }
}

/// Inputs to site resolution; flags and env are read by the caller so this
/// stays testable.
#[derive(Debug, Default, Clone)]
pub struct Overrides {
    pub site: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
}

pub fn resolve_target(config: &Config, overrides: &Overrides) -> Result<Target> {
    let domain = overrides
        .site
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(normalize_domain)
        .or_else(|| config.default_site.clone())
        .or_else(|| {
            if config.sites.len() == 1 {
                config.sites.keys().next().cloned()
            } else {
                None
            }
        });

    let Some(domain) = domain else {
        // A base URL alone (local dev) still needs a domain for messages.
        if let Some(base) = &overrides.base_url {
            check_transport(base)?;
            let domain = normalize_domain(base);
            return Ok(Target {
                domain,
                base_url: base.trim_end_matches('/').to_string(),
                api_key: overrides.api_key.clone(),
            });
        }
        return Err(Error::Config(
            "no site selected. Pass --site <domain>, set GEEKCLI_SITE, or run `geekcli auth login`"
                .into(),
        ));
    };

    let stored = config.site(&domain);
    let stored_base = stored.and_then(|s| s.base_url.clone());
    let base_url = overrides
        .base_url
        .clone()
        .or_else(|| stored_base.clone())
        .unwrap_or_else(|| format!("https://{domain}"));
    check_transport(&base_url)?;
    let override_key = overrides
        .api_key
        .clone()
        .filter(|k| !k.trim().is_empty());
    // A stored key only goes to the server it was stored for. A --base-url or
    // a GEEKCLI_BASE_URL left over in the shell must bring its own key.
    if let (None, Some(_), Some(base)) = (&override_key, stored, &overrides.base_url) {
        let home = stored_base.unwrap_or_else(|| format!("https://{domain}"));
        if normalize_domain(base) != normalize_domain(&home) {
            return Err(Error::Config(format!(
                "not sending the stored key for {domain} to {base}; pass --api-key (or GEEKCLI_API_KEY) for that server, or unset GEEKCLI_BASE_URL"
            )));
        }
    }
    let api_key = override_key.or_else(|| stored.map(|s| s.api_key.clone()));

    Ok(Target {
        domain,
        base_url: base_url.trim_end_matches('/').to_string(),
        api_key,
    })
}

/// Keys and login codes only travel over https, except to a local dev host
/// (localhost, a loopback address, `*.localhost`, `*.local` or `*.test`).
pub fn check_transport(base_url: &str) -> Result<()> {
    let url = url::Url::parse(base_url)
        .map_err(|e| Error::Config(format!("invalid base URL {base_url}: {e}")))?;
    match url.scheme() {
        "https" => Ok(()),
        "http" if url.host().is_some_and(|h| is_local_host(&h)) => Ok(()),
        "http" => Err(Error::Config(format!(
            "refusing to send an API key over plain http to {base_url}; use https (plain http is only allowed for localhost, *.local and *.test)"
        ))),
        other => Err(Error::Config(format!(
            "unsupported scheme {other}: in {base_url}; use https://"
        ))),
    }
}

fn is_local_host(host: &url::Host<&str>) -> bool {
    match host {
        url::Host::Ipv4(ip) => ip.is_loopback(),
        url::Host::Ipv6(ip) => ip.is_loopback(),
        url::Host::Domain(name) => {
            let name = name.to_ascii_lowercase();
            name == "localhost"
                || [".localhost", ".local", ".test"]
                    .iter()
                    .any(|suffix| name.ends_with(suffix))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_http_only_for_local_hosts() {
        for ok in [
            "https://www.example.com",
            "http://localhost:8000",
            "http://127.0.0.1:9000",
            "http://[::1]:8000",
            "http://www.cypress.local",
            "http://site.test",
            "http://api.localhost",
        ] {
            assert!(check_transport(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://www.example.com",
            "http://10.0.0.5",
            "ftp://www.example.com",
            "http://local.example.com",
        ] {
            assert!(check_transport(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_stored_key_is_not_sent_to_another_base_url() {
        let mut config = Config::default();
        config.put_site(
            "www.a.com",
            SiteConfig {
                api_key: "rg_live_a".into(),
                ..Default::default()
            },
        );
        let elsewhere = Overrides {
            site: Some("www.a.com".into()),
            base_url: Some("https://staging.example.net".into()),
            ..Default::default()
        };
        assert!(resolve_target(&config, &elsewhere).is_err());

        let with_key = Overrides {
            api_key: Some("rg_live_staging".into()),
            ..elsewhere
        };
        assert_eq!(
            resolve_target(&config, &with_key).ok().and_then(|t| t.api_key),
            Some("rg_live_staging".into())
        );

        let same = Overrides {
            site: Some("www.a.com".into()),
            base_url: Some("https://www.a.com/".into()),
            ..Default::default()
        };
        assert!(resolve_target(&config, &same).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn saves_privately_and_replaces_a_planted_symlink() -> std::io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let base = tempfile::tempdir()?;
        let dir = base.path().join("geekcli");
        let path = dir.join("config.toml");
        let decoy = base.path().join("decoy");
        fs::write(&decoy, "untouched")?;
        fs::create_dir(&dir)?;
        std::os::unix::fs::symlink(&decoy, &path)?;

        let mut config = Config::default();
        config.put_site(
            "www.a.com",
            SiteConfig {
                api_key: "rg_live_a".into(),
                ..Default::default()
            },
        );
        assert!(config.save_to(&path).is_ok());

        assert_eq!(fs::read_to_string(&decoy)?, "untouched");
        assert!(!fs::symlink_metadata(&path)?.file_type().is_symlink());
        assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
        assert_eq!(Config::load_from(&path).ok(), Some(config));

        let fresh = base.path().join("new/geekcli/config.toml");
        assert!(Config::default().save_to(&fresh).is_ok());
        let dir_mode = fs::metadata(base.path().join("new/geekcli"))?.permissions().mode();
        assert_eq!(dir_mode & 0o777, 0o700);
        Ok(())
    }

    #[test]
    fn moves_a_config_left_under_the_old_name() -> std::io::Result<()> {
        let base = tempfile::tempdir()?;
        fs::create_dir(base.path().join("realgeeks"))?;
        fs::write(
            base.path().join("realgeeks/config.toml"),
            "default_site = \"a\"\n",
        )?;

        let dir = migrate_legacy_dir(base.path());

        assert_eq!(dir, base.path().join("geekcli"));
        assert!(dir.join("config.toml").is_file());
        assert!(!base.path().join("realgeeks").exists());
        Ok(())
    }

    #[test]
    fn leaves_the_old_dir_alone_once_the_new_one_exists() -> std::io::Result<()> {
        let base = tempfile::tempdir()?;
        fs::create_dir_all(base.path().join("realgeeks"))?;
        fs::write(base.path().join("realgeeks/config.toml"), "")?;
        fs::create_dir(base.path().join("geekcli"))?;

        assert_eq!(migrate_legacy_dir(base.path()), base.path().join("geekcli"));
        assert!(base.path().join("realgeeks/config.toml").is_file());
        Ok(())
    }

    #[test]
    fn normalizes_domains() {
        assert_eq!(
            normalize_domain("https://WWW.Example.com/blog/"),
            "www.example.com"
        );
        assert_eq!(normalize_domain(" example.com "), "example.com");
        assert_eq!(normalize_domain("http://localhost:8000"), "localhost:8000");
    }

    #[test]
    fn resolves_single_stored_site_by_default() {
        let mut config = Config::default();
        config.sites.insert(
            "www.a.com".into(),
            SiteConfig {
                api_key: "rg_live_a".into(),
                ..Default::default()
            },
        );
        let target = resolve_target(&config, &Overrides::default()).ok();
        assert_eq!(
            target,
            Some(Target {
                domain: "www.a.com".into(),
                base_url: "https://www.a.com".into(),
                api_key: Some("rg_live_a".into()),
            })
        );
    }

    #[test]
    fn flag_key_beats_stored_key() {
        let mut config = Config::default();
        config.put_site(
            "www.a.com",
            SiteConfig {
                api_key: "stored".into(),
                ..Default::default()
            },
        );
        let overrides = Overrides {
            api_key: Some("flag".into()),
            ..Default::default()
        };
        let target = resolve_target(&config, &overrides).ok();
        assert_eq!(target.and_then(|t| t.api_key), Some("flag".into()));
    }

    #[test]
    fn no_site_is_a_config_error() {
        let err = resolve_target(&Config::default(), &Overrides::default()).err();
        assert!(matches!(err, Some(Error::Config(_))));
    }

    #[test]
    fn removing_default_site_picks_another() {
        let mut config = Config::default();
        config.put_site("a.com", SiteConfig::default());
        config.put_site("b.com", SiteConfig::default());
        assert_eq!(config.default_site.as_deref(), Some("a.com"));
        assert!(config.remove_site("a.com"));
        assert_eq!(config.default_site.as_deref(), Some("b.com"));
    }

    #[test]
    fn round_trips_through_toml() {
        let mut config = Config::default();
        config.put_site(
            "a.com",
            SiteConfig {
                api_key: "rg_live_x".into(),
                scopes: vec!["blog:write".into()],
                ..Default::default()
            },
        );
        let dir = tempfile::tempdir().ok();
        let Some(dir) = dir else { return };
        let path = dir.path().join("config.toml");
        assert!(config.save_to(&path).is_ok());
        assert_eq!(Config::load_from(&path).ok(), Some(config));
    }
}
