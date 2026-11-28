//! `geekcli snapshot …` — render a page of the site with a locally
//! installed Chrome/Chromium and save a PNG, so an agent can look at what
//! it just published. Chrome is driven over the DevTools protocol, which
//! is what makes a real full-page capture possible: the viewport stays a
//! normal size (so `100vh` heroes stay one screen tall) while the whole
//! document is captured.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use clap::Args;
use headless_chrome::protocol::cdp::{Emulation, Page};
use headless_chrome::{Browser, LaunchOptions, Tab};
use serde_json::json;

use super::Context;
use crate::error::{Error, Result};

pub const BROWSER_ENV: &str = "GEEKCLI_BROWSER";

#[derive(Debug, Args)]
#[command(after_help = "Notes:
  - --full captures the whole document at a normal viewport, so a 100vh hero stays one screen tall.
  - Look for: text on borders, a card row wider than a phone, links that look like plain text, ? where a special character was, a class the theme does not style, the old agent's name or phone.
  - Drafts and scheduled posts 404 here (the browser has no login); publish first.
  - Themes without a phone breakpoint overflow at 390px on every page, not just yours; compare with an untouched page before changing content.")]
pub struct SnapshotArgs {
    /// Site-relative path (/, /blog/my-post/) or a full URL. Default: the home page
    #[arg(value_name = "PATH_OR_URL", default_value = "/")]
    pub target: String,
    /// Where to write the PNG (default: ./snapshot-<path>.png)
    #[arg(long, value_name = "FILE")]
    pub out: Option<PathBuf>,
    /// Viewport width in CSS pixels
    #[arg(long, default_value_t = 1440, value_name = "PX")]
    pub width: u32,
    /// Viewport height in CSS pixels
    #[arg(long, default_value_t = 900, value_name = "PX")]
    pub height: u32,
    /// Capture the whole page, not just the first screen
    #[arg(long)]
    pub full: bool,
    /// Capture the first element matching this CSS selector instead of the page
    #[arg(long, value_name = "CSS")]
    pub selector: Option<String>,
    /// Zero-based match to capture when --selector matches more than one element
    #[arg(long, default_value_t = 0, requires = "selector", value_name = "N")]
    pub nth: usize,
    /// Phone emulation (390x844, touch, device scale 2); not with --width/--height/--scale
    #[arg(long, conflicts_with_all = ["width", "height", "scale"])]
    pub mobile: bool,
    /// Device scale factor; 2 doubles the pixel size for sharper text
    #[arg(long, default_value_t = 1, value_name = "N")]
    pub scale: u8,
    /// Give scripts, fonts and lazy images this long to settle before capturing
    #[arg(long, default_value_t = 1500, value_name = "MS")]
    pub wait: u32,
    /// Chrome/Chromium binary (or set GEEKCLI_BROWSER); auto-detected otherwise
    #[arg(long, env = BROWSER_ENV, value_name = "PATH")]
    pub browser: Option<PathBuf>,
}

pub fn run(ctx: &Context, args: &SnapshotArgs) -> Result<()> {
    let url = resolve_url(&ctx.client, &args.target);
    let (width, height, scale) = if args.mobile {
        (390, 844, 2)
    } else {
        (args.width, args.height, args.scale.max(1))
    };
    let out = args.out.clone().unwrap_or_else(|| default_out(&url));
    let browser_path = find_browser(args.browser.as_deref())?;

    if ctx.printer.format == crate::output::Format::Table {
        ctx.printer
            .note(&format!("Rendering {url} with {}", browser_path.display()));
    }
    let capture = Capture {
        url: &url,
        width,
        height,
        scale,
        mobile: args.mobile,
        full: args.full,
        wait_ms: u64::from(args.wait),
    };
    let png = render(&browser_path, &capture, args.selector.as_deref(), args.nth)?;
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&out, &png)?;

    let doc = json!({
        "path": out.display().to_string(),
        "url": url,
        "width": width,
        "height": height,
        "scale": scale,
        "full": args.full,
        "selector": args.selector,
        "nth": args.selector.as_ref().map(|_| args.nth),
        "bytes": png.len(),
        "browser": browser_path.display().to_string(),
    });
    if ctx.printer.quiet {
        println!("{}", out.display());
        return Ok(());
    }
    ctx.printer.raw(&doc)
}

pub struct Capture<'a> {
    pub url: &'a str,
    pub width: u32,
    pub height: u32,
    pub scale: u8,
    pub mobile: bool,
    pub full: bool,
    pub wait_ms: u64,
}

fn cdp<T>(result: std::result::Result<T, anyhow::Error>, what: &str) -> Result<T> {
    result.map_err(|e| Error::Other(format!("{what}: {e}")))
}

