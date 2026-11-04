//! 이 컴퓨터로 채굴하는 것과 그 둘레 설정의 한국어 글.
pub const WORDS: &[(&str, &str)] = &[
    ("storage.merge_about", "작은 코인이 16개 쌓이면 8개를 하나로 합칩니다. 보낼 때 쓰는 코인 수가 줄어듭니다."),
    ("local.preset_computer", "컴퓨터 크기"),
];

pub const EXPERT: &[(&str, &str)] = &[
    ("pro.threads", "채굴 스레드"),
    ("pro.threads_about", "이 컴퓨터 코어는 {}개입니다. 이 앱과 더한 채굴기 모두 이 수만큼 스레드를 쓰고 제한 하나를 나눠 씁니다."),
    ("storage.merge_about", "UTXO가 16개 쌓이면 8개를 하나로 묶습니다. 비트코인 코어처럼 기본은 꺼짐입니다."),
];
