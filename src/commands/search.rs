//! `geekcli search …` — test and refine property-search criteria against
//! the site's legacy search API, so links written into pages and posts
//! actually filter the way they read.
//!
//! The site silently drops any criterion that is not one of its search
//! form fields (so `?q=Buckhead` "works" but matches everything). These
//! commands make that visible: every criterion you pass is checked against
//! what the site echoes back, and anything ignored is reported.

use clap::{Args, Subcommand};
use serde_json::{json, Map, Value};

use super::Context;
use crate::client::Query;
use crate::error::{Error, Result};
use crate::output::{cell, col, Column, Format};

pub const SEARCH_PATH: &str = "api/v2/search/";
pub const METADATA_PATH: &str = "api/v2/search/metadata/";
pub const FORM_PATH: &str = "search_forms/api/advanced_search_form.json";

/// Query keys that control the request rather than filter properties.
const CONTROL_KEYS: &[&str] = &[
    "page",
    "per_page",
    "fields",
    "include_description",
    "add_labels",
    "add_averages",
    "escape_result",
    "no_coords_override",
];
const SORT_KEYS: &[&str] = &[
    "sort_highest",
    "sort_lowest",
    "sort_latest",
    "sort_oldest",
    "sort_relevance",
];
const DEFAULT_FIELDS: &str = "mls_number,address,city,list_price,beds,baths,area,type,status,url";

pub const FIELD_COLUMNS: &[Column] = &[
    col("attr", "/attr"),
    col("label", "/label"),
    col("widget", "/widget"),
    col("default", "/default"),
    col("value", "/value"),
    col("choices", "/choices_count"),
];

pub const CHOICE_COLUMNS: &[Column] = &[col("value", "/value"), col("label", "/label")];

pub const RESULT_COLUMNS: &[Column] = &[
    col("mls", "/mls_number"),
    col("address", "/address"),
    col("city", "/city"),
    col("price", "/list_price"),
    col("beds", "/beds"),
    col("baths", "/baths"),
    col("type", "/type"),
];

#[derive(Debug, Args)]
pub struct SearchCommand {
    #[command(subcommand)]
    pub command: SearchSub,
}

#[derive(Debug, Subcommand)]
pub enum SearchSub {
    /// List the search fields this site accepts (the advanced search form)
    Fields,
    /// List the valid values for one field, e.g. `city`
    Choices {
        /// Field name from `search fields`
        field: String,
        /// Only values containing this text
        #[arg(long, short = 's')]
        search: Option<String>,
        /// Every value the site knows for the field, across all counties and cities (from the
        /// search autocomplete index), not just the form's default list
        #[arg(long)]
        all: bool,
    },
    /// Check criteria: what the site understood, and what it ignored
    #[command(after_help = "Notes:
  - The site drops unknown criteria silently and the page still 'works', showing everything.
  - Exit code 5 here means a key was ignored; `search fields` lists the keys this site takes, `search choices <field>` the values (city and subdivision lists depend on the default county).
  - Paste a whole URL to check a link already on a page.
  - polygon=lat,lng;lat,lng;… searches a custom map area (latitude first, first point repeated at the end); see `geekcli guide polygon`.")]
    Check(CriteriaArgs),
    /// Run a search and show matching properties
    Run(RunArgs),
    /// Print site URLs for criteria; --save stores the search and prints its id
    Url(UrlArgs),
}

/// Criteria as `key=value` items, or a pasted `?a=b&c=d` query string or URL.
#[derive(Debug, Args, Clone)]
pub struct CriteriaArgs {
    /// key=value pairs (repeat a key for several values), or one query string / URL
    #[arg(value_name = "CRITERIA", required = true)]
    pub items: Vec<String>,
}

#[derive(Debug, Args)]
pub struct RunArgs {
    #[command(flatten)]
    pub criteria: CriteriaArgs,
    /// Results per page
    #[arg(long, default_value_t = 10, value_name = "N")]
    pub per_page: u32,
    #[arg(long, default_value_t = 1)]
    pub page: u32,
    /// highest, lowest, latest, oldest or relevance
    #[arg(long, value_name = "ORDER")]
    pub sort: Option<String>,
    /// Comma-separated property fields to return
    #[arg(long, value_name = "FIELDS")]
    pub fields: Option<String>,
    /// Fail (exit 5) if any criterion is ignored by the site
    #[arg(long)]
    pub strict: bool,
}

