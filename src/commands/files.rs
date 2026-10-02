//! `geekcli files …` — the site's uploaded files (what the admin's Manage
//! Files page shows), served from `https://u.realgeeks.media/`. Paths are
//! relative to the site's folder; the root is `""`.

use std::path::Path;

use clap::{Args, Subcommand};
use reqwest::blocking::multipart::{Form, Part};
use serde_json::{json, Value};

use super::{confirm, Context};
use crate::client::Query;
use crate::error::{Error, Result};
use crate::output::{cell, col, Column, Format};

pub const PATH: &str = "files/";

pub const COLUMNS: &[Column] = &[
    col("type", "/type"),
    col("path", "/path"),
    col("size", "/size"),
    col("content_type", "/content_type"),
    col("dimensions", "/dimensions"),
    col("last_modified", "/last_modified"),
];

pub const DETAIL_COLUMNS: &[Column] = &[
    col("path", "/path"),
    col("type", "/type"),
    col("url", "/url"),
    col("size", "/size"),
    col("content_type", "/content_type"),
    col("dimensions", "/dimensions"),
    col("thumbnail_url", "/thumbnail_url"),
    col("last_modified", "/last_modified"),
];

#[derive(Debug, Args)]
pub struct FilesCommand {
    #[command(subcommand)]
    pub command: FilesSub,
}

#[derive(Debug, Subcommand)]
pub enum FilesSub {
    /// List a folder (folders first); the root when no path is given
    List {
        /// Folder path, e.g. images
        path: Option<String>,
        /// Search file names across every folder
        #[arg(long, short = 's', value_name = "TEXT")]
        search: Option<String>,
        /// Entries per page (max 500)
        #[arg(long, value_name = "N")]
        page_size: Option<u32>,
        /// Continue a listing from a previous `next_cursor`
        #[arg(long, value_name = "CURSOR")]
        cursor: Option<String>,
        /// Fetch every page
        #[arg(long, conflicts_with = "cursor")]
        all: bool,
    },
    /// Show one file or folder
    Get { path: String },
    /// Print a file's public URL
    Url { path: String },
    /// Upload one or more local files into a folder
    #[command(after_help = "Notes:
  - The folder must exist (files mkdir). 8,000,000 bytes max; jpg/jpeg, png, gif, ico, mp4, pdf, txt, css only. HTML, SVG, XML and scripts are refused (files are served inline from a domain shared by every site); the stored type follows the extension (of --name when given).
  - Every file's type and size is checked before anything is sent: one bad file refuses the whole run (exit 2) and names each problem, so nothing is half uploaded.
  - --from-url URL makes the site fetch the file itself, e.g. to copy a file that is already on u.realgeeks.media. Only https URLs on hosts the API allows (Real Geeks media and the file CDNs of AI platforms such as ChatGPT, Grok and Perplexity) are accepted; any other host is a validation error that names the allowed hosts. AI-platform URLs rarely end in a file name, so pass --name with the right extension.
  - -q prints the public URL to use in --facebook-image, <img src>, footers and file settings such as HEADER_LOGO.
  - Header logo: Real Geeks recommends 400x86 px, PNG, horizontal or text-based; upload at 2x (800x172) for sharp screens.
  - The CDN caches by path: after --overwrite a page may keep showing the old file. To replace an image that is already live, upload it under a new name and point the setting or content at that.")]
    Upload(UploadArgs),
    /// Create a folder, e.g. images/2026
    Mkdir { path: String },
    /// Move or rename a file
    Move {
        from: String,
        to: String,
        /// Replace an existing file at the destination
        #[arg(long)]
        overwrite: bool,
    },
    /// Delete a file, or a folder and everything in it
    Delete { path: String },
}

#[derive(Debug, Args)]
pub struct UploadArgs {
    /// Local files to upload
    #[arg(value_name = "FILE", required_unless_present = "from_url")]
    pub files: Vec<String>,
    /// Have the site fetch the file from this https URL instead of uploading a local one
    #[arg(long, value_name = "URL", conflicts_with = "files")]
    pub from_url: Option<String>,
    /// Destination folder on the site (default: root)
    #[arg(long, value_name = "FOLDER", default_value = "")]
    pub to: String,
    /// Name to store under (only with a single file; default: the local name)
    #[arg(long)]
    pub name: Option<String>,
    /// Replace an existing file with the same name
    #[arg(long)]
    pub overwrite: bool,
    /// Override the detected content type
    #[arg(long, value_name = "MIME")]
    pub content_type: Option<String>,
}

