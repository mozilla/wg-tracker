use lazy_static::lazy_static;
use regex::{Captures, Regex};

pub fn escape_markdown(s: &str) -> String {
    ESCAPE_MARKDOWN_RE
        .replace_all(s, |cs: &Captures| {
            let c = cs.get(0).unwrap().as_str().chars().next().unwrap();
            let s = match c {
                '\\' => "\\\\",
                '&' => "&amp;",
                '<' => "&lt;",
                '>' => "&gt;",
                '|' => "&124;",
                _ => return format!("\\{}", c),
            };
            String::from(s)
        })
        .into_owned()
}

pub fn extract_urls(s: &str) -> Vec<String> {
    MARKDOWN_URLS_RE
        .captures_iter(s)
        .map(|m| m.get(1).unwrap().as_str().to_string())
        .collect()
}

lazy_static! {
    pub static ref CLIENT: reqwest::Client = reqwest::Client::new();
    pub static ref ESCAPE_MARKDOWN_RE: Regex = Regex::new(r#"[#&()*+<>\[\]\\_`|-]"#).unwrap();
    pub static ref MARKDOWN_URLS_RE: Regex = Regex::new(r#"\((https:[^)]*)"#).unwrap();
}