#[derive(Debug, Args)]
pub struct UrlArgs {
    #[command(flatten)]
    pub criteria: CriteriaArgs,
    /// Save the search on the site and print URLs using its id
    #[arg(long)]
    pub save: bool,
}

pub fn run(ctx: &Context, cmd: SearchCommand) -> Result<()> {
    match cmd.command {
        SearchSub::Fields => fields(ctx),
        SearchSub::Choices { field, search, all } => {
            if all {
                all_choices(ctx, &field, search.as_deref())
            } else {
                choices(ctx, &field, search.as_deref())
            }
        }
        SearchSub::Check(args) => check(ctx, &args),
        SearchSub::Run(args) => run_search(ctx, &args),
        SearchSub::Url(args) => url(ctx, &args),
    }
}

// ---------------------------------------------------------------- criteria

/// Turn CLI items into a query. Accepts `key=value` items and, as a
/// convenience, a whole query string or URL copied from the site.
pub fn parse_criteria(items: &[String]) -> Result<Query> {
    let mut query = Query::new();
    for item in items {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let looks_like_url = item.starts_with('?')
            || item.starts_with("http://")
            || item.starts_with("https://")
            || (item.starts_with('/') && item.contains('?'))
            || (item.contains('&') && item.contains('='));
        if looks_like_url {
            let qs = match item.find('?') {
                Some(idx) => &item[idx + 1..],
                None => item,
            };
            let qs = qs.split('#').next().unwrap_or_default();
            for (k, v) in url::form_urlencoded::parse(qs.as_bytes()) {
                if !k.is_empty() {
                    query.push((k.to_string(), v.to_string()));
                }
            }
            continue;
        }
        let Some((k, v)) = item.split_once('=') else {
            return Err(Error::Usage(format!(
                "expected key=value, got '{item}' (run `geekcli search fields` for the field names)"
            )));
        };
        query.push((k.trim().to_string(), v.trim().to_string()));
    }
    if query.is_empty() {
        return Err(Error::Usage("no criteria given".into()));
    }
    for (k, v) in &query {
        if k == POLYGON {
            for warning in check_polygon(v)? {
                eprintln!("warning: polygon: {warning}");
            }
        }
    }
    Ok(query)
}

// ----------------------------------------------------------------- polygon

/// The map-drawn area criterion. The site accepts it but its search form
/// does not list it, so `search fields` adds it and the value is checked here.
pub const POLYGON: &str = "polygon";
/// Above this many points the URL gets long and the boundary is usually
/// over-traced; warn rather than fail.
const POLYGON_MAX_POINTS: usize = 100;
/// No real-estate search sits south of this latitude. A US longitude given
/// first (lng,lat) lands here, so it is the swapped-order tell.
const POLYGON_SWAP_LAT: f64 = -60.0;

