//! English words for joining a pool elsewhere with many miners at once, for the chain words of
//! the console as their notices read on screen, and for making blocks on the Lab's own coin.
pub const WORDS: &[(&str, &str)] = &[
    ("pool.join_who", "Who joins"),
    ("pool.join_choose", "Pick at least one to join."),
    (
        "pool.join_boards_about",
        "Boards join through this app. It takes the pool's work and hands it to each board, and a board heard only by radio gets it through the board on the cable. Each one signs in with its own name, so the pool counts and pays each one.",
    ),
    ("notice.waiting_list", "{}"),
    ("notice.coins_list", "{}"),
    ("notice.chain_closed", "Still opening. Try again in a moment."),
    ("notice.making", "Making {} blocks at full speed."),
    ("notice.made_block", "Made a block: {}"),
    ("notice.made", "Made all the blocks you asked for."),
    ("notice.making_status", "{}"),
    ("notice.making_stopped", "Stopped making blocks."),
    ("notice.not_making", "No blocks are being made."),
    ("notice.making_count", "Say how many blocks, from 1 to 1,000."),
    ("notice.making_already", "Blocks are being made already."),
    ("notice.making_in_pool", "This computer mines in a pool now. Leave the pool, then try again."),
    ("notice.making_light", "This computer keeps block summaries only, so it cannot mine."),
    ("lab2.make_title", "Make blocks now"),
    ("lab2.make_about", "Mines blocks on the Lab's own coin at full speed. The next run starts from them."),
    ("lab2.make_count", "Blocks"),
    ("lab2.make", "Make blocks"),
    ("lab2.making", "Making blocks: {} of {}"),
    ("lab2.made", "Made {} blocks. The Lab's coin is at block {}."),
    ("lab2.make_failed", "The blocks were not made: {}"),
    ("lab2.make_details", "The Lab keeps its miners running after this, so the next run uses the same coin and the blocks made here. Remove them below when you are done."),
];

pub const EXPERT: &[(&str, &str)] = &[
    (
        "pool.join_boards_about",
        "Boards join through this app's Stratum client: jobs go down the cable, and to radio-only boards as signed lent work through the cable board (i19 or later on both). Each worker authorizes on its own.",
    ),
    ("notice.chain_closed", "The chain is still opening. Try again in a moment."),
    ("notice.making_light", "A headers-only node cannot mine. Turn light mode off first."),
    ("lab2.make_about", "Mines on the Lab coin with every miner capped off, until the chain is N blocks longer."),
];