pub fn run(ctx: &Context, cmd: FilesCommand) -> Result<()> {
    match cmd.command {
        FilesSub::List {
            path,
            search,
            page_size,
            cursor,
            all,
        } => list(
            ctx,
            path.as_deref(),
            search.as_deref(),
            page_size,
            cursor.as_deref(),
            all,
        ),
        FilesSub::Get { path } => {
            let entry = ctx.client.get(&detail_path(&path), &Query::new())?.body;
            print_entry(ctx, &entry)
        }
        FilesSub::Url { path } => {
            let entry = ctx.client.get(&detail_path(&path), &Query::new())?.body;
            let url = entry.get("url").and_then(Value::as_str).unwrap_or("");
            println!("{url}");
            Ok(())
        }
        FilesSub::Upload(args) => upload(ctx, &args),
        FilesSub::Mkdir { path } => {
            let (folder, name) = split_path(&path)?;
            let created = ctx
                .client
                .post(
                    &format!("{PATH}folders/"),
                    &json!({ "path": folder, "name": name }),
                )?
                .body;
            ctx.printer
                .note(&format!("Created folder {}", cell(&created["path"])));
            print_entry(ctx, &created)
        }
        FilesSub::Move {
            from,
            to,
            overwrite,
        } => {
            let body =
                json!({ "from": normalize(&from), "to": normalize(&to), "overwrite": overwrite });
            let moved = ctx.client.post(&format!("{PATH}move/"), &body)?.body;
            ctx.printer
                .note(&format!("Moved to {}", cell(&moved["path"])));
            print_entry(ctx, &moved)
        }
        FilesSub::Delete { path } => {
            let path = normalize(&path);
            if !confirm(
                ctx,
                &format!("{path} (a folder is deleted with everything in it)"),
            )? {
                return Err(Error::Usage("cancelled".into()));
            }
            ctx.client.delete(&detail_path(&path), &Query::new())?;
            ctx.printer.note(&format!("Deleted {path}"));
            if ctx.printer.format != Format::Table {
                ctx.printer.raw(&json!({ "deleted": true, "path": path }))?;
            }
            Ok(())
        }
    }
}

/// Trim slashes so `images/` and `/images` both mean `images`.
pub fn normalize(path: &str) -> String {
    path.trim().trim_matches('/').to_string()
}

fn detail_path(path: &str) -> String {
    let path = normalize(path);
    let encoded: Vec<String> = path
        .split('/')
        .map(|seg| {
            url::form_urlencoded::byte_serialize(seg.as_bytes())
                .collect::<String>()
                .replace('+', "%20")
        })
        .collect();
    format!("{PATH}{}/", encoded.join("/"))
}

/// `images/2026` → (`images`, `2026`); `logo.png` → (``, `logo.png`).
pub fn split_path(path: &str) -> Result<(String, String)> {
    let path = normalize(path);
    if path.is_empty() {
        return Err(Error::Usage("a path is required".into()));
    }
    Ok(match path.rsplit_once('/') {
        Some((folder, name)) => (folder.to_string(), name.to_string()),
        None => (String::new(), path),
    })
}

fn with_dimensions(entry: &Value) -> Value {
    let mut out = entry.clone();
    if let (Some(w), Some(h)) = (
        entry.pointer("/dimensions/width").and_then(Value::as_u64),
        entry.pointer("/dimensions/height").and_then(Value::as_u64),
    ) {
        out["dimensions"] = Value::String(format!("{w}x{h}"));
    }
    out
}

fn print_entry(ctx: &Context, entry: &Value) -> Result<()> {
    if ctx.printer.quiet {
        println!(
            "{}",
            entry
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or_else(|| entry.get("path").and_then(Value::as_str).unwrap_or(""))
        );
        return Ok(());
    }
    if ctx.printer.format == Format::Table {
        return ctx.printer.one(&with_dimensions(entry), DETAIL_COLUMNS);
    }
    ctx.printer.raw(entry)
}