/// Check a `polygon=lat,lng;lat,lng;…` value. Malformed values are a usage
/// error; returns warnings for values the site takes but that are probably
/// not what was meant. The value itself is never rewritten.
///
/// Heuristics, kept simple on purpose:
/// - not closed: the first point is not repeated at the end;
/// - swapped: a point south of latitude -60, or a latitude out of range
///   whose longitude would be a valid latitude (lng,lat order);
/// - large: more than 100 points.
pub fn check_polygon(value: &str) -> Result<Vec<String>> {
    let usage = |why: String| {
        Error::Usage(format!(
            "polygon: {why}. Expected lat,lng;lat,lng;… in decimal degrees, latitude first, at least 3 points (see `geekcli guide polygon`)"
        ))
    };
    let mut points: Vec<(f64, f64)> = Vec::new();
    for (i, raw) in value.split(';').enumerate() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let n = i + 1;
        let parts: Vec<&str> = raw.split(',').map(str::trim).collect();
        let [lat, lng] = parts[..] else {
            return Err(usage(format!("point {n} '{raw}' is not a lat,lng pair")));
        };
        let number = |text: &str| text.parse::<f64>().ok().filter(|f| f.is_finite());
        let (Some(lat), Some(lng)) = (number(lat), number(lng)) else {
            return Err(usage(format!("point {n} '{raw}' is not numeric")));
        };
        if !(-90.0..=90.0).contains(&lat) {
            let hint = if (-90.0..=90.0).contains(&lng) {
                " (it looks like lng,lat; latitude comes first)"
            } else {
                ""
            };
            return Err(usage(format!(
                "point {n} latitude {lat} is outside -90..90{hint}"
            )));
        }
        if !(-180.0..=180.0).contains(&lng) {
            return Err(usage(format!(
                "point {n} longitude {lng} is outside -180..180"
            )));
        }
        points.push((lat, lng));
    }
    let mut distinct: Vec<(f64, f64)> = Vec::new();
    for p in &points {
        if !distinct.contains(p) {
            distinct.push(*p);
        }
    }
    if distinct.len() < 3 {
        return Err(usage(format!("{} distinct point(s) given", distinct.len())));
    }

    let mut warnings = Vec::new();
    if points.first() != points.last() {
        let (lat, lng) = points[0];
        warnings.push(format!(
            "the ring is not closed; repeat the first point ({lat},{lng}) at the end, as the map tool does"
        ));
    }
    if points.iter().any(|(lat, _)| *lat < POLYGON_SWAP_LAT) {
        warnings.push(format!(
            "a latitude is below {POLYGON_SWAP_LAT}, which usually means lng,lat order; latitude comes first"
        ));
    }
    if points.len() > POLYGON_MAX_POINTS {
        warnings.push(format!(
            "{} points; keep it to about {POLYGON_MAX_POINTS} or fewer by simplifying the boundary",
            points.len()
        ));
    }
    Ok(warnings)
}

/// The `search fields` row for `polygon`, shaped like the form rows.
fn polygon_field() -> Value {
    json!({
        "attr": POLYGON,
        "label": "Custom area: lat,lng;lat,lng;… (built in, not on the site's form; see `geekcli guide polygon`)",
        "section": "builtin",
        "widget": "polygon",
        "default": Value::Null,
        "value": Value::Null,
        "depends_on": Value::Null,
        "choices_count": 0,
        "choices": [],
    })
}

fn is_control(key: &str) -> bool {
    CONTROL_KEYS.contains(&key) || SORT_KEYS.contains(&key)
}

/// The site's view of the criteria: description, what it kept, and which
/// of our keys it dropped.
struct Understood {
    description: String,
    criteria: Map<String, Value>,
    ignored: Vec<String>,
}

fn understand(ctx: &Context, query: &Query) -> Result<Understood> {
    let response = ctx.client.site_get(METADATA_PATH, query)?;
    let description = response
        .body
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let criteria = response
        .body
        .get("criteria")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut ignored: Vec<String> = query
        .iter()
        .map(|(k, _)| k.clone())
        .filter(|k| !is_control(k) && !criteria.contains_key(k))
        .collect();
    ignored.sort();
    ignored.dedup();
    Ok(Understood {
        description,
        criteria,
        ignored,
    })
}

fn ignored_error(ignored: &[String]) -> Error {
    Error::Api {
        status: 422,
        code: "ignored_criteria".into(),
        message: format!(
            "the site ignored {} criterion/criteria: {}. Run `geekcli search fields` for the names it accepts.",
            ignored.len(),
            ignored.join(", ")
        ),
        fields: ignored
            .iter()
            .map(|k| (k.clone(), vec!["Not a search field on this site; ignored".to_string()]))
            .collect(),
        retry_after: None,
    }
}

/// Canonical query string from the site's echoed criteria (lists expand).
fn canonical_query(criteria: &Map<String, Value>) -> Query {
    let mut query = Query::new();
    for (k, v) in criteria {
        match v {
            Value::Array(items) => {
                for item in items {
                    query.push((k.clone(), cell(item)));
                }
            }
            other => query.push((k.clone(), cell(other))),
        }
    }
    query
}