/// Drive Chrome: open the page, let it settle and scroll once end to end so
/// lazy images and reveal-on-scroll sections render before `operation` runs.
pub fn visit<T>(
    browser_path: &Path,
    capture: &Capture<'_>,
    operation: impl FnOnce(&Tab) -> Result<T>,
) -> Result<T> {
    let options = LaunchOptions::default_builder()
        .path(Some(browser_path.to_path_buf()))
        .headless(true)
        .window_size(Some((capture.width, capture.height)))
        .args(vec![
            OsStr::new("--hide-scrollbars"),
            OsStr::new("--disable-extensions"),
        ])
        .idle_browser_timeout(Duration::from_secs(120))
        // headless_chrome passes --ignore-certificate-errors unless told not
        // to; a page an agent reads or asserts on must come from the real site
        .ignore_certificate_errors(false)
        .build()
        .map_err(|e| Error::Other(format!("cannot configure the browser: {e}")))?;
    let browser = cdp(Browser::new(options), "cannot start the browser")?;
    let tab = cdp(browser.new_tab(), "cannot open a tab")?;

    cdp(
        tab.call_method(Emulation::SetDeviceMetricsOverride {
            width: capture.width,
            height: capture.height,
            device_scale_factor: f64::from(capture.scale),
            mobile: capture.mobile,
            scale: None,
            screen_width: None,
            screen_height: None,
            position_x: None,
            position_y: None,
            dont_set_visible_size: None,
            screen_orientation: None,
            viewport: None,
            display_feature: None,
            device_posture: None,
        }),
        "cannot set the viewport",
    )?;
    if capture.mobile {
        cdp(
            tab.call_method(Emulation::SetTouchEmulationEnabled {
                enabled: true,
                max_touch_points: Some(5),
            }),
            "cannot enable touch",
        )?;
    }

    cdp(tab.navigate_to(capture.url), "cannot open the page")?;
    cdp(
        tab.wait_until_navigated(),
        "the page did not finish loading",
    )?;
    thread::sleep(Duration::from_millis(capture.wait_ms));

    // Walk the page so lazy-loaded images and reveal-on-scroll sections render.
    let scroll = r"(async () => {
        const step = Math.max(200, window.innerHeight / 2);
        const total = document.documentElement.scrollHeight;
        for (let y = 0; y <= total; y += step) { window.scrollTo(0, y); await new Promise(r => setTimeout(r, 60)); }
        window.scrollTo(0, 0);
        await new Promise(r => setTimeout(r, 300));
        return document.documentElement.scrollHeight;
    })()";
    let _ = tab.evaluate(scroll, true);
    thread::sleep(Duration::from_millis(300));

    operation(&tab)
}

/// Capture the viewport, whole document, or a selector's border box.
pub fn render(
    browser_path: &Path,
    capture: &Capture<'_>,
    selector: Option<&str>,
    nth: usize,
) -> Result<Vec<u8>> {
    visit(browser_path, capture, |tab| {
        let clip = if let Some(selector) = selector {
            let elements = tab.find_elements(selector).map_err(|error| {
                if is_no_element_found(&error) {
                    Error::Other(format!("no element matches selector `{selector}`"))
                } else {
                    Error::Other(format!("browser query failed: {error}"))
                }
            })?;
            let Some(element) = elements.get(nth) else {
                return Err(Error::Other(format!(
                    "selector `{selector}` matched {} element(s); --nth {nth} is out of range",
                    elements.len()
                )));
            };
            Some(cdp(element.get_box_model(), "cannot get the element bounds")?.border_viewport())
        } else {
            None
        };
        let result = cdp(
            tab.call_method(Page::CaptureScreenshot {
                format: Some(Page::CaptureScreenshotFormatOption::Png),
                quality: None,
                clip,
                from_surface: Some(true),
                capture_beyond_viewport: Some(capture.full || selector.is_some()),
                optimize_for_speed: None,
            }),
            "the browser could not capture the page",
        )?;
        STANDARD
            .decode(result.data)
            .map_err(|e| Error::Other(format!("bad screenshot data from the browser: {e}")))
    })
}

/// A site-relative path becomes an absolute URL on the current site. A full
/// URL is accepted in any letter case and comes back normalized
/// (`HTTPS://…` → `https://…`).
pub fn resolve_url(client: &crate::client::Client, target: &str) -> String {
    let target = target.trim();
    let lower = target.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return url::Url::parse(target).map_or_else(|_| target.to_string(), String::from);
    }
    client.site_url(target)
}

