//! Which language a system speaks to its person, as a short code such as `ko` or `en`.

/// The language part of a locale or language tag: `ko_KR.UTF-8`, `ko-KR` and `ko` all give `ko`.
/// `C` and `POSIX` name no language, so they give nothing.
pub fn code_of(tag: &str) -> Option<String> {
    let code = tag.split(['_', '-', '.', '@']).next().unwrap_or_default().trim();
    let named = !code.is_empty() && code.chars().all(|letter| letter.is_ascii_alphabetic());
    let none = matches!(code, "C" | "POSIX");
    (named && !none).then(|| code.to_ascii_lowercase())
}

/// The language of messages on Linux and other Unix systems, read from what `var` gives for each
/// environment name. The locale comes from `LC_ALL`, then `LC_MESSAGES`, then `LANG`. When the
/// locale names a language, the first entry of `LANGUAGE` goes before it, as gettext does, so the
/// app speaks as the rest of the desktop does.
pub fn posix(var: impl Fn(&str) -> Option<String>) -> Option<String> {
    let set = |name: &str| var(name).filter(|value| !value.is_empty());
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"].into_iter().find_map(set)?;
    let locale = code_of(&locale)?;
    let first = set("LANGUAGE").and_then(|list| list.split(':').find_map(code_of));
    Some(first.unwrap_or(locale))
}

/// The first of the person's preferred languages, as Windows and macOS keep them.
#[cfg(not(target_os = "linux"))]
pub fn preferred() -> Option<String> {
    sys_locale::get_locales().find_map(|tag| code_of(&tag))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| pairs.iter().find(|(key, _)| *key == name).map(|(_, value)| value.to_string())
    }

    #[test]
    fn tags_give_their_language() {
        assert_eq!(code_of("ko_KR.UTF-8").as_deref(), Some("ko"));
        assert_eq!(code_of("ko-KR").as_deref(), Some("ko"));
        assert_eq!(code_of("en_US@euro").as_deref(), Some("en"));
        assert_eq!(code_of("C.UTF-8"), None);
        assert_eq!(code_of("POSIX"), None);
        assert_eq!(code_of(""), None);
    }

    #[test]
    fn the_locale_is_read_in_the_order_unix_reads_it() {
        assert_eq!(posix(env(&[("LANG", "ko_KR.UTF-8")])).as_deref(), Some("ko"));
        let messages = [("LANG", "en_US.UTF-8"), ("LC_MESSAGES", "ko_KR.UTF-8")];
        assert_eq!(posix(env(&messages)).as_deref(), Some("ko"));
        let all = [("LANG", "ko_KR.UTF-8"), ("LC_MESSAGES", "ko_KR.UTF-8"), ("LC_ALL", "de_DE")];
        assert_eq!(posix(env(&all)).as_deref(), Some("de"));
        let empty = [("LANG", "ko_KR.UTF-8"), ("LC_ALL", "")];
        assert_eq!(posix(env(&empty)).as_deref(), Some("ko"));
        assert_eq!(posix(env(&[])), None);
    }

    #[test]
    fn language_goes_first_unless_the_locale_is_c() {
        let list = [("LANG", "en_US.UTF-8"), ("LANGUAGE", "ko:en")];
        assert_eq!(posix(env(&list)).as_deref(), Some("ko"));
        let plain = [("LANG", "C.UTF-8"), ("LANGUAGE", "ko")];
        assert_eq!(posix(env(&plain)), None);
    }
}
