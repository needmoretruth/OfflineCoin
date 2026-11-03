//! 이 컴퓨터로 채굴하는 것과 그 둘레 설정의 한국어 글.
pub const WORDS: &[(&str, &str)] = &[
    ("settings.speed_quarter", "4분의 1"),
    ("settings.device_about", "이 컴퓨터에서 채굴기를 몇 개 만들지 정합니다. 다른 설정은 그대로입니다."),
    ("storage.merge_about", "작은 코인이 16개 쌓이면 8개를 하나로 합칩니다. 보낼 때 쓰는 코인 수가 줄어듭니다."),
    ("local.preset_computer", "컴퓨터 크기"),
];

pub const EXPERT: &[(&str, &str)] = &[
    ("settings.speed_about", "이 컴퓨터 최대는 {}입니다. 스레드 {}개 기준입니다."),
    ("pro.cap_about", "0은 상한 없음입니다. Enter를 누르거나 칸을 벗어나면 적용됩니다."),
    ("pro.threads", "채굴 스레드"),
    ("pro.threads_about", "이 컴퓨터 코어는 {}개입니다. 상한은 모든 스레드가 나눠 씁니다."),
    ("storage.merge_about", "UTXO가 16개 쌓이면 8개를 하나로 묶습니다. 비트코인 코어처럼 기본은 꺼짐입니다."),
    ("settings.device_about", "로컬 노드를 몇 개 만들지 정합니다. 상한과 스위치는 그대로입니다."),
];
