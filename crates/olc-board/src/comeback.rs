//! What a board says first after an update. Its beacon names the build it runs well before it has
//! caught up enough to answer a question, which on a long chain takes minutes.

/// Why an update ended with the board on the build it ran before: the new one did not start, or
/// it stopped within its first minute and the board went back.
pub const WENT_BACK: &str = "The board is back on its earlier build.";

/// What the build a board names says about an update it was sent.
#[derive(Debug, PartialEq, Eq)]
pub enum Back {
    /// It runs a build other than the one it ran before: the new one started.
    OnNewBuild,
    /// It names the build it ran before, or none: only the slot it runs from can tell.
    Unclear,
}

pub fn back_on(before: &str, now: &str) -> Back {
    if !now.is_empty() && now != before {
        Back::OnNewBuild
    } else {
        Back::Unclear
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_board_naming_another_build_is_back_on_the_new_one_at_once() {
        assert_eq!(back_on("i13", "i14"), Back::OnNewBuild);
        assert_eq!(back_on("", "i14"), Back::OnNewBuild);
    }

    #[test]
    fn the_same_build_or_none_leaves_it_to_the_slot() {
        assert_eq!(back_on("i13", "i13"), Back::Unclear);
        assert_eq!(back_on("i13", ""), Back::Unclear);
    }
}
