//! A Korean particle such as 는/은, 를/을, 가/이, 로/으로 takes its form from the last sound of
//! the word before it. When that word is a filled-in value (an amount, a speed, a name, a block
//! number), the right form changes with the value, so "초당 20,139번로" or "블록 1,232이" slip
//! through. Korean lines put such values where no particle follows them.

use std::fs;
use std::path::Path;

/// Particles whose form depends on the sound before them. 의, 에, 에서, 부터, 까지 and the copula
/// 입니다 read the same after any value and are allowed.
const PARTICLES: [&str; 14] = [
    "으로", "이랑", "는", "은", "를", "을", "가", "이", "와", "과", "로", "랑", "야", "아",
];

fn is_hangul(c: char) -> bool {
    ('\u{AC00}'..='\u{D7A3}').contains(&c)
}

/// The particles that follow a `{}` in `line` and end a word there.
fn particles_after_values(line: &str) -> Vec<&'static str> {
    let mut found = Vec::new();
    for (at, _) in line.match_indices("{}") {
        let rest = &line[at + 2..];
        for particle in PARTICLES {
            if let Some(after) = rest.strip_prefix(particle) {
                if !after.chars().next().is_some_and(is_hangul) {
                    found.push(particle);
                    break;
                }
            }
        }
    }
    found
}

#[test]
fn the_check_catches_a_particle_after_a_value() {
    assert_eq!(particles_after_values(r#"("home.mining", "{}로 채굴 중"),"#), ["로"]);
    assert_eq!(particles_after_values(r#"("x", "블록 {}이 없습니다."),"#), ["이"]);
    assert!(particles_after_values(r#"("x", "{} 채굴 중입니다. 블록 {}부터 있습니다."),"#).is_empty());
    assert!(particles_after_values(r#"("x", "이 지갑의 주소는 {}입니다."),"#).is_empty());
}

#[test]
fn no_korean_line_puts_a_particle_right_after_a_value() {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/i18n");
    let mut wrong = Vec::new();
    let mut tables = 0;
    for entry in fs::read_dir(&folder).expect("the i18n folder") {
        let path = entry.expect("an i18n file").path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_owned();
        if !(name.starts_with("ko") && name.ends_with(".rs")) {
            continue;
        }
        tables += 1;
        let text = fs::read_to_string(&path).expect("a Korean table");
        for (number, line) in text.lines().enumerate() {
            for particle in particles_after_values(line) {
                wrong.push(format!("{name}:{}: {{}}{particle}: {}", number + 1, line.trim()));
            }
        }
    }
    assert!(tables >= 4, "found {tables} Korean tables in {}", folder.display());
    assert!(wrong.is_empty(), "particles that change with the value:\n{}", wrong.join("\n"));
}
