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
  - Drafts 404 for the browser; publish first.
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
    /// Phone emulation (390x844, touch, device scale 2)
    #[arg(long, conflicts_with_all = ["width", "height"])]
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
    let out = args
        .out
        .clone()
        .unwrap_or_else(|| default_out(&args.target));
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
        .idle_browser_timeout(Duration::from_mins(2))
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
            let elements = tab
                .find_elements(selector)
                .map_err(|_| Error::Other(format!("no element matches selector `{selector}`")))?;
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

/// A site-relative path becomes an absolute URL on the current site.
pub fn resolve_url(client: &crate::client::Client, target: &str) -> String {
    let target = target.trim();
    if target.starts_with("http://") || target.starts_with("https://") {
        return target.to_string();
    }
    client.site_url(target)
}

/// `/blog/my-post/` → `snapshot-blog-my-post.png`; `/` → `snapshot-home.png`.
pub fn default_out(target: &str) -> PathBuf {
    let path = target
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let path = path.find('/').map_or("", |i| &path[i..]);
    let slug: String = path
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
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

fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }

    #[test]
    fn explicit_missing_browser_is_a_config_error() {
        assert!(matches!(
            find_browser(Some(Path::new("/nope/chrome"))),
            Err(Error::Config(_))
        ));
    }
}
