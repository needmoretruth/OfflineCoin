//! The experiment's report lines in Korean. The tool writes English lines, so this reads them by
//! shape: `{}` stands for one word of the line, and the Korean puts them back by number.
use super::Lang;

const LINES: &[(&str, &str)] = &[
    ("Attacker {} at height {}", "공격자 $1, 높이 $2"),
    ("Victim {} at height {}", "피해자 $1, 높이 $2"),
    ("1. Hash power", "1. 해시 비율"),
    ("attacker {} H/s, {}% of all hashing", "공격자 $1 H/s, 전체의 $2%"),
    ("victim {} H/s, {}% of all hashing", "피해자 $1 H/s, 전체의 $2%"),
    ("Both boards agree at height {}", "두 보드가 높이 $1에서 같습니다"),
    ("2. The attacker pays the victim {} OLC", "2. 공격자가 피해자에게 $1 OLC를 보냅니다"),
    ("victim balance: {} before, {} confirmed, {} now", "피해자 잔액: 처음 $1, 확인 때 $2, 지금 $3"),
    ("victim balance {} OLC", "피해자 잔액 $1 OLC"),
    ("payment {} reached the victim", "송금 $1 피해자에게 도착"),
    ("3. The attacker goes dark", "3. 공격자가 네트워크를 끊습니다"),
    ("4. The victim mines the payment", "4. 피해자가 송금을 채굴합니다"),
    ("payment at height {}, {} of {} confirmations", "송금이 높이 $1에 들어갔습니다. 확인 $2/$3"),
    ("payment confirmed, victim balance {} OLC", "송금 확인 끝. 피해자 잔액 $1 OLC"),
    ("5. The attacker spends the same coins on itself, out of sight", "5. 공격자가 같은 코인을 몰래 자기에게 보냅니다"),
    ("private spend: at height {}", "몰래 보낸 송금: 높이 $1"),
    ("private spend {}", "몰래 보낸 송금 $1"),
    ("6. The private chain against the public chain", "6. 숨긴 체인과 공개 체인의 경쟁"),
    ("attacker height {} work {} at {} H/s | victim height {} work {} at {} H/s", "공격자 높이 $1 일 $2, $3 H/s | 피해자 높이 $4 일 $5, $6 H/s"),
    ("the private chain is heavier", "숨긴 체인이 더 무거워졌습니다"),
    ("7. The attacker comes back", "7. 공격자가 돌아옵니다"),
    ("the victim dropped {} blocks at height {}", "피해자가 높이 $2에서 블록 $1개를 버렸습니다"),
    ("8. The payment goes back to the pool and is refused", "8. 송금이 풀로 돌아갔다가 거부됩니다"),
    ("the dropped blocks gave payment {} back", "버려진 블록의 송금 $1 풀로 돌아옴"),
    ("refused: its coins are already spent on the winning chain", "거부: 이 코인은 이긴 체인에서 이미 쓰였습니다"),
    ("9. Result", "9. 결과"),
    ("winning chain height {}", "이긴 체인 높이 $1"),
    ("payment: gone", "송금: 사라짐"),
    ("Both boards are back to their cap and network state.", "두 보드가 원래 상한과 네트워크 상태로 돌아왔습니다."),
];

/// The words of `line` where `shape` has `{}`, when the line has that shape.
fn read<'a>(shape: &str, mut line: &'a str) -> Option<Vec<&'a str>> {
    let mut words = Vec::new();
    let mut parts = shape.split("{}");
    line = line.strip_prefix(parts.next()?)?;
    for part in parts {
        let end = line.find(char::is_whitespace).unwrap_or(line.len());
        let end = if part.starts_with(char::is_whitespace) || part.is_empty() {
            end
        } else {
            line.find(part)?
        };
        words.push(&line[..end]);
        line = line[end..].strip_prefix(part)?;
    }
    Some(words)
}

fn korean(line: &str) -> Option<String> {
    let start = line.len() - line.trim_start().len();
    let body = line.trim();
    let (shape, words) = LINES
        .iter()
        .find_map(|(shape, korean)| read(shape, body).map(|words| ((shape, korean), words)))?;
    let mut out = (*shape.1).to_string();
    for (index, word) in words.iter().enumerate().rev() {
        out = out.replace(&format!("${}", index + 1), word);
    }
    Some(format!("{}{out}", &line[..start]))
}

pub fn lab_line(lang: Lang, line: &str) -> String {
    let translated = match lang {
        Lang::En => None,
        Lang::Ko => korean(line),
    };
    translated.unwrap_or_else(|| line.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_read_by_shape() {
        assert_eq!(
            lab_line(Lang::Ko, "   attacker 15000 H/s, 83.3% of all hashing"),
            "   공격자 15000 H/s, 전체의 83.3%"
        );
        assert_eq!(
            lab_line(Lang::Ko, "   the victim dropped 3 blocks at height 41"),
            "   피해자가 높이 41에서 블록 3개를 버렸습니다"
        );
        assert_eq!(lab_line(Lang::Ko, "Something new"), "Something new");
        assert_eq!(lab_line(Lang::En, "9. Result"), "9. Result");
    }
}
