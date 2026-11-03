//! Every amount a notice gives carries the coin's unit: "1 OLC sent", never "1 sent". Notices are
//! made in the service and the command line; an amount reaches one only through `amount_of` or
//! `Host::amount_text`, never as a bare `format_coins`.
use std::fs;
use std::path::Path;

fn sources(folder: &Path, found: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(folder).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push((path.display().to_string(), fs::read_to_string(&path).unwrap()));
        }
    }
}

/// A line that names a notice key, as `("notice.sent", ...)` or `said("lock.paid", ...)` do.
fn names_a_notice(line: &str) -> bool {
    ["(\"notice.", "(\"lock.", "say(\"notice.", "said(\"notice."].iter().any(|start| line.contains(start))
}

#[test]
fn no_notice_gives_an_amount_without_its_unit() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut found = Vec::new();
    sources(&root.join("src"), &mut found);
    sources(&root.join("../olc-cli/src"), &mut found);
    let mut bare = Vec::new();
    for (path, text) in &found {
        let lines: Vec<&str> = text.lines().collect();
        for (at, line) in lines.iter().enumerate() {
            // An amount formatted on the line before a notice reaches it the same way.
            let near = [lines.get(at.wrapping_sub(1)).copied().unwrap_or(""), line];
            if names_a_notice(line) && near.iter().any(|line| line.contains("format_coins(")) {
                bare.push(format!("{path}:{}: {}", at + 1, line.trim()));
            }
        }
    }
    assert!(bare.is_empty(), "notices with a bare amount:\n{}", bare.join("\n"));
    assert!(found.iter().any(|(_, text)| text.contains("fn amount_of(")), "the one formatter is gone");
}