fn encode(query: &Query) -> String {
    let mut ser = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in query {
        ser.append_pair(k, v);
    }
    ser.finish()
}

fn strip_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for ch in text.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

// ------------------------------------------------------------------ fields

fn form_fields(ctx: &Context) -> Result<Vec<Value>> {
    let form = ctx.client.site_get(FORM_PATH, &Query::new())?.body;
    let mut rows = Vec::new();
    for section in ["primary", "secondary"] {
        let Some(items) = form.get(section).and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            let choices: Vec<Value> = item
                .get("choices")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .map(|pair| match pair.as_array() {
                            Some(p) if p.len() == 2 => json!({ "value": p[0], "label": p[1] }),
                            _ => json!({ "value": pair, "label": pair }),
                        })
                        .collect()
                })
                .unwrap_or_default();
            rows.push(json!({
                "attr": item.get("attr").cloned().unwrap_or(Value::Null),
                "label": item.get("label").cloned().unwrap_or(Value::Null),
                "section": section,
                "widget": item.get("widget_type").cloned().unwrap_or(Value::Null),
                "default": item.get("default_value").cloned().unwrap_or(Value::Null),
                "value": item.get("value").cloned().unwrap_or(Value::Null),
                "depends_on": item.get("dependent_fields").cloned().unwrap_or(Value::Null),
                "choices_count": choices.len(),
                "choices": choices,
            }));
        }
    }
    Ok(rows)
}

fn fields(ctx: &Context) -> Result<()> {
    let mut rows = form_fields(ctx)?;
    rows.push(polygon_field());
    if ctx.printer.format == Format::Table {
        // checkboxes for the same attr (type=res, type=con, …) read better merged
        let mut merged: Vec<Value> = Vec::new();
        for row in rows {
            let attr = row.get("attr").cloned().unwrap_or(Value::Null);
            if let Some(existing) = merged.iter_mut().find(|m| {
                m.get("attr") == Some(&attr) && m.get("value").is_some_and(|v| !v.is_null())
            }) {
                let values = format!("{}, {}", cell(&existing["value"]), cell(&row["value"]));
                existing["value"] = Value::String(values);
                existing["label"] = Value::String("Property type".into());
                continue;
            }
            merged.push(row);
        }
        ctx.printer.list(&merged, None, FIELD_COLUMNS)?;
        ctx.printer.note("Use `geekcli search choices <attr>` for a field's values; checkbox values combine, e.g. type=res type=con.");
        return Ok(());
    }
    ctx.printer.list(&rows, None, FIELD_COLUMNS)
}

fn choices(ctx: &Context, field: &str, needle: Option<&str>) -> Result<()> {
    if field == POLYGON {
        return Err(Error::Usage(
            "polygon has no list of values: it takes lat,lng;lat,lng;… points (see `geekcli guide polygon`)".into(),
        ));
    }
    let rows = form_fields(ctx)?;
    let matching: Vec<&Value> = rows
        .iter()
        .filter(|r| r.get("attr").and_then(Value::as_str) == Some(field))
        .collect();
    if matching.is_empty() {
        return Err(Error::Api {
            status: 404,
            code: "not_found".into(),
            message: format!(
                "no search field named '{field}' on this site; see `geekcli search fields`"
            ),
            fields: std::collections::BTreeMap::new(),
            retry_after: None,
        });
    }
    // checkbox groups: each row is one value
    let mut out: Vec<Value> = Vec::new();
    for row in matching {
        if let Some(v) = row.get("value").filter(|v| !v.is_null()) {
            out.push(json!({ "value": v, "label": row["label"] }));
        }
        if let Some(list) = row.get("choices").and_then(Value::as_array) {
            out.extend(list.iter().cloned());
        }
    }
    if let Some(needle) = needle {
        let needle = needle.to_ascii_lowercase();
        out.retain(|c| {
            cell(&c["value"]).to_ascii_lowercase().contains(&needle)
                || cell(&c["label"]).to_ascii_lowercase().contains(&needle)
        });
    }
    ctx.printer.list(&out, None, CHOICE_COLUMNS)
}

