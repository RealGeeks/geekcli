//! `geekcli search …` — test and refine property-search criteria against
//! the site's legacy search API, so links written into pages and posts
//! actually filter the way they read.
//!
//! The site silently drops any criterion that is not one of its search
//! fields (so `?q=Buckhead` "works" but matches everything). These
//! commands make that visible: every criterion you pass is checked against
//! what the site echoes back, and anything ignored is reported.

use std::collections::BTreeMap;

use clap::{Args, Subcommand};
use serde_json::{json, Map, Value};

use super::Context;
use crate::client::Query;
use crate::error::{Error, Result};
use crate::output::{cell, col, sanitize, Column, Format};

pub const SEARCH_PATH: &str = "api/v2/search/";
pub const METADATA_PATH: &str = "api/v2/search/metadata/";
pub const FORM_PATH: &str = "search_forms/api/advanced_search_form.json";
/// Every field the site's search accepts for its board (the site's own
/// AI search reads it); a superset of the advanced form.
pub const CATALOG_PATH: &str = "search_forms/api/dump_uberform_fields.json";

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

pub const CATALOG_COLUMNS: &[Column] = &[
    col("attr", "/attr"),
    col("params", "/params"),
    col("label", "/label"),
    col("widget", "/widget"),
    col("default", "/default"),
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
    /// List every search field this site accepts (its field catalog)
    #[command(after_help = "Notes:
  - Fields come from the site's field catalog, every field its search accepts for the board, including ones the advanced search form leaves out (subdivision, zip, school_district …). Sites without a catalog fall back to the form, with a note on stderr.
  - Range fields are searched by their `params`, never the bare name: list_price is list_price_min / list_price_max.
  - `dynamic: true` fields take their values from the listings (city, subdivision …): `search choices <field> --all` lists them.
  - `polygon` is built in: a custom map area, not on the site's lists (see `geekcli guide polygon`).")]
    Fields,
    /// List the valid values for one field, e.g. `city`
    #[command(after_help = "Notes:
  - `--fuzzy` ranks values by edit distance to the text (values containing it first) and shows the 10 closest, with a `distance` column. Use it when a value does not match exactly, e.g. `search choices city --all --fuzzy mclean`.
  - Values are case sensitive on the site: `McLean` and `Mclean` are different cities. Copy the value exactly.
  - Sources: without --all, the site's field catalog (fixed lists such as type, price presets, frontage); for fields filled from the listings (city, subdivision …) the advanced form's default-county list. --all reads the search autocomplete index: every value, every county.")]
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
        /// Rank values by closeness to this text and show the 10 closest
        #[arg(long, value_name = "TEXT")]
        fuzzy: Option<String>,
    },
    /// Check criteria: what the site understood, what it ignored, and whether the values exist
    #[command(after_help = "Notes:
  - The site drops unknown criteria silently and the page still 'works', showing everything.
  - Exit code 5 here means a key was ignored; `search fields` lists the keys this site takes, `search choices <field>` the values (city and subdivision lists depend on the default county; add `--all` for every county).
  - Values are checked too, for fields with a choice list (city, subdivision, type …; not prices or beds). Matching is case sensitive on the site, so `city=McLean` finds nothing when the site's value is `Mclean`. A case-only difference or an unknown value is reported in `value_warnings` (with up to 3 suggestions) and on stderr. It exits 0 unless you pass --strict, which exits 5.
  - --count also runs the search and reports how many listings match; 0 is a warning (exit 5 with --strict). A clean check without --count does not prove the page will show listings.
  - Paste a whole URL to check a link already on a page.
  - polygon=lat,lng;lat,lng;… searches a custom map area (latitude first, first point repeated at the end); see `geekcli guide polygon`.")]
    Check(CheckArgs),
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
pub struct CheckArgs {
    #[command(flatten)]
    pub criteria: CriteriaArgs,
    /// Also run the search and report the number of matching listings (warns on 0)
    #[arg(long)]
    pub count: bool,
    /// Fail (exit 5) on value warnings and, with --count, on 0 matches; exit 1 if the values could not be checked
    #[arg(long)]
    pub strict: bool,
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
        SearchSub::Choices {
            field,
            search,
            all,
            fuzzy,
        } => {
            if all {
                all_choices(ctx, &field, search.as_deref(), fuzzy.as_deref())
            } else {
                choices(ctx, &field, search.as_deref(), fuzzy.as_deref())
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

/// The `search fields` row for `polygon`, shaped like the other rows.
fn polygon_field() -> Value {
    FieldRow {
        attr: json!(POLYGON),
        label: json!("Custom area: lat,lng;lat,lng;… (built in, not on the site's form; see `geekcli guide polygon`)"),
        section: "builtin",
        widget: json!("polygon"),
        default: Value::Null,
        value: Value::Null,
        depends_on: Value::Null,
        choices: Vec::new(),
        params: vec![POLYGON.to_string()],
        flags: Vec::new(),
        default_min: Value::Null,
        default_max: Value::Null,
    }
    .into_json()
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

/// Where the field rows came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldSource {
    /// The site's field catalog: every field the search accepts.
    Catalog,
    /// The advanced search form: a subset, used when the catalog is missing.
    Form,
}

impl FieldSource {
    fn name(self) -> &'static str {
        match self {
            FieldSource::Catalog => "catalog",
            FieldSource::Form => "form",
        }
    }
}

struct Fields {
    rows: Vec<Value>,
    source: FieldSource,
}

/// One `{value, label}` choice from any of the shapes the site uses:
/// `[value, label]` pairs (form), `{val|value, label}` objects (catalog)
/// or bare values.
fn choice_row(choice: &Value) -> Value {
    match choice {
        Value::Array(pair) if pair.len() == 2 => json!({ "value": pair[0], "label": pair[1] }),
        Value::Object(o) => {
            let value = o
                .get("val")
                .or_else(|| o.get("value"))
                .or_else(|| o.get("id"))
                .cloned()
                .unwrap_or(Value::Null);
            let label = o
                .get("label")
                .or_else(|| o.get("name"))
                .cloned()
                .unwrap_or_else(|| value.clone());
            json!({ "value": value, "label": label })
        }
        other => json!({ "value": other, "label": other }),
    }
}

fn choice_list(choices: Option<&Value>) -> Vec<Value> {
    choices
        .and_then(Value::as_array)
        .map(|list| list.iter().map(choice_row).collect())
        .unwrap_or_default()
}

/// A `search fields` row. Form, catalog and built-in rows share this shape.
struct FieldRow {
    attr: Value,
    label: Value,
    section: &'static str,
    widget: Value,
    default: Value,
    value: Value,
    depends_on: Value,
    choices: Vec<Value>,
    params: Vec<String>,
    flags: Vec<String>,
    default_min: Value,
    default_max: Value,
}

impl FieldRow {
    fn into_json(self) -> Value {
        let dynamic = self.flags.iter().any(|f| f == "dynamic");
        json!({
            "attr": self.attr,
            "label": self.label,
            "section": self.section,
            "widget": self.widget,
            "default": self.default,
            "value": self.value,
            "depends_on": self.depends_on,
            "choices_count": self.choices.len(),
            "choices": self.choices,
            "params": self.params,
            "flags": self.flags,
            "dynamic": dynamic,
            "default_min": self.default_min,
            "default_max": self.default_max,
        })
    }
}

fn form_fields(ctx: &Context) -> Result<Vec<Value>> {
    let form = ctx.client.site_get(FORM_PATH, &Query::new())?.body;
    let mut rows = Vec::new();
    for section in ["primary", "secondary"] {
        let Some(items) = form.get(section).and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            let get = |key: &str| item.get(key).cloned().unwrap_or(Value::Null);
            let attr = get("attr");
            rows.push(
                FieldRow {
                    params: attr.as_str().map(str::to_string).into_iter().collect(),
                    attr,
                    label: get("label"),
                    section,
                    widget: get("widget_type"),
                    default: get("default_value"),
                    value: get("value"),
                    depends_on: get("dependent_fields"),
                    choices: choice_list(item.get("choices")),
                    flags: Vec::new(),
                    default_min: Value::Null,
                    default_max: Value::Null,
                }
                .into_json(),
            );
        }
    }
    Ok(rows)
}

/// Rows from the site's field catalog: a JSON list of
/// `{id, label, widget, choices: [{val, label}], flags, default, …}`.
/// Unknown widgets and missing keys are tolerated; `None` if the body is
/// not a list of fields at all.
fn catalog_rows(body: &Value) -> Option<Vec<Value>> {
    let items = body
        .as_array()
        .or_else(|| body.get("fields").and_then(Value::as_array))?;
    let mut rows = Vec::new();
    for item in items {
        let Some(id) = ["id", "attr", "name"]
            .iter()
            .find_map(|k| item.get(*k).and_then(Value::as_str))
            .filter(|id| !id.is_empty())
        else {
            continue;
        };
        let get = |key: &str| item.get(key).cloned().unwrap_or(Value::Null);
        let flags: Vec<String> = item
            .get("flags")
            .and_then(Value::as_array)
            .map(|f| {
                f.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let widget = get("widget");
        // a range field is searched as <id>_min / <id>_max, never <id>
        let range =
            widget.as_str() == Some("min_max_field") || flags.iter().any(|f| f == "min_max_field");
        let params = if range {
            vec![format!("{id}_min"), format!("{id}_max")]
        } else {
            vec![id.to_string()]
        };
        rows.push(
            FieldRow {
                attr: json!(id),
                label: get("label"),
                section: "catalog",
                widget,
                default: get("default"),
                value: Value::Null,
                depends_on: get("child"),
                choices: choice_list(item.get("choices")),
                params,
                flags,
                default_min: get("default_min"),
                default_max: get("default_max"),
            }
            .into_json(),
        );
    }
    Some(rows)
}

fn catalog_fields(ctx: &Context) -> Result<Vec<Value>> {
    let body = ctx.client.site_get(CATALOG_PATH, &Query::new())?.body;
    catalog_rows(&body).ok_or_else(|| Error::Other("the response is not a list of fields".into()))
}

/// The site's searchable fields: the field catalog, or the advanced search
/// form on sites that do not serve the catalog (with a note on stderr when
/// `note` is set).
fn load_fields(ctx: &Context, note: bool) -> Result<Fields> {
    match catalog_fields(ctx) {
        Ok(rows) => Ok(Fields {
            rows,
            source: FieldSource::Catalog,
        }),
        Err(e) => {
            if note {
                eprintln!(
                "note: the site's field catalog could not be read ({}); using its advanced search form, which lists fewer fields",
                sanitize(&e.to_string())
            );
            }
            Ok(Fields {
                rows: form_fields(ctx)?,
                source: FieldSource::Form,
            })
        }
    }
}

fn fields(ctx: &Context) -> Result<()> {
    let Fields { mut rows, source } = load_fields(ctx, true)?;
    rows.push(polygon_field());
    if ctx.printer.format == Format::Table {
        if source == FieldSource::Catalog {
            ctx.printer.list(&rows, None, CATALOG_COLUMNS)?;
            ctx.printer.note("Use `geekcli search choices <attr>` for a field's values; range fields are searched by their params (list_price_min, list_price_max).");
            return Ok(());
        }
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

fn not_found(message: String) -> Error {
    Error::Api {
        status: 404,
        code: "not_found".into(),
        message,
        fields: BTreeMap::new(),
        retry_after: None,
    }
}

/// The rows for one search key: by `attr`, or by one of its `params`
/// (`list_price_min` finds the catalog's `list_price`).
fn field_rows<'a>(rows: &'a [Value], field: &str) -> Vec<&'a Value> {
    rows.iter()
        .filter(|r| {
            r.get("attr").and_then(Value::as_str) == Some(field)
                || r.get("params")
                    .and_then(Value::as_array)
                    .is_some_and(|p| p.iter().any(|k| k.as_str() == Some(field)))
        })
        .collect()
}

/// Choices of the given rows as `{value, label}`. Checkbox groups are one
/// row per box, each carrying its own `value`.
fn choice_rows(rows: &[&Value]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for row in rows {
        if let Some(v) = row.get("value").filter(|v| !v.is_null()) {
            out.push(json!({ "value": v, "label": row["label"] }));
        }
        if let Some(list) = row.get("choices").and_then(Value::as_array) {
            out.extend(list.iter().cloned());
        }
    }
    out
}

/// The form's choices for a field, also looked up by the catalog row's
/// params (the form lists `list_price_min`, the catalog `list_price`).
fn form_choices_for(ctx: &Context, field: &str, catalog: &[&Value]) -> Option<Vec<Value>> {
    let form = form_fields(ctx).ok()?;
    let mut names: Vec<String> = vec![field.to_string()];
    for row in catalog {
        for key in ["attr", "params"] {
            match row.get(key) {
                Some(Value::String(s)) => names.push(s.clone()),
                Some(Value::Array(list)) => {
                    names.extend(list.iter().filter_map(Value::as_str).map(str::to_string));
                }
                _ => {}
            }
        }
    }
    names.sort();
    names.dedup();
    let rows: Vec<&Value> = form
        .iter()
        .filter(|r| {
            r.get("attr")
                .and_then(Value::as_str)
                .is_some_and(|a| names.iter().any(|n| n == a))
        })
        .collect();
    let out = choice_rows(&rows);
    (!out.is_empty()).then_some(out)
}

/// `search choices <field>`: the catalog's fixed list; for fields whose
/// values come from the listings (city, subdivision …; empty in the
/// catalog) the advanced form's default-county list; `--all` reads the
/// autocomplete index instead.
fn choices(ctx: &Context, field: &str, needle: Option<&str>, fuzzy: Option<&str>) -> Result<()> {
    if field == POLYGON {
        return Err(Error::Usage(
            "polygon has no list of values: it takes lat,lng;lat,lng;… points (see `geekcli guide polygon`)".into(),
        ));
    }
    let fields = load_fields(ctx, true)?;
    let matching = field_rows(&fields.rows, field);
    if matching.is_empty() {
        return Err(not_found(format!(
            "no search field named '{field}' on this site; see `geekcli search fields`"
        )));
    }
    let mut out = choice_rows(&matching);
    let mut source = match fields.source {
        FieldSource::Catalog => "the site's field catalog",
        FieldSource::Form => "the advanced search form",
    };
    if out.is_empty() && fields.source == FieldSource::Catalog {
        if let Some(form) = form_choices_for(ctx, field, &matching) {
            out = form;
            source = "the advanced search form (default county)";
        }
    }
    if out.is_empty() {
        eprintln!(
            "note: '{field}' has no fixed list of values; `geekcli search choices {field} --all` lists every value the site's autocomplete knows"
        );
    } else {
        ctx.printer.note(&format!("Values from {source}."));
    }
    if let Some(needle) = needle {
        let needle = needle.to_ascii_lowercase();
        out.retain(|c| {
            cell(&c["value"]).to_ascii_lowercase().contains(&needle)
                || cell(&c["label"]).to_ascii_lowercase().contains(&needle)
        });
    }
    print_choices(ctx, out, fuzzy)
}

/// Print choice rows; with `--fuzzy`, ranked by closeness and cut to the
/// closest few, each with its `distance`.
fn print_choices(ctx: &Context, mut rows: Vec<Value>, fuzzy: Option<&str>) -> Result<()> {
    let Some(text) = fuzzy else {
        return ctx.printer.list(&rows, None, CHOICE_COLUMNS);
    };
    let mut scored: Vec<((usize, usize), Value)> = rows
        .drain(..)
        .map(|row| {
            let score = fuzzy_score(text, &cell(&row["value"]))
                .min(fuzzy_score(text, &cell(&row["label"])));
            (score, row)
        })
        .collect();
    scored.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| cell(&a.1["value"]).cmp(&cell(&b.1["value"])))
    });
    let ranked: Vec<Value> = scored
        .into_iter()
        .take(FUZZY_LIMIT)
        .map(|((_, distance), mut row)| {
            row["distance"] = json!(distance);
            row
        })
        .collect();
    ctx.printer.list(&ranked, None, FUZZY_COLUMNS)
}

