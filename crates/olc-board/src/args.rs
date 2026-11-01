//! Reads `word --flag value --switch` command lines.
use std::collections::BTreeMap;

pub struct Args {
    words: Vec<String>,
    flags: BTreeMap<String, Vec<String>>,
}

impl Args {
    pub fn parse(items: impl Iterator<Item = String>) -> Self {
        let mut words = Vec::new();
        let mut flags: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut items = items.peekable();
        while let Some(item) = items.next() {
            match item.strip_prefix("--") {
                Some(name) => {
                    let value = items
                        .next_if(|next| !next.starts_with("--"))
                        .unwrap_or_default();
                    flags.entry(name.to_string()).or_default().push(value);
                }
                None => words.push(item),
            }
        }
        Self { words, flags }
    }

    pub fn command(&self) -> Option<&str> {
        self.words.first().map(String::as_str)
    }

    pub fn word(&self, index: usize) -> Option<&str> {
        self.words.get(index).map(String::as_str)
    }

    pub fn value(&self, name: &str) -> Option<&str> {
        self.flags.get(name)?.last().map(String::as_str)
    }

    pub fn values(&self, name: &str) -> Vec<String> {
        self.flags.get(name).cloned().unwrap_or_default()
    }

    pub fn has(&self, name: &str) -> bool {
        self.flags.contains_key(name)
    }

    pub fn number<T: std::str::FromStr>(&self, name: &str, default: T) -> Result<T, String> {
        match self.value(name) {
            Some(text) => text.parse().map_err(|_| format!("--{name} needs a number")),
            None => Ok(default),
        }
    }
}