pub const AUTOCOMPLETE_PATH: &str = "api/v2/search/autocomplete-options/";

/// Every known value for a field from the site's autocomplete index
/// (`[{"field", "value"}, …]`), which covers all cities, unlike the
/// advanced form's choice list that follows the default county.
fn all_choices(ctx: &Context, field: &str, needle: Option<&str>) -> Result<()> {
    let body = ctx.client.site_get(AUTOCOMPLETE_PATH, &Query::new())?.body;
    let needle = needle.map(str::to_ascii_lowercase);
    let mut values: Vec<String> = body
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter(|i| i.get("field").and_then(Value::as_str) == Some(field))
                .filter_map(|i| i.get("value").and_then(Value::as_str))
                .filter(|v| {
                    needle
                        .as_ref()
                        .is_none_or(|n| v.to_ascii_lowercase().contains(n))
                })
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    values.sort();
    values.dedup();
    if values.is_empty() {
        return Err(Error::Api {
            status: 404,
            code: "not_found".into(),
            message: format!(
                "no autocomplete values for field '{field}'{}",
                needle
                    .map(|n| format!(" matching '{n}'"))
                    .unwrap_or_default()
            ),
            fields: std::collections::BTreeMap::new(),
            retry_after: None,
        });
    }
    let rows: Vec<Value> = values
        .iter()
        .map(|v| json!({ "value": v, "label": v }))
        .collect();
    ctx.printer.list(&rows, None, CHOICE_COLUMNS)
}

// ------------------------------------------------------------------- check

fn check(ctx: &Context, args: &CriteriaArgs) -> Result<()> {
    let query = parse_criteria(&args.items)?;
    let understood = understand(ctx, &query)?;
    let canonical = canonical_query(&understood.criteria);
    let doc = json!({
        "description": understood.description,
        "criteria": understood.criteria,
        "ignored": understood.ignored,
        "results_url": ctx.client.site_url(&format!("search/results/?{}", encode(&canonical))),
    });
    ctx.printer.raw(&doc)?;
    if understood.ignored.is_empty() {
        Ok(())
    } else {
        Err(ignored_error(&understood.ignored))
    }
}

// --------------------------------------------------------------------- run

fn flatten_property(prop: &Value, site: &crate::client::Client) -> Value {
    let mut out = Map::new();
    if let Some(board) = prop.get("board") {
        out.insert("board".into(), board.clone());
    }
    if let Some(kind) = prop.get("type") {
        out.insert("type_code".into(), kind.clone());
    }
    if let Some(fields) = prop.get("fields").and_then(Value::as_object) {
        for (name, field) in fields {
            let data = field.get("data").cloned().unwrap_or(Value::Null);
            if name == "list_price" {
                if let Some(raw) = field.get("raw") {
                    out.insert("list_price_raw".into(), raw.clone());
                }
            }
            if name == "url" {
                if let Some(path) = data.as_str() {
                    out.insert("url".into(), Value::String(site.site_url(path)));
                    out.insert("path".into(), Value::String(path.to_string()));
                    continue;
                }
            }
            out.insert(name.clone(), data);
        }
    }
    Value::Object(out)
}

