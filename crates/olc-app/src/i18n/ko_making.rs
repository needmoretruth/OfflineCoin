//! 다른 컴퓨터의 풀에 여러 기기를 한 번에 넣는 말, 콘솔 체인 단어의 알림이 화면에 뜨는 말, 실험실
//! 코인에 블록을 바로 만드는 말.
pub const WORDS: &[(&str, &str)] = &[
    ("pool.join_who", "함께 들어갈 기기"),
    ("pool.join_choose", "들어갈 기기를 하나 이상 고르세요."),
    (
        "pool.join_boards_about",
        "보드는 이 앱을 거쳐 들어갑니다. 앱이 풀의 일을 받아 보드마다 나눠 주고, 무선으로만 잡히는 보드는 케이블에 꽂힌 보드를 통해 받습니다. 기기마다 이름이 따로 있어서 풀이 따로 세고 따로 보냅니다.",
    ),
    ("notice.waiting_list", "{}"),
    ("notice.coins_list", "{}"),
    ("notice.chain_closed", "아직 여는 중입니다. 잠시 뒤 다시 해 보세요."),
    ("notice.making", "블록 {}개를 최대 속도로 만드는 중입니다."),
    ("notice.made_block", "블록을 하나 만들었습니다: {}"),
    ("notice.made", "말한 블록을 다 만들었습니다."),
    ("notice.making_status", "{}"),
    ("notice.making_stopped", "블록 만들기를 멈췄습니다."),
    ("notice.not_making", "지금 만드는 블록이 없습니다."),
    ("notice.making_count", "블록 수를 1에서 1,000 사이로 적으세요."),
    ("notice.making_already", "이미 블록을 만드는 중입니다."),
    ("notice.making_in_pool", "이 컴퓨터는 지금 풀에서 채굴 중입니다. 풀에서 나온 뒤 다시 해 보세요."),
    ("notice.making_light", "이 컴퓨터는 블록 요약만 받고 있어서 채굴할 수 없습니다."),
];

pub const EXPERT: &[(&str, &str)] = &[
    (
        "pool.join_boards_about",
        "보드는 이 앱의 Stratum 클라이언트를 거쳐 들어갑니다. 작업은 케이블로 내려가고, 무선으로만 잡히는 보드에는 케이블 보드를 거쳐 서명된 위임 작업으로 갑니다(둘 다 i19 이상). 워커마다 따로 인증합니다.",
    ),
    ("notice.chain_closed", "체인을 아직 여는 중입니다. 잠시 뒤 다시 해 보세요."),
    ("notice.making_light", "헤더만 받는 노드는 채굴할 수 없습니다. 먼저 라이트 모드를 끄세요."),
];
