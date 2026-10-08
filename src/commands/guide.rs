//! `geekcli guide [TOPIC]` — the embedded guide, whole or one section at
//! a time so an agent can load only what it needs.

use clap::Args;

use crate::error::{Error, Result};

#[derive(Debug, Args)]
pub struct GuideArgs {
    /// A section number or a word from its title, e.g. `html`, `rebrand`, `search`, `12`
    #[arg(value_name = "TOPIC")]
    pub topic: Option<String>,
    /// List the section titles
    #[arg(long)]
    pub list: bool,
}

pub struct Section<'a> {
    pub number: usize,
    pub title: &'a str,
    pub body: &'a str,
}

/// Split the guide at its `## N. Title` headings.
pub fn sections(guide: &str) -> Vec<Section<'_>> {
    let mut out = Vec::new();
    let mut starts: Vec<(usize, usize, &str)> = Vec::new();
    for (offset, line) in line_offsets(guide) {
        if let Some(rest) = line.strip_prefix("## ") {
            if let Some((num, title)) = rest.split_once(". ") {
                if let Ok(number) = num.trim().parse::<usize>() {
                    starts.push((offset, number, title.trim()));
                }
            }
        }
    }
    for (i, (offset, number, title)) in starts.iter().enumerate() {
        let end = starts.get(i + 1).map_or(guide.len(), |s| s.0);
        out.push(Section {
            number: *number,
            title,
            body: guide[*offset..end].trim_end(),
        });
    }
    out
}

fn line_offsets(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut offset = 0;
    text.split_inclusive('\n').map(move |line| {
        let start = offset;
        offset += line.len();
        (start, line.trim_end_matches('\n'))
    })
}

/// Sections matching a topic: by number, else by a case-insensitive word
/// in the title (with a few aliases agents are likely to reach for).
pub fn find(all: &[Section<'_>], topic: &str) -> Vec<usize> {
    let wanted = topic.trim().to_ascii_lowercase();
    if let Ok(n) = wanted.parse::<usize>() {
        return all
            .iter()
            .enumerate()
            .filter(|(_, s)| s.number == n)
            .map(|(i, _)| i)
            .collect();
    }
    let aliases: &[(&str, &str)] = &[
        ("html", "content that renders"),
        ("sanitizer", "content that renders"),
        ("css", "content that renders"),
        ("style", "content that renders"),
        ("layout", "content that renders"),
        ("rebrand", "rebranding"),
        ("branding", "rebranding"),
        ("logo", "rebranding"),
        ("auth", "setup"),
        ("login", "setup"),
        ("errors", "exit codes"),
        ("exit", "exit codes"),
        ("json", "output"),
        ("criteria", "search"),
        ("links", "search"),
        ("polygon", "property-search"),
        ("map", "property-search"),
        ("nav", "navigation"),
        ("screenshot", "snapshots"),
        ("workflow", "workflow"),
        ("api", "anything else"),
        ("support", "support articles"),
        ("design guidance", "design guidance"),
        ("guidance", "design guidance"),
        ("docs", "support articles"),
        ("articles", "support articles"),
        ("raw", "anything else"),
        ("update", "updating geekcli"),
        ("upgrade", "updating geekcli"),
        ("version", "updating geekcli"),
        ("revert", "revisions and undo"),
        ("history", "revisions and undo"),
        ("restore", "files"),
    ];
    let needle = aliases
        .iter()
        .find(|(k, _)| *k == wanted)
        .map_or(wanted.as_str(), |(_, v)| v);
    all.iter()
        .enumerate()
        .filter(|(_, s)| s.title.to_ascii_lowercase().contains(needle))
        .map(|(i, _)| i)
        .collect()
}

pub fn run(args: &GuideArgs) -> Result<()> {
    let all = sections(crate::GUIDE);
    if args.list {
        for s in &all {
            println!("{:>2}  {}", s.number, s.title);
        }
        return Ok(());
    }
    let Some(topic) = &args.topic else {
        print!("{}", crate::GUIDE);
        return Ok(());
    };
    let hits = find(&all, topic);
    if hits.is_empty() {
        let titles: Vec<String> = all
            .iter()
            .map(|s| format!("{}. {}", s.number, s.title))
            .collect();
        return Err(Error::Usage(format!(
            "no guide section matches '{topic}'. Sections:\n  {}",
            titles.join("\n  ")
        )));
    }
    for (n, i) in hits.iter().enumerate() {
        if n > 0 {
            println!();
        }
        println!("{}", all[*i].body);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "# Guide\n\nintro\n\n## 1. Setup\n\nlogin here\n\n## 2. Writing content that renders\n\nrules\n\n## 3. Exit codes\n\ncodes\n";

    #[test]
    fn splits_sections() {
        let all = sections(DOC);
        assert_eq!(all.len(), 3);
        assert_eq!(all[1].title, "Writing content that renders");
        assert!(all[1].body.starts_with("## 2.") && all[1].body.ends_with("rules"));
    }

    #[test]
    fn finds_by_number_word_and_alias() {
        let all = sections(DOC);
        assert_eq!(find(&all, "3"), vec![2]);
        assert_eq!(find(&all, "Content"), vec![1]);
        assert_eq!(find(&all, "html"), vec![1]);
        assert_eq!(find(&all, "errors"), vec![2]);
        assert_eq!(find(&all, "nope"), Vec::<usize>::new());
    }

    #[test]
    fn real_guide_has_the_expected_topics() {
        let all = sections(crate::GUIDE);
        for topic in [
            "html",
            "rebrand",
            "search",
            "polygon",
            "map",
            "snapshots",
            "settings",
            "files",
            "footers",
            "banners",
            "market",
            "workflow",
            "update",
        ] {
            assert!(!find(&all, topic).is_empty(), "no section for {topic}");
        }
    }
}