fn run_search(ctx: &Context, args: &RunArgs) -> Result<()> {
    let mut query = parse_criteria(&args.criteria.items)?;
    let understood = understand(ctx, &query)?;
    if !understood.ignored.is_empty() {
        if args.strict {
            return Err(ignored_error(&understood.ignored));
        }
        eprintln!(
            "warning: the site ignored: {}",
            understood.ignored.join(", ")
        );
    }

    query.retain(|(k, _)| {
        !matches!(
            k.as_str(),
            "page" | "per_page" | "fields" | "include_description"
        )
    });
    query.push(("page".into(), args.page.to_string()));
    query.push(("per_page".into(), args.per_page.to_string()));
    query.push((
        "fields".into(),
        args.fields
            .clone()
            .unwrap_or_else(|| DEFAULT_FIELDS.to_string()),
    ));
    query.push(("include_description".into(), "long".into()));
    if let Some(sort) = &args.sort {
        let key = format!("sort_{}", sort.trim().to_ascii_lowercase());
        if !SORT_KEYS.contains(&key.as_str()) {
            return Err(Error::Usage(
                "--sort must be highest, lowest, latest, oldest or relevance".into(),
            ));
        }
        query.retain(|(k, _)| !SORT_KEYS.contains(&k.as_str()));
        query.push((key, "true".into()));
    }

    let response = ctx.client.site_get(SEARCH_PATH, &query)?;
    let total: Option<u64> = response
        .header("x-total-count")
        .and_then(|v| v.parse().ok());
    let limited = response.header("x-total-count-limited").map(str::to_string);
    let description = response.header("x-description").map(strip_tags);
    let results: Vec<Value> = response
        .body
        .as_array()
        .map(|list| {
            list.iter()
                .map(|p| flatten_property(p, &ctx.client))
                .collect()
        })
        .unwrap_or_default();

    match ctx.printer.format {
        Format::Table => {
            if let Some(d) = &description {
                ctx.printer.note(d);
            }
            ctx.printer.list(&results, None, RESULT_COLUMNS)?;
            let count = total.map_or("?".to_string(), |t| t.to_string());
            let noun = if total == Some(1) {
                "property"
            } else {
                "properties"
            };
            let limit = limited
                .as_ref()
                .map(|r| format!(" (count limited by {r})"))
                .unwrap_or_default();
            ctx.printer.note(&format!(
                "{count} matching {noun}{limit}, page {} of {} shown",
                args.page, args.per_page
            ));
            Ok(())
        }
        Format::Json | Format::Jsonl => ctx.printer.raw(&json!({
            "total": total,
            "total_limited_by": limited,
            "description": description.or(Some(understood.description)),
            "criteria": understood.criteria,
            "ignored": understood.ignored,
            "page": args.page,
            "per_page": args.per_page,
            "results": results,
        })),
    }
}

// --------------------------------------------------------------------- url

fn url(ctx: &Context, args: &UrlArgs) -> Result<()> {
    let query = parse_criteria(&args.criteria.items)?;
    let understood = understand(ctx, &query)?;
    let canonical = canonical_query(&understood.criteria);
    let qs = encode(&canonical);
    let mut doc = json!({
        "description": understood.description,
        "criteria": understood.criteria,
        "ignored": understood.ignored,
        "results_url": ctx.client.site_url(&format!("search/results/?{qs}")),
        "results_path": format!("/search/results/?{qs}"),
        "map_path": format!("/map_search/results/?{qs}"),
    });
    if args.save {
        let saved = ctx.client.site_post(SEARCH_PATH, &canonical)?.body;
        if let Some(id) = saved.get("search_id").and_then(Value::as_str) {
            doc["search_id"] = Value::String(id.to_string());
            doc["search_id_int"] = saved.get("search_id_int").cloned().unwrap_or(Value::Null);
            doc["results_path"] = Value::String(format!("/search/results/{id}/"));
            doc["map_path"] = Value::String(format!("/map_search/results/{id}/1/"));
            doc["results_url"] =
                Value::String(ctx.client.site_url(&format!("search/results/{id}/")));
        }
    }
    ctx.printer.raw(&doc)?;
    if understood.ignored.is_empty() {
        Ok(())
    } else {
        Err(ignored_error(&understood.ignored))
    }
}

// ------------------------------------------------------- saved searches

