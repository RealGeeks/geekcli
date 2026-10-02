//! Body/content input: read from a flag, a file or stdin, and optionally
//! convert Markdown to the HTML the API expects.
//!
//! The API sanitizes HTML server side with an allow-list; Markdown output
//! from `pulldown-cmark` fits inside that allow-list, and raw HTML in the
//! Markdown (including the blog's `<!--read more-->` marker) passes through
//! untouched.

use std::fs;
use std::io::{self, Read};
use std::path::Path;

use pulldown_cmark::{html, Options, Parser};

use crate::error::{Error, Result};

pub const READ_MORE: &str = "<!--read more-->";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    Html,
    Markdown,
}

/// Resolve a body from an inline value or a file path (`-` for stdin).
/// Returns `None` when neither was given.
pub fn read_body(
    inline: Option<&str>,
    file: Option<&str>,
    markdown: bool,
) -> Result<Option<String>> {
    let (text, kind) = match (inline, file) {
        (Some(_), Some(_)) => {
            return Err(Error::Usage(
                "give either an inline value or a file, not both".into(),
            ));
        }
        (Some(text), None) => (
            text.to_string(),
            if markdown {
                BodyKind::Markdown
            } else {
                BodyKind::Html
            },
        ),
        (None, Some(path)) => {
            let text = read_file_or_stdin(path)?;
            let kind = if markdown || looks_like_markdown_path(path) {
                BodyKind::Markdown
            } else {
                BodyKind::Html
            };
            (text, kind)
        }
        (None, None) => return Ok(None),
    };
    Ok(Some(match kind {
        BodyKind::Html => text,
        BodyKind::Markdown => markdown_to_html(&text),
    }))
}

pub fn read_file_or_stdin(path: &str) -> Result<String> {
    if path == "-" {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        return Ok(buf);
    }
    fs::read_to_string(path).map_err(|e| Error::Io(format!("cannot read {path}: {e}")))
}

fn looks_like_markdown_path(path: &str) -> bool {
    matches!(
        Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("md" | "markdown" | "mdown" | "mkd")
    )
}

/// Convert Markdown to HTML. Tables, strikethrough and footnotes are on;
/// the output is trimmed so a trailing newline does not end up in the post.
pub fn markdown_to_html(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_HEADING_ATTRIBUTES);
    let parser = Parser::new_ext(markdown, options);
    let mut out = String::with_capacity(markdown.len() * 2);
    html::push_html(&mut out, parser);
    out.trim().to_string()
}

/// Split a body at the read-more marker: `(summary, rest)`.
pub fn split_read_more(body: &str) -> (&str, Option<&str>) {
    match body.find(READ_MORE) {
        Some(idx) => (&body[..idx], Some(&body[idx + READ_MORE.len()..])),
        None => (body, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_markdown_and_keeps_read_more() {
        let html = markdown_to_html("# Hi\n\nIntro.\n\n<!--read more-->\n\nMore *text*.");
        assert!(html.starts_with("<h1>Hi</h1>"));
        assert!(html.contains(READ_MORE));
        assert!(html.contains("<em>text</em>"));
    }

    #[test]
    fn detects_markdown_extension() {
        assert!(looks_like_markdown_path("post.md"));
        assert!(looks_like_markdown_path("/tmp/x.Markdown"));
        assert!(!looks_like_markdown_path("post.html"));
        assert!(!looks_like_markdown_path("-"));
    }

    #[test]
    fn splits_summary() {
        let (summary, rest) = split_read_more("<p>a</p><!--read more--><p>b</p>");
        assert_eq!(summary, "<p>a</p>");
        assert_eq!(rest, Some("<p>b</p>"));
        assert_eq!(split_read_more("<p>a</p>"), ("<p>a</p>", None));
    }

    #[test]
    fn inline_and_file_are_exclusive() {
        assert!(matches!(
            read_body(Some("x"), Some("y"), false),
            Err(Error::Usage(_))
        ));
        assert_eq!(read_body(None, None, false).ok().flatten(), None);
        assert_eq!(
            read_body(Some("**b**"), None, true).ok().flatten(),
            Some("<p><strong>b</strong></p>".into())
        );
    }
}