fn list(
    ctx: &Context,
    path: Option<&str>,
    search: Option<&str>,
    page_size: Option<u32>,
    cursor: Option<&str>,
    all: bool,
) -> Result<()> {
    let folder = normalize(path.unwrap_or(""));
    let mut query = Query::new();
    query.push(("path".into(), folder.clone()));
    if let Some(q) = search {
        query.push(("q".into(), q.to_string()));
    }
    if let Some(n) = page_size {
        query.push(("page_size".into(), n.to_string()));
    }
    let mut entries: Vec<Value> = Vec::new();
    let mut cursor = cursor.map(str::to_string);
    loop {
        let mut q = query.clone();
        if let Some(c) = &cursor {
            q.push(("cursor".into(), c.clone()));
        }
        let response = ctx.client.get(PATH, &q)?.body;
        entries.extend(
            response
                .get("results")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        );
        cursor = response
            .get("next_cursor")
            .and_then(Value::as_str)
            .map(str::to_string);
        if !all || cursor.is_none() {
            break;
        }
    }
    if ctx.printer.quiet {
        for entry in &entries {
            println!(
                "{}",
                entry.get("path").and_then(Value::as_str).unwrap_or("")
            );
        }
        return Ok(());
    }
    match ctx.printer.format {
        Format::Table => {
            let rows: Vec<Value> = entries.iter().map(with_dimensions).collect();
            ctx.printer.note(&format!("/{folder}"));
            ctx.printer.list(&rows, None, COLUMNS)?;
            if let Some(c) = cursor {
                ctx.printer.note(&format!("more entries: --cursor {c}"));
            }
            Ok(())
        }
        Format::Json | Format::Jsonl => ctx.printer.raw(&json!({
            "path": folder,
            "results": entries,
            "next_cursor": cursor,
        })),
    }
}

fn upload(ctx: &Context, args: &UploadArgs) -> Result<()> {
    if let Some(url) = &args.from_url {
        return upload_from_url(ctx, args, url);
    }
    if args.name.is_some() && args.files.len() > 1 {
        return Err(Error::Usage(
            "--name only applies when uploading a single file".into(),
        ));
    }
    let folder = normalize(&args.to);
    let planned = check_uploads(&args.files, args.name.as_deref())?;
    let mut uploaded = Vec::new();
    for (local, file_name) in planned {
        let bytes =
            std::fs::read(local).map_err(|e| Error::Io(format!("cannot read {local}: {e}")))?;
        let content_type = args
            .content_type
            .clone()
            .unwrap_or_else(|| guess_content_type(&file_name).to_string());
        let part = Part::bytes(bytes)
            .file_name(file_name.clone())
            .mime_str(&content_type)
            .map_err(|e| Error::Usage(format!("bad content type '{content_type}': {e}")))?;
        let mut form = Form::new()
            .part("file", part)
            .text("path", folder.clone())
            .text("name", file_name.clone());
        if args.overwrite {
            form = form.text("overwrite", "true");
        }
        let entry = ctx
            .client
            .post_multipart(&format!("{PATH}upload/"), form)?
            .body;
        ctx.printer
            .note(&format!("Uploaded {} → {}", local, cell(&entry["url"])));
        uploaded.push(entry);
    }
    if uploaded.len() == 1 {
        return print_entry(ctx, &uploaded[0]);
    }
    if ctx.printer.quiet {
        for entry in &uploaded {
            println!("{}", entry.get("url").and_then(Value::as_str).unwrap_or(""));
        }
        return Ok(());
    }
    let rows: Vec<Value> = uploaded.iter().map(with_dimensions).collect();
    ctx.printer.list(&rows, None, COLUMNS)
}

/// Extensions the upload endpoint accepts (lower case).
pub const UPLOAD_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "ico", "mp4", "pdf", "txt", "css",
];

/// Largest file the upload endpoint accepts, in bytes.
pub const UPLOAD_MAX_BYTES: u64 = 8_000_000;

