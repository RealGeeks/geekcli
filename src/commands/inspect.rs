//! `geekcli inspect …` — query the rendered public page through Chrome.
//!
//! This complements snapshots: API responses show what was saved, while these
//! commands show the DOM visitors (and browser JavaScript) actually receive.

use std::path::PathBuf;

use clap::{ArgGroup, Args};
use serde_json::{json, Value};

use super::{snapshot, Context};
use crate::error::{Error, Result};

#[derive(Debug, Args)]
#[command(
    group(ArgGroup::new("operation").required(true).multiple(false)),
    after_help = "Examples:
  geekcli inspect / --text h1
  geekcli inspect / --count '.featured-listing'
  geekcli inspect / --attr 'a.cta' href
  geekcli inspect / --assert \"document.querySelectorAll('h1').length === 1\"

`--assert` exits 1 when its JavaScript expression is false. `--js` is an
escape hatch for browser-side, read-only queries; prefer the selector options
when they express the check."
)]
pub struct InspectArgs {
    /// Site-relative path or a full URL. Default: the home page
    #[arg(value_name = "PATH_OR_URL", default_value = "/")]
    pub target: String,
    /// Print an element's rendered text
    #[arg(long, group = "operation", value_name = "CSS")]
    pub text: Option<String>,
    /// Print an element's outer HTML
    #[arg(long, group = "operation", value_name = "CSS")]
    pub html: Option<String>,
    /// Print an element attribute: CSS NAME
    #[arg(long, group = "operation", num_args = 2, value_names = ["CSS", "NAME"])]
    pub attr: Option<Vec<String>>,
    /// Print the number of elements matching a CSS selector
    #[arg(long, group = "operation", value_name = "CSS")]
    pub count: Option<String>,
    /// Print whether a CSS selector matches an element
    #[arg(long, group = "operation", value_name = "CSS")]
    pub exists: Option<String>,
    /// Print whether an element exists and is visible
    #[arg(long, group = "operation", value_name = "CSS")]
    pub visible: Option<String>,
    /// Evaluate a JavaScript expression and print its JSON value
    #[arg(long, group = "operation", value_name = "EXPRESSION")]
    pub js: Option<String>,
    /// Require a JavaScript expression to evaluate to true (exit 1 if false)
    #[arg(long, group = "operation", value_name = "EXPRESSION")]
    pub assert: Option<String>,
    /// Viewport width in CSS pixels
    #[arg(long, default_value_t = 1440, value_name = "PX")]
    pub width: u32,
    /// Viewport height in CSS pixels
    #[arg(long, default_value_t = 900, value_name = "PX")]
    pub height: u32,
    /// Phone emulation (390x844, touch, device scale 2)
    #[arg(long, conflicts_with_all = ["width", "height"])]
    pub mobile: bool,
    /// Device scale factor; 2 doubles the pixel size for sharper text
    #[arg(long, default_value_t = 1, value_name = "N")]
    pub scale: u8,
    /// Give scripts, fonts and lazy images this long to settle before querying
    #[arg(long, default_value_t = 1500, value_name = "MS")]
    pub wait: u32,
    /// Chrome/Chromium binary (or set GEEKCLI_BROWSER); auto-detected otherwise
    #[arg(long, env = snapshot::BROWSER_ENV, value_name = "PATH")]
    pub browser: Option<PathBuf>,
}

pub fn run(ctx: &Context, args: &InspectArgs) -> Result<()> {
    let url = snapshot::resolve_url(&ctx.client, &args.target);
    let (width, height, scale) = if args.mobile {
        (390, 844, 2)
    } else {
        (args.width, args.height, args.scale.max(1))
    };
    let browser = snapshot::find_browser(args.browser.as_deref())?;
    let capture = snapshot::Capture {
        url: &url,
        width,
        height,
        scale,
        mobile: args.mobile,
        full: false,
        wait_ms: u64::from(args.wait),
    };

    if ctx.printer.format == crate::output::Format::Table {
        ctx.printer
            .note(&format!("Inspecting {url} with {}", browser.display()));
    }

    let mut doc = snapshot::visit(&browser, &capture, |tab| {
        if let Some(selector) = &args.text {
            let value = element(tab, selector)?
                .get_inner_text()
                .map_err(|error| browser_error(&error))?;
            Ok(json!({"operation": "text", "selector": selector, "value": value}))
        } else if let Some(selector) = &args.html {
            let value = element(tab, selector)?
                .get_content()
                .map_err(|error| browser_error(&error))?;
            Ok(json!({"operation": "html", "selector": selector, "value": value}))
        } else if let Some(values) = &args.attr {
            let selector = &values[0];
            let name = &values[1];
            let attributes = element(tab, selector)?
                .get_attributes()
                .map_err(|error| browser_error(&error))?;
            let value = attributes.and_then(|attributes| {
                attributes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .find_map(|pair| (pair[0] == *name).then(|| Value::String(pair[1].clone())))
            });
            Ok(
                json!({"operation": "attr", "selector": selector, "attribute": name, "value": value}),
            )
        } else if let Some(selector) = &args.count {
            let value = selector_count(tab, selector)?;
            Ok(json!({"operation": "count", "selector": selector, "value": value}))
        } else if let Some(selector) = &args.exists {
            let value = tab.find_element(selector).is_ok();
            Ok(json!({"operation": "exists", "selector": selector, "value": value}))
        } else if let Some(selector) = &args.visible {
            let value = tab
                .find_element(selector)
                .ok()
                .map(|element| element.is_visible().map_err(|error| browser_error(&error)))
                .transpose()?
                .unwrap_or(false);
            Ok(json!({"operation": "visible", "selector": selector, "value": value}))
        } else if let Some(expression) = &args.js {
            let value = evaluate(tab, expression)?;
            Ok(json!({"operation": "js", "expression": expression, "value": value}))
        } else if let Some(expression) = &args.assert {
            let value = evaluate(tab, expression)?;
            if value.as_bool() != Some(true) {
                return Err(Error::Other(format!(
                    "assertion failed: `{expression}` evaluated to {value}"
                )));
            }
            Ok(json!({"operation": "assert", "expression": expression, "value": true}))
        } else {
            Err(Error::Usage("choose one inspection operation".into()))
        }
    })?;
    doc["url"] = Value::String(url);
    doc["width"] = json!(width);
    doc["height"] = json!(height);
    doc["mobile"] = json!(args.mobile);
    ctx.printer.raw(&doc)
}

fn element<'a>(
    tab: &'a headless_chrome::Tab,
    selector: &str,
) -> Result<headless_chrome::Element<'a>> {
    tab.find_element(selector)
        .map_err(|_| Error::Other(format!("no element matches selector `{selector}`")))
}

fn selector_count(tab: &headless_chrome::Tab, selector: &str) -> Result<u64> {
    let selector = serde_json::to_string(selector)
        .map_err(|e| Error::Other(format!("cannot encode selector: {e}")))?;
    let value = evaluate(
        tab,
        &format!("document.querySelectorAll({selector}).length"),
    )?;
    value
        .as_u64()
        .ok_or_else(|| Error::Other("the browser did not return a selector count".into()))
}

fn evaluate(tab: &headless_chrome::Tab, expression: &str) -> Result<Value> {
    tab.evaluate(expression, true)
        .map_err(|error| browser_error(&error))?
        .value
        .ok_or_else(|| Error::Other("JavaScript did not return a JSON value".into()))
}

fn browser_error(error: &anyhow::Error) -> Error {
    Error::Other(format!("browser query failed: {error}"))
}