pub fn int_to_base36(mut n: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".into();
    }
    let mut out = Vec::new();
    while n > 0 {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// Describe a saved search (as referenced by a page's `search_id`).
pub fn describe_saved(ctx: &Context, search_id_int: u64) -> Result<Value> {
    let short = int_to_base36(search_id_int);
    let query: Query = vec![
        ("per_page".into(), "1".into()),
        ("fields".into(), "url".into()),
        ("include_description".into(), "long".into()),
    ];
    let response = ctx
        .client
        .site_get(&format!("{SEARCH_PATH}{short}/"), &query)?;
    let total: Option<u64> = response
        .header("x-total-count")
        .and_then(|v| v.parse().ok());
    Ok(json!({
        "search_id": short,
        "search_id_int": search_id_int,
        "description": response.header("x-description").map(strip_tags),
        "total": total,
        "total_limited_by": response.header("x-total-count-limited"),
        "results_path": format!("/search/results/{short}/"),
        "results_url": ctx.client.site_url(&format!("search/results/{short}/")),
        "api_path": format!("/api/v2/search/{short}/"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pairs_and_query_strings() {
        let q = parse_criteria(&["city=Miami".into(), "type=res".into(), "type=con".into()])
            .unwrap_or_default();
        assert_eq!(q.len(), 3);
        let q =
            parse_criteria(&["/search/results/?list_price_min=1000000&city=Key+Biscayne#x".into()])
                .unwrap_or_default();
        assert_eq!(
            q,
            vec![
                ("list_price_min".to_string(), "1000000".to_string()),
                ("city".to_string(), "Key Biscayne".to_string())
            ]
        );
        let q = parse_criteria(&["?a=1".into()]).unwrap_or_default();
        assert_eq!(q, vec![("a".to_string(), "1".to_string())]);
        assert!(matches!(
            parse_criteria(&["nonsense".into()]),
            Err(Error::Usage(_))
        ));
    }

    #[test]
    fn polygon_values() {
        let closed = "38.78512,-77.24901;38.79870,-77.21544;38.76632,-77.19902;38.78512,-77.24901";
        assert!(check_polygon(closed).unwrap_or_default().is_empty());
        let open = "38.78512,-77.24901;38.79870,-77.21544;38.76632,-77.19902";
        let w = check_polygon(open).unwrap_or_default();
        assert!(w.len() == 1 && w[0].contains("not closed"), "{w:?}");
        let swapped = "-77.24901,38.78512;-77.21544,38.79870;-77.19902,38.76632;-77.24901,38.78512";
        let w = check_polygon(swapped).unwrap_or_default();
        assert!(w.len() == 1 && w[0].contains("lng,lat"), "{w:?}");
        let many: Vec<String> = (0..120)
            .map(|i| format!("{},{}", f64::from(i) / 1000.0, f64::from(i % 7) / 1000.0))
            .collect();
        let w = check_polygon(&many.join(";")).unwrap_or_default();
        assert!(w.iter().any(|m| m.contains("120 points")), "{w:?}");
        for bad in [
            "",
            "38.7,-77.2;38.8,-77.1",
            "38.7,-77.2;38.7,-77.2;38.7,-77.2;38.7,-77.2",
            "38.7,-77.2;38.8;38.6,-77.0",
            "38.7,-77.2;north,-77.1;38.6,-77.0",
            "38.7,-77.2;38.8,-77.1,5;38.6,-77.0",
            "38.7,-77.2;-120.8,38.1;38.6,-77.0",
            "38.7,-77.2;38.8,-190.1;38.6,-77.0",
            "38.7,-77.2;NaN,-77.1;38.6,-77.0",
        ] {
            assert!(
                matches!(check_polygon(bad), Err(Error::Usage(_))),
                "accepted {bad:?}"
            );
        }
        assert!(matches!(
            parse_criteria(&["polygon=1,2;3,4".into()]),
            Err(Error::Usage(_))
        ));
        let q = parse_criteria(&[format!("polygon={closed}")]).unwrap_or_default();
        assert_eq!(q, vec![("polygon".to_string(), closed.to_string())]);
    }

    #[test]
    fn base36() {
        assert_eq!(int_to_base36(0), "0");
        assert_eq!(int_to_base36(10), "a");
        assert_eq!(int_to_base36(36), "10");
        assert_eq!(int_to_base36(1295), "zz");
    }

    #[test]
    fn strips_tags() {
        assert_eq!(
            strip_tags("Search having <strong>City</strong>=Miami"),
            "Search having City=Miami"
        );
    }
}