/// `/blog/my-post/` → `snapshot-blog-my-post.png`; `/` → `snapshot-home.png`.
pub fn default_out(target: &str) -> PathBuf {
    let trimmed = target.trim();
    let parsed = url::Url::parse(trimmed)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https"));
    let raw_path = match &parsed {
        Some(url) => url.path(),
        None => trimmed.split(['?', '#']).next().unwrap_or_default(),
    };
    // `Url::path()` is percent-encoded; slug the characters it stands for.
    let raw_path = percent_encoding::percent_decode_str(raw_path).decode_utf8_lossy();
    let slug: String = raw_path
        .trim_matches('/')
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let slug = if slug.is_empty() {
        "home".to_string()
    } else {
        slug
    };
    PathBuf::from(format!("snapshot-{slug}.png"))
}

/// The first Chrome-family browser we can find.
pub fn find_browser(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        if path.exists() {
            return Ok(path.to_path_buf());
        }
        if let Some(found) = on_path(&path.display().to_string()) {
            return Ok(found);
        }
        return Err(Error::Config(format!(
            "browser not found: {}",
            path.display()
        )));
    }
    for candidate in candidates() {
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    for name in [
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
        "chrome",
        "brave-browser",
        "microsoft-edge",
    ] {
        if let Some(found) = on_path(name) {
            return Ok(found);
        }
    }
    Err(Error::Config(
        "no Chrome or Chromium found. Install Google Chrome, or point --browser / GEEKCLI_BROWSER at a Chromium binary (Playwright's works too)".into(),
    ))
}

fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let home = dirs::home_dir().unwrap_or_default();
    #[cfg(target_os = "macos")]
    {
        for app in [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        ] {
            out.push(PathBuf::from(app));
            out.push(home.join(app.trim_start_matches('/')));
        }
        out.extend(playwright_chromium(
            &home.join("Library/Caches/ms-playwright"),
            "chrome-mac/Chromium.app/Contents/MacOS/Chromium",
        ));
        out.extend(playwright_chromium(
            &home.join("Library/Caches/ms-playwright"),
            "chrome-mac-arm64/Chromium.app/Contents/MacOS/Chromium",
        ));
    }
    #[cfg(target_os = "linux")]
    {
        for bin in [
            "/usr/bin/google-chrome",
            "/usr/bin/google-chrome-stable",
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
            "/snap/bin/chromium",
        ] {
            out.push(PathBuf::from(bin));
        }
        out.extend(playwright_chromium(
            &home.join(".cache/ms-playwright"),
            "chrome-linux/chrome",
        ));
    }
    #[cfg(target_os = "windows")]
    {
        for bin in [
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        ] {
            out.push(PathBuf::from(bin));
        }
        let _ = &home;
    }
    out
}

/// Playwright keeps `chromium-<build>/…`; newest build first.
fn playwright_chromium(cache: &Path, suffix: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(cache) else {
        return vec![];
    };
    let mut builds: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("chromium-"))
        })
        .collect();
    builds.sort();
    builds.reverse();
    builds.into_iter().map(|b| b.join(suffix)).collect()
}

pub(crate) fn is_no_element_found(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<headless_chrome::browser::tab::NoElementFound>()
        .is_some()
}

fn parse_pathext(raw: Option<&str>) -> Vec<String> {
    let exts: Vec<String> = raw
        .unwrap_or_default()
        .split(';')
        .map(|ext| ext.trim().trim_start_matches('.'))
        .filter(|ext| !ext.is_empty())
        .map(str::to_string)
        .collect();
    if exts.is_empty() {
        vec!["EXE".to_string()]
    } else {
        exts
    }
}

fn path_candidates(dir: &Path, name: &str, pathext: Option<&str>) -> Vec<PathBuf> {
    if Path::new(name).extension().is_none() {
        if let Some(raw) = pathext {
            return parse_pathext(Some(raw))
                .into_iter()
                .map(|ext| dir.join(format!("{name}.{ext}")))
                .collect();
        }
    }
    vec![dir.join(name)]
}

fn find_on_dirs(dirs: &[PathBuf], name: &str, pathext: Option<&str>) -> Option<PathBuf> {
    if let Some(found) = dirs
        .iter()
        .flat_map(|dir| path_candidates(dir, name, pathext))
        .find(|p| p.is_file())
    {
        return Some(found);
    }
    if pathext.is_some() && Path::new(name).extension().is_none() {
        return dirs.iter().map(|dir| dir.join(name)).find(|p| p.is_file());
    }
    None
}

fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    let pathext = if cfg!(windows) {
        Some(std::env::var("PATHEXT").unwrap_or_default())
    } else {
        None
    };
    find_on_dirs(&dirs, name, pathext.as_deref())
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;

    use super::*;
    use crate::cli::Cli;
    use crate::client::Client;
    use crate::config::Target;

    #[test]
    fn default_output_names() {
        assert_eq!(default_out("/"), PathBuf::from("snapshot-home.png"));
        assert_eq!(
            default_out("/blog/my-post/"),
            PathBuf::from("snapshot-blog-my-post.png")
        );
        assert_eq!(
            default_out("https://x.com/buying/?a=1"),
            PathBuf::from("snapshot-buying.png")
        );
        assert_eq!(
            default_out("HTTPS://x.com/buying/?a=1"),
            PathBuf::from("snapshot-buying.png")
        );
        assert_eq!(
            default_out("https://x.com?next=/buying"),
            PathBuf::from("snapshot-home.png")
        );
        assert_eq!(default_out("buying"), PathBuf::from("snapshot-buying.png"));
        // A resolved URL's path is percent-encoded; the slug must not show it.
        assert_eq!(
            default_out("https://x.com/search%20results/"),
            PathBuf::from("snapshot-search-results.png")
        );
        assert_eq!(
            default_out("https://x.com/blog/caf%C3%A9/"),
            PathBuf::from("snapshot-blog-caf.png")
        );
        assert_eq!(
            default_out("blog/my-post"),
            PathBuf::from("snapshot-blog-my-post.png")
        );
        assert_eq!(
            default_out("buying?a=1#frag"),
            PathBuf::from("snapshot-buying.png")
        );
    }

    #[test]
    fn resolve_url_is_case_insensitive_for_http_schemes() -> Result<()> {
        let client = Client::new(
            &Target {
                domain: "www.example.com".into(),
                base_url: "https://www.example.com".into(),
                api_key: None,
            },
            0,
            false,
        )?;
        // Accepted in any case, printed and visited with the scheme lowercased.
        assert_eq!(
            resolve_url(&client, "HTTPS://example.com/a"),
            "https://example.com/a"
        );
        assert_eq!(
            resolve_url(&client, "Http://example.com/a"),
            "http://example.com/a"
        );
        assert_eq!(
            resolve_url(&client, "/buying"),
            "https://www.example.com/buying"
        );
        Ok(())
    }

    #[test]
    fn path_candidates_handles_windows_pathext() {
        let dir = Path::new("/bin");
        assert_eq!(
            path_candidates(dir, "chrome", None),
            vec![PathBuf::from("/bin/chrome")]
        );
        assert_eq!(
            path_candidates(dir, "chrome", Some(".EXE;; .CMD ;.BAT")),
            vec![
                PathBuf::from("/bin/chrome.EXE"),
                PathBuf::from("/bin/chrome.CMD"),
                PathBuf::from("/bin/chrome.BAT"),
            ]
        );
        assert_eq!(
            path_candidates(dir, "chrome", Some(";")),
            vec![PathBuf::from("/bin/chrome.EXE")]
        );
        assert_eq!(
            path_candidates(dir, "chrome", Some(" ; ; . ")),
            vec![PathBuf::from("/bin/chrome.EXE")]
        );
        assert_eq!(
            path_candidates(dir, "chrome.exe", Some(".EXE;.CMD")),
            vec![PathBuf::from("/bin/chrome.exe")]
        );
    }

    #[test]
    fn find_on_dirs_prefers_pathext_across_path_over_earlier_extensionless_shim(
    ) -> std::io::Result<()> {
        let root = tempfile::tempdir()?;
        let dir1 = root.path().join("dir1");
        let dir2 = root.path().join("dir2");
        std::fs::create_dir(&dir1)?;
        std::fs::create_dir(&dir2)?;
        std::fs::write(dir1.join("chrome"), b"")?;
        std::fs::write(dir2.join("chrome.EXE"), b"")?;

        let search_dirs = [dir1.clone(), dir2.clone()];
        assert_eq!(
            find_on_dirs(&search_dirs, "chrome", Some(".EXE;.CMD")),
            Some(dir2.join("chrome.EXE"))
        );
        assert_eq!(
            find_on_dirs(&search_dirs, "chrome", Some(";")),
            Some(dir2.join("chrome.EXE"))
        );
        assert_eq!(
            find_on_dirs(std::slice::from_ref(&dir1), "chrome", Some(".EXE")),
            Some(dir1.join("chrome"))
        );
        assert_eq!(
            find_on_dirs(&search_dirs, "chrome", None),
            Some(dir1.join("chrome"))
        );
        Ok(())
    }

    #[test]
    fn is_no_element_found_distinguishes_missing_from_other_errors() {
        let missing = anyhow::Error::new(headless_chrome::browser::tab::NoElementFound {});
        let invalid = anyhow::anyhow!("DOM Error while querying");
        assert!(is_no_element_found(&missing));
        assert!(!is_no_element_found(&invalid));
    }

    #[test]
    fn mobile_conflicts_with_scale() {
        assert!(Cli::try_parse_from(["geekcli", "snapshot", "--mobile", "--scale", "1"]).is_err());
    }

    #[test]
    fn explicit_missing_browser_is_a_config_error() {
        assert!(matches!(
            find_browser(Some(Path::new("/nope/chrome"))),
            Err(Error::Config(_))
        ));
    }
}