pub const AUTOCOMPLETE_PATH: &str = "api/v2/search/autocomplete-options/";

/// The site's autocomplete index (`[{"field", "value"}, …]`), which covers
/// all cities, unlike the advanced form's choice list that follows the
/// default county.
fn autocomplete_index(ctx: &Context) -> Result<Value> {
    Ok(ctx.client.site_get(AUTOCOMPLETE_PATH, &Query::new())?.body)
}

/// Sorted, de-duplicated values for one field from the autocomplete index.
fn autocomplete_values(index: &Value, field: &str) -> Vec<String> {
    let mut values: Vec<String> = index
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter(|i| i.get("field").and_then(Value::as_str) == Some(field))
                .filter_map(|i| i.get("value").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    values.sort();
    values.dedup();
    values
}

/// Every known value for a field from the site's autocomplete index.
fn all_choices(
    ctx: &Context,
    field: &str,
    needle: Option<&str>,
    fuzzy: Option<&str>,
) -> Result<()> {
    let index = autocomplete_index(ctx)?;
    let needle = needle.map(str::to_ascii_lowercase);
    let mut values = autocomplete_values(&index, field);
    if let Some(n) = &needle {
        values.retain(|v| v.to_ascii_lowercase().contains(n));
    }
    if values.is_empty() {
        return Err(not_found(format!(
            "no autocomplete values for field '{field}'{}",
            needle
                .map(|n| format!(" matching '{n}'"))
                .unwrap_or_default()
        )));
    }
    let rows: Vec<Value> = values
        .iter()
        .map(|v| json!({ "value": v, "label": v }))
        .collect();
    print_choices(ctx, rows, fuzzy)
}

// ------------------------------------------------------------ fuzzy match

/// How many values `choices --fuzzy` shows.
const FUZZY_LIMIT: usize = 10;
/// How many suggestions a value warning carries.
const SUGGESTION_LIMIT: usize = 3;

pub const FUZZY_COLUMNS: &[Column] = &[
    col("value", "/value"),
    col("label", "/label"),
    col("distance", "/distance"),
];

/// Levenshtein distance over characters.
fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.chars().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != *cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Sort key for how well `candidate` matches `text`, ignoring case: values
/// that contain the text come first, then by edit distance. The second
/// element is the edit distance itself.
fn fuzzy_score(text: &str, candidate: &str) -> (usize, usize) {
    let text = text.to_lowercase();
    let candidate = candidate.to_lowercase();
    let distance = levenshtein(&text, &candidate);
    let contains = !text.is_empty() && candidate.contains(&text);
    (usize::from(!contains), distance)
}

/// Up to `limit` candidates closest to `value`.
fn closest(value: &str, candidates: &[String], limit: usize) -> Vec<String> {
    let mut scored: Vec<((usize, usize), &String)> = candidates
        .iter()
        .map(|c| (fuzzy_score(value, c), c))
        .collect();
    scored.sort();
    scored
        .into_iter()
        .take(limit)
        .map(|(_, c)| c.clone())
        .collect()
}

// ------------------------------------------------------------------- check

/// A choice list made only of numbers (prices, beds, square feet) is a set
/// of presets, not the only values the search takes.
fn is_numeric_list(values: &[String]) -> bool {
    let mut any = false;
    for v in values {
        if v.is_empty() || v.eq_ignore_ascii_case("all") {
            continue;
        }
        if v.replace(',', "").parse::<f64>().is_err() {
            return false;
        }
        any = true;
    }
    any
}

/// Result of comparing criteria values with the site's choices.
#[derive(Default)]
struct ValueCheck {
    /// Fields whose values were compared with a choice list.
    checked: Vec<String>,
    warnings: Vec<Value>,
    /// Why the values could not be checked, if a choice source failed.
    error: Option<String>,
    /// Where the choice lists came from, when they were read.
    source: Option<FieldSource>,
}

/// Compare each criterion value with the field's choices: the catalog's
/// list (or the advanced form's, on sites without a catalog), plus the
/// autocomplete index (every county) when that alone does not settle it.
/// Each source is fetched at most once.
fn check_values(ctx: &Context, query: &Query, ignored: &[String]) -> ValueCheck {
    let mut by_field: Vec<(String, Vec<String>)> = Vec::new();
    for (k, v) in query {
        // polygon is free-form (validated by check_polygon), not a choice
        if is_control(k) || ignored.contains(k) || v.is_empty() || k == POLYGON {
            continue;
        }
        match by_field.iter_mut().find(|(f, _)| f == k) {
            Some((_, values)) if !values.contains(v) => values.push(v.clone()),
            Some(_) => {}
            None => by_field.push((k.clone(), vec![v.clone()])),
        }
    }
    let mut result = ValueCheck::default();
    if by_field.is_empty() {
        return result;
    }
    let fields = match load_fields(ctx, false) {
        Ok(fields) => {
            result.source = Some(fields.source);
            fields
        }
        Err(e) => {
            result.error = Some(format!("could not read the search form: {e}"));
            return result;
        }
    };
    let mut index: Option<Value> = None;
    for (field, values) in by_field {
        let rows = field_rows(&fields.rows, &field);
        // the site takes true/1/yes for yes-no fields, not just the listed values
        if rows
            .iter()
            .any(|r| r.get("widget").and_then(Value::as_str) == Some("boolean"))
        {
            continue;
        }
        let mut candidates: Vec<String> = choice_rows(&rows)
            .iter()
            .map(|c| cell(&c["value"]))
            .collect();
        // the form submits `all` for "no preference" on any listed field
        let values: Vec<String> = if candidates.is_empty() {
            values
        } else {
            values.into_iter().filter(|v| v != "all").collect()
        };
        if is_numeric_list(&candidates) {
            continue;
        }
        let settled = !candidates.is_empty() && values.iter().all(|v| candidates.contains(v));
        if !settled {
            if index.is_none() {
                match autocomplete_index(ctx) {
                    Ok(body) => index = Some(body),
                    Err(e) => {
                        result.error = Some(format!("could not read the autocomplete index: {e}"));
                        index = Some(Value::Null);
                    }
                }
            }
            if let Some(body) = &index {
                candidates.extend(autocomplete_values(body, &field));
            }
        }
        candidates.sort();
        candidates.dedup();
        if candidates.is_empty() || is_numeric_list(&candidates) {
            continue;
        }
        for value in &values {
            if let Some(w) = value_warning(&field, value, &candidates) {
                result.warnings.push(w);
            }
        }
        result.checked.push(field);
    }
    result
}

fn value_warning(field: &str, value: &str, candidates: &[String]) -> Option<Value> {
    if candidates.iter().any(|c| c == value) {
        return None;
    }
    let lower = value.to_lowercase();
    let same_case: Vec<String> = candidates
        .iter()
        .filter(|c| c.to_lowercase() == lower)
        .cloned()
        .collect();
    let (kind, suggestions, message) = if same_case.is_empty() {
        let suggestions = closest(value, candidates, SUGGESTION_LIMIT);
        let hint = if suggestions.is_empty() {
            String::new()
        } else {
            format!("; did you mean {}?", quoted(&suggestions))
        };
        let message = format!("{field}={value} is not one of the site's values for {field}{hint}");
        ("unknown_value", suggestions, message)
    } else {
        let message = format!(
            "{field}={value} differs from the site's value only in case and will match nothing; did you mean {}?",
            quoted(&same_case)
        );
        ("case_mismatch", same_case, message)
    };
    Some(json!({
        "field": field,
        "value": value,
        "kind": kind,
        "suggestions": suggestions,
        "message": message,
    }))
}

fn quoted(values: &[String]) -> String {
    values
        .iter()
        .map(|v| format!("`{v}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Total matches for a query, read from the search's count header.
fn count_matches(ctx: &Context, query: &Query) -> Result<(Option<u64>, Option<String>)> {
    let mut query = query.clone();
    query.retain(|(k, _)| !matches!(k.as_str(), "page" | "per_page" | "fields"));
    query.push(("per_page".into(), "1".into()));
    query.push(("fields".into(), "url".into()));
    let response = ctx.client.site_get(SEARCH_PATH, &query)?;
    let total = response
        .header("x-total-count")
        .and_then(|v| v.parse().ok());
    let limited = response.header("x-total-count-limited").map(str::to_string);
    Ok((total, limited))
}

fn check(ctx: &Context, args: &CheckArgs) -> Result<()> {
    let query = parse_criteria(&args.criteria.items)?;
    let understood = understand(ctx, &query)?;
    let canonical = canonical_query(&understood.criteria);
    let values = check_values(ctx, &query, &understood.ignored);

    let mut warnings: Vec<String> = values
        .warnings
        .iter()
        .filter_map(|w| w["message"].as_str().map(str::to_string))
        .collect();
    let mut doc = json!({
        "description": understood.description,
        "criteria": understood.criteria,
        "ignored": understood.ignored,
        "values_checked": values.checked,
        "value_warnings": values.warnings,
        "results_url": ctx.client.site_url(&format!("search/results/?{}", encode(&canonical))),
    });
    if let Some(e) = &values.error {
        doc["value_check_error"] = json!(e);
    }
    if let Some(source) = values.source {
        doc["choices_source"] = json!(source.name());
    }
    let mut no_matches = false;
    if args.count {
        let (total, limited) = count_matches(ctx, &canonical)?;
        doc["count"] = json!(total);
        doc["count_limited_by"] = json!(limited);
        if total == Some(0) {
            no_matches = true;
            warnings.push("the search matches 0 listings".into());
        }
    }
    doc["warnings"] = json!(warnings);
    ctx.printer.raw(&doc)?;

    let mut fields: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for w in &values.warnings {
        if let (Some(f), Some(m)) = (w["field"].as_str(), w["message"].as_str()) {
            fields.entry(f.to_string()).or_default().push(m.to_string());
        }
    }
    if !understood.ignored.is_empty() {
        let mut err = ignored_error(&understood.ignored);
        if let Error::Api { fields: f, .. } = &mut err {
            for (k, v) in fields {
                f.entry(k).or_default().extend(v);
            }
        }
        return Err(err);
    }
    if args.strict && (!fields.is_empty() || no_matches) {
        let message = if fields.is_empty() {
            "the search matches 0 listings".to_string()
        } else {
            format!(
                "{} value(s) do not match the site's choices: {}",
                values.warnings.len(),
                warnings.join("; ")
            )
        };
        return Err(Error::Api {
            status: 422,
            code: if fields.is_empty() {
                "no_matches".into()
            } else {
                "value_mismatch".into()
            },
            message,
            fields,
            retry_after: None,
        });
    }
    // --strict promises the values were checked; say so when they could not be
    if let (true, Some(e)) = (args.strict, &values.error) {
        return Err(Error::Api {
            status: 503,
            code: "value_check_unavailable".into(),
            message: format!("values could not be checked, so --strict cannot pass: {e}"),
            fields: BTreeMap::new(),
            retry_after: None,
        });
    }
    // Messages carry values from the site, so strip control characters
    if let Some(e) = &values.error {
        eprintln!("warning: values not checked: {}", sanitize(e));
    }
    for w in &warnings {
        eprintln!("warning: {}", sanitize(w));
    }
    Ok(())
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
        assert!(matches!(
            check_polygon(closed).unwrap_or_default().as_slice(),
            []
        ));
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

    #[test]
    fn edit_distance_and_suggestions() {
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("Mclean", "Mclean"), 0);
        let candidates: Vec<String> = [
            "Riverside",
            "Fairview Beach",
            "Miami",
            "Island at Riverside",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
        assert_eq!(
            closest("riverside", &candidates, 2),
            ["Riverside", "Island at Riverside"]
        );
        assert_eq!(closest("Riversdie", &candidates, 1), ["Riverside"]);
    }

    #[test]
    fn numeric_choice_lists_are_presets() {
        let list = |v: &[&str]| v.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
        assert!(is_numeric_list(&list(&["all", "100000", "1,000,000"])));
        assert!(!is_numeric_list(&list(&["all"])));
        assert!(!is_numeric_list(&list(&["Miami", "33458"])));
    }
}