/// Check every local file's stored name and size before anything is sent,
/// so a multi-file upload is refused whole instead of stopping halfway.
/// Returns `(local path, stored name)` pairs in the order given.
fn check_uploads<'a>(files: &'a [String], name: Option<&str>) -> Result<Vec<(&'a str, String)>> {
    let mut planned = Vec::new();
    let mut problems = Vec::new();
    for local in files {
        let path = Path::new(local);
        let Some(file_name) = name.map(str::to_string).or_else(|| {
            path.file_name()
                .and_then(|n| n.to_str())
                .map(str::to_string)
        }) else {
            problems.push(format!("{local}: cannot determine a file name"));
            continue;
        };
        let ext = Path::new(&file_name)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        if !ext
            .as_deref()
            .is_some_and(|e| UPLOAD_EXTENSIONS.contains(&e))
        {
            problems.push(format!(
                "{file_name}: type not accepted (allowed: {})",
                UPLOAD_EXTENSIONS.join(", ")
            ));
        }
        match std::fs::metadata(path) {
            Ok(meta) if !meta.is_file() => problems.push(format!("{local}: not a file")),
            Ok(meta) if meta.len() > UPLOAD_MAX_BYTES => problems.push(format!(
                "{local}: {} bytes is over the {UPLOAD_MAX_BYTES}-byte limit",
                meta.len()
            )),
            Ok(_) => {}
            Err(e) => problems.push(format!("cannot read {local}: {e}")),
        }
        planned.push((local.as_str(), file_name));
    }
    if problems.is_empty() {
        Ok(planned)
    } else {
        Err(Error::Usage(format!(
            "nothing uploaded; fix these first: {}",
            problems.join("; ")
        )))
    }
}

/// Content type from the extension, limited to what the API accepts.
pub fn guess_content_type(name: &str) -> &'static str {
    match Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("mp4") => "video/mp4",
        Some("pdf") => "application/pdf",
        Some("txt" | "md") => "text/plain",
        Some("html" | "htm") => "text/html",
        Some("css") => "text/css",
        Some("js") => "application/javascript",
        Some("xml") => "text/xml",
        _ => "application/octet-stream",
    }
}

/// `POST files/upload/` as JSON with `content_url`: the site downloads the
/// file. The stored name defaults to the URL's last path segment.
fn upload_from_url(ctx: &Context, args: &UploadArgs, url: &str) -> Result<()> {
    let name = match &args.name {
        Some(name) => name.clone(),
        None => url_file_name(url)
            .ok_or_else(|| {
                Error::Usage(format!(
                    "{url} does not end in a file name with an extension; pass --name, e.g. --name photo.jpg"
                ))
            })?,
    };
    let mut body = json!({
        "path": normalize(&args.to),
        "name": name,
        "content_url": url,
    });
    if let Some(content_type) = &args.content_type {
        body["content_type"] = json!(content_type);
    }
    if args.overwrite {
        body["overwrite"] = json!(true);
    }
    let entry = ctx.client.post(&format!("{PATH}upload/"), &body)?.body;
    ctx.printer
        .note(&format!("Fetched {url} → {}", cell(&entry["url"])));
    print_entry(ctx, &entry)
}

/// The URL's last path segment when it looks like a file name (has an
/// extension); the site decides the stored type from the extension.
fn url_file_name(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let segment = parsed.path_segments()?.rev().find(|s| !s.is_empty())?;
    let (stem, ext) = segment.rsplit_once('.')?;
    (!stem.is_empty() && !ext.is_empty()).then(|| segment.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_a_fetched_file_from_its_url() {
        assert_eq!(
            url_file_name("https://u.realgeeks.media/site/images/logo.png?v=2").as_deref(),
            Some("logo.png")
        );
        assert_eq!(
            url_file_name("https://u.realgeeks.media/site/docs/guide.pdf/").as_deref(),
            Some("guide.pdf")
        );
        assert_eq!(
            url_file_name("https://files.oaiusercontent.com/file-abc123"),
            None
        );
        assert_eq!(url_file_name("https://example.com/"), None);
        assert_eq!(url_file_name("not a url"), None);
    }

    #[test]
    fn splits_and_normalizes_paths() {
        assert_eq!(
            split_path("/images/2026/").ok(),
            Some(("images".into(), "2026".into()))
        );
        assert_eq!(
            split_path("logo.png").ok(),
            Some((String::new(), "logo.png".into()))
        );
        assert!(split_path("/").is_err());
        assert_eq!(
            detail_path("images/my logo.png"),
            "files/images/my%20logo.png/"
        );
    }

    #[test]
    fn guesses_types() {
        assert_eq!(guess_content_type("Photo.JPG"), "image/jpeg");
        assert_eq!(guess_content_type("doc.pdf"), "application/pdf");
        assert_eq!(guess_content_type("weird.bin"), "application/octet-stream");
    }
}
