//! Passphrase quick connect (D24): five dictated words instead of ID + code.
//!
//! ```text
//!   word1 word2 | word3 word4 word5
//!   locator     | secret
//!   20 bits     | 30 bits
//! ```
//!
//! - The **locator** is allocated by the rendezvous server (`POST /v1/locator`),
//!   unique among active locators, and only says *which* host to dial.
//! - The **secret** is drawn on the host and never leaves it except through
//!   the user's mouth. It is the SPAKE2 password ([`crate::pake`]), so the
//!   server cannot pair, and a guesser gets one online attempt per lookup,
//!   bounded by the server lockout. 30 bits online-only is the same trade-off
//!   as the 39-bit one-time code, with words that survive a phone call.
//!
//! Each word is a 10-bit index into a 1024-word list. Lists exist per language
//! (`wordlists/*.txt`); indices, not words, are what count, so a host showing
//! Romanian words can be typed by a controller whose app is in English. Input
//! is folded (case, diacritics), and every word is identified by its first four
//! folded letters, also across languages (`wordlists/check.ps1` enforces it).

use std::collections::HashMap;
use std::sync::LazyLock;

use zeroize::Zeroizing;

use crate::{Error, Result, random_bytes};

/// Words per passphrase.
pub const WORDS: usize = 5;
/// Bits carried by one word.
pub const WORD_BITS: u32 = 10;
/// Largest locator (20 bits), matching the server's allocator.
pub const LOCATOR_MAX: u32 = (1 << (2 * WORD_BITS)) - 1;
const LIST_LEN: usize = 1 << WORD_BITS;
const SECRET_WORDS: usize = 3;
/// Letters that identify a word.
const PREFIX: usize = 4;

/// Language of a word list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    En,
    Ro,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::En, Lang::Ro];

    /// BCP 47 tag.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ro => "ro",
        }
    }

    /// Picks the list for a UI locale such as `ro-RO`; English otherwise.
    #[must_use]
    pub fn from_locale(locale: &str) -> Self {
        let primary = locale.split(['-', '_']).next().unwrap_or("");
        if primary.eq_ignore_ascii_case("ro") {
            Lang::Ro
        } else {
            Lang::En
        }
    }

    fn list(self) -> &'static [&'static str] {
        match self {
            Lang::En => &LISTS.en,
            Lang::Ro => &LISTS.ro,
        }
    }
}

struct Lists {
    en: Vec<&'static str>,
    ro: Vec<&'static str>,
    /// Folded word or folded 4-letter prefix -> (language, index).
    index: HashMap<String, (Lang, u16)>,
}

static LISTS: LazyLock<Lists> = LazyLock::new(|| {
    let en = parse_list(include_str!("../wordlists/en.txt"));
    let ro = parse_list(include_str!("../wordlists/ro.txt"));
    let mut index = HashMap::with_capacity(4 * LIST_LEN);
    for (lang, list) in [(Lang::En, &en), (Lang::Ro, &ro)] {
        for (i, w) in list.iter().enumerate() {
            // Lists have exactly LIST_LEN (1024) entries, so i fits in u16.
            let i = u16::try_from(i).unwrap_or(u16::MAX);
            let folded = fold(w);
            let prefix: String = folded.chars().take(PREFIX).collect();
            index.insert(prefix, (lang, i));
            index.insert(folded, (lang, i));
        }
    }
    Lists { en, ro, index }
});

fn parse_list(text: &'static str) -> Vec<&'static str> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect()
}

/// Lowercases and strips Romanian diacritics (comma-below and the legacy
/// cedilla forms), so `Pădure`, `padure` and `PĂDURE` are one word.
#[must_use]
pub fn fold(word: &str) -> String {
    word.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'ă' | 'â' => 'a',
            'î' => 'i',
            'ș' | 'ş' => 's',
            'ț' | 'ţ' => 't',
            other => other,
        })
        .collect()
}

/// Resolves one typed word: exact folded match, else its first four letters.
fn lookup(token: &str) -> Option<(Lang, u16)> {
    let folded = fold(token);
    let lists = &*LISTS;
    if let Some(hit) = lists.index.get(&folded) {
        return Some(*hit);
    }
    if folded.chars().count() > PREFIX {
        let prefix: String = folded.chars().take(PREFIX).collect();
        return lists.index.get(&prefix).copied();
    }
    None
}

/// A host's passphrase: server locator plus a local secret.
#[derive(Clone)]
pub struct Passphrase {
    locator: u32,
    secret: Zeroizing<[u16; SECRET_WORDS]>,
}

impl Passphrase {
    /// Draws a fresh secret for a locator the server allocated.
    pub fn generate(locator: u32) -> Result<Self> {
        if locator > LOCATOR_MAX {
            return Err(Error::Malformed("locator range"));
        }
        let raw = Zeroizing::new(random_bytes::<{ 2 * SECRET_WORDS }>()?);
        let mut secret = Zeroizing::new([0u16; SECRET_WORDS]);
        let (pairs, _) = raw.as_chunks::<2>();
        for (slot, pair) in secret.iter_mut().zip(pairs) {
            // 1024 divides 65536: masking the low 10 bits is unbiased.
            *slot = u16::from_le_bytes(*pair) & 0x03FF;
        }
        Ok(Self { locator, secret })
    }

    #[must_use]
    pub fn locator(&self) -> u32 {
        self.locator
    }

    /// The five words in `lang`.
    #[must_use]
    pub fn words(&self, lang: Lang) -> [&'static str; WORDS] {
        let list = lang.list();
        let idx = self.indices();
        idx.map(|i| list[usize::from(i)])
    }

    /// Words joined by spaces, for display and dictation.
    #[must_use]
    pub fn display(&self, lang: Lang) -> String {
        self.words(lang).join(" ")
    }

    /// SPAKE2 password for [`crate::pake::Pairing::start`]. Depends only on
    /// the secret indices, never on the language, and cannot collide with a
    /// one-time code (those never contain `:`).
    #[must_use]
    pub fn pake_password(&self) -> Zeroizing<String> {
        password(&self.secret)
    }

    fn indices(&self) -> [u16; WORDS] {
        let hi = u16::try_from(self.locator >> WORD_BITS).unwrap_or(0);
        let lo = u16::try_from(self.locator & 0x03FF).unwrap_or(0);
        [hi, lo, self.secret[0], self.secret[1], self.secret[2]]
    }
}

impl std::fmt::Debug for Passphrase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Passphrase")
            .field("locator", &self.locator)
            .field("secret", &"<redacted>")
            .finish()
    }
}

/// What a controller typed, resolved to a locator and a PAKE password.
pub struct Parsed {
    pub locator: u32,
    pub password: Zeroizing<String>,
    /// Language of the first word: the host's display language.
    pub lang: Lang,
}

impl std::fmt::Debug for Parsed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Parsed")
            .field("locator", &self.locator)
            .field("password", &"<redacted>")
            .field("lang", &self.lang)
            .finish()
    }
}

/// Parses dictated input: any case, with or without diacritics, words
/// separated by spaces, dashes, dots or commas, each word complete or cut to
/// its first four letters.
pub fn parse(input: &str) -> Result<Parsed> {
    let tokens: Vec<&str> = input
        .split(|c: char| c.is_whitespace() || matches!(c, '-' | '.' | ',' | '·'))
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.len() != WORDS {
        return Err(Error::Malformed("passphrase word count"));
    }
    let mut idx = Zeroizing::new([0u16; WORDS]);
    let mut lang = Lang::En;
    for (n, token) in tokens.iter().enumerate() {
        let (l, i) = lookup(token).ok_or(Error::Malformed("passphrase word"))?;
        if n == 0 {
            lang = l;
        }
        idx[n] = i;
    }
    let locator = (u32::from(idx[0]) << WORD_BITS) | u32::from(idx[1]);
    let secret = Zeroizing::new([idx[2], idx[3], idx[4]]);
    Ok(Parsed {
        locator,
        password: password(&secret),
        lang,
    })
}

/// Completion for a partly typed word, in `lang`, for input fields.
#[must_use]
pub fn suggest(partial: &str, lang: Lang, limit: usize) -> Vec<&'static str> {
    let p = fold(partial);
    if p.is_empty() {
        return Vec::new();
    }
    lang.list()
        .iter()
        .copied()
        .filter(|w| fold(w).starts_with(&p))
        .take(limit)
        .collect()
}

fn password(secret: &Zeroizing<[u16; SECRET_WORDS]>) -> Zeroizing<String> {
    Zeroizing::new(format!(
        "scrin-phrase:{}.{}.{}",
        secret[0], secret[1], secret[2]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_have_exactly_1024_words_and_unique_prefixes() {
        assert_eq!(LISTS.en.len(), LIST_LEN);
        assert_eq!(LISTS.ro.len(), LIST_LEN);
        // Every word resolves back to itself, in its own language.
        for lang in Lang::ALL {
            for (i, w) in lang.list().iter().enumerate() {
                assert_eq!(
                    lookup(w),
                    Some((lang, u16::try_from(i).expect("fits"))),
                    "{w}"
                );
            }
        }
    }

    #[test]
    fn round_trips_in_both_languages() {
        let p = Passphrase::generate(731_045).expect("rng");
        for lang in Lang::ALL {
            let parsed = parse(&p.display(lang)).expect("parse");
            assert_eq!(parsed.locator, 731_045);
            assert_eq!(parsed.password.as_str(), p.pake_password().as_str());
            assert_eq!(parsed.lang, lang);
        }
    }

    #[test]
    fn accepts_missing_diacritics_case_prefixes_and_separators() {
        let p = Passphrase::generate(LOCATOR_MAX).expect("rng");
        let words = p.words(Lang::Ro);
        let typed = words
            .iter()
            .map(|w| {
                let f = fold(w).to_uppercase();
                if f.chars().count() > PREFIX {
                    f.chars().take(PREFIX).collect()
                } else {
                    f
                }
            })
            .collect::<Vec<_>>()
            .join(" - ");
        let parsed = parse(&typed).expect("parse");
        assert_eq!(parsed.locator, LOCATOR_MAX);
        assert_eq!(parsed.password.as_str(), p.pake_password().as_str());
    }

    #[test]
    fn mixed_language_input_resolves_by_index() {
        let p = Passphrase::generate(42).expect("rng");
        let en = p.words(Lang::En);
        let ro = p.words(Lang::Ro);
        let mixed = format!("{} {} {} {} {}", ro[0], en[1], ro[2], en[3], ro[4]);
        let parsed = parse(&mixed).expect("parse");
        assert_eq!(parsed.locator, 42);
        assert_eq!(parsed.password.as_str(), p.pake_password().as_str());
        assert_eq!(parsed.lang, Lang::Ro);
    }

    #[test]
    fn rejects_wrong_count_and_unknown_words() {
        let p = Passphrase::generate(1).expect("rng");
        let words = p.words(Lang::En);
        assert!(parse(&words[..4].join(" ")).is_err());
        assert!(parse(&format!("{} zzzzqx", words.join(" "))).is_err());
        assert!(parse("zzzq zzzq zzzq zzzq zzzq").is_err());
        assert!(Passphrase::generate(LOCATOR_MAX + 1).is_err());
    }

    #[test]
    fn secret_changes_password_but_not_locator() {
        let a = Passphrase::generate(7).expect("rng");
        let b = Passphrase::generate(7).expect("rng");
        assert_eq!(a.words(Lang::En)[..2], b.words(Lang::En)[..2]);
        // 2^-30 chance of a false failure.
        assert_ne!(a.pake_password().as_str(), b.pake_password().as_str());
    }

    #[test]
    fn password_never_looks_like_a_one_time_code() {
        let p = Passphrase::generate(3).expect("rng");
        assert!(crate::code::normalize(&p.pake_password()).is_err());
    }

    #[test]
    fn debug_redacts_the_secret() {
        let p = Passphrase::generate(9).expect("rng");
        // Exact text: a substring check flakes when a secret word ("red")
        // happens to occur inside "<redacted>".
        assert_eq!(
            format!("{p:?}"),
            r#"Passphrase { locator: 9, secret: "<redacted>" }"#
        );
    }

    #[test]
    fn suggests_completions_without_diacritics() {
        let first = Lang::Ro.list()[0];
        let start: String = fold(first).chars().take(2).collect();
        let s = suggest(&start, Lang::Ro, 5);
        assert!(!s.is_empty() && s.len() <= 5);
        assert!(s.iter().all(|w| fold(w).starts_with(&start)));
        assert_eq!(Lang::from_locale("ro-RO"), Lang::Ro);
        assert_eq!(Lang::from_locale("de"), Lang::En);
    }
}
