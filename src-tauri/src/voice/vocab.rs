//! What the whisper.cpp engines are told before they listen, and the tidying of what they hear.
//! Pure and tested; nothing here is logged (the hints are the person's own words and contacts).
//!
//! **The prompt.** whisper.cpp takes "previous text" that biases what it expects to hear next.
//! tug gives it the spoken commands `text.rs` understands ("camel case", "open paren", "new line",
//! "dot", "equals"…) and everyday coding words ("check logs", "git", "npm"…), all lower case with
//! no punctuation: Whisper continues in the style of the prompt, so it writes "comma" as the word
//! and leaves out capitals and full stops it would otherwise add, and `text.rs` turns the words
//! into symbols exactly as it does for Windows' recognizer.
//!
//! **Personal hints** ([`Hints`]): the words in the person's own dictionary (Settings) and the
//! names of the people they text most (tug's contacts), spelled as they're written, so Whisper
//! spells them that way too. Both stay on the PC: they only ever reach whisper.cpp.
//!
//! **Where the words are going** ([`focus_for_process`]): in a code editor or terminal the prompt
//! is the coding vocabulary first, then the hints; in chat and mail (and browsers) it leads with
//! names and everyday phrases. [`prompt_for`] fills the token budget in that order, skipping any
//! hint that doesn't fit rather than cutting one in half.
//!
//! **The tidy-up.** Whisper marks sounds that aren't speech ("[BLANK_AUDIO]", "(silence)") and
//! sometimes ends a phrase with a full stop nobody said; both go before `text::format` runs. On a
//! cough it can play its prompt back; [`is_prompt_echo`] catches that, names included.

use crate::voice::text::{exe_name, style_for_process, Style};

/// The spoken commands in the prompt: each is one `text.rs` turns into a symbol or a casing
/// (a test checks). The ones people use most come first.
pub const COMMANDS: &[&str] = &[
    "camel case",
    "snake case",
    "pascal case",
    "kebab case",
    "constant case",
    "open paren",
    "close paren",
    "open bracket",
    "close bracket",
    "open brace",
    "close brace",
    "new line",
    "dot",
    "comma",
    "semicolon",
    "colon",
    "equals",
    "double equals",
    "plus",
    "minus",
    "arrow",
    "fat arrow",
    "quote",
    "single quote",
    "backtick",
    "underscore",
    "dash",
    "slash",
    "backslash",
    "question mark",
    "exclamation mark",
    "period",
    "greater than",
    "less than",
    "ampersand",
    "pipe",
    "hash",
    "at sign",
    "dollar sign",
    "literal",
    "all caps",
    "capitalize",
];

/// Words people say to coding tools and terminals, which a general speech model mishears ("check
/// logs" came out as "At Los…"). Lower case, as they'd be said.
pub const CODE_WORDS: &[&str] = &[
    "check logs",
    "run the tests",
    "git commit",
    "git push",
    "pull request",
    "merge",
    "branch",
    "rebase",
    "npm install",
    "npm run build",
    "cargo",
    "build",
    "deploy",
    "debug",
    "refactor",
    "function",
    "const",
    "let",
    "var",
    "async",
    "await",
    "return",
    "import",
    "export",
    "class",
    "interface",
    "struct",
    "enum",
    "def",
    "self",
    "this",
    "null",
    "undefined",
    "true",
    "false",
    "user id",
    "json",
    "yaml",
    "sql",
    "api",
    "url",
    "localhost",
    "readme",
    "todo",
    "config",
    "typescript",
    "python",
    "rust",
    "vue",
    "regex",
    "stack trace",
];

/// A few short lines showing how commands sit inside speech, so Whisper keeps them as words.
const EXAMPLES: &[&str] = &[
    "const camel case user id equals five semicolon",
    "print open paren quote hello quote close paren",
    "hello comma how are you question mark",
];

/// The same for chat and mail: punctuation said as words inside everyday sentences.
const CHAT_EXAMPLES: &[&str] = &[
    "hello comma how are you question mark",
    "thanks comma see you tomorrow period",
];

/// The commands people use in messages: punctuation, not casing.
const CHAT_COMMANDS: &[&str] = &[
    "comma",
    "period",
    "question mark",
    "exclamation mark",
    "new line",
    "colon",
    "quote",
    "dash",
];

/// Everyday phrases a chat or an email is made of, which a coding prompt would push aside.
pub const EVERYDAY: &[&str] = &[
    "thanks",
    "thank you",
    "sounds good",
    "see you soon",
    "on my way",
    "running late",
    "no worries",
    "let me know",
    "call me",
    "talk later",
    "tomorrow",
    "tonight",
    "this morning",
    "this afternoon",
    "next week",
    "meeting",
    "lunch",
    "dinner",
    "happy birthday",
    "congratulations",
    "love you",
    "okay",
];

/// Chat, mail and messaging programs, and browsers (where most webmail and web chat is), by
/// executable name (lowercase). tug itself is one: its message box.
pub const CHAT_APPS: &[&str] = &[
    "ms-teams.exe",
    "teams.exe",
    "slack.exe",
    "discord.exe",
    "discordptb.exe",
    "discordcanary.exe",
    "outlook.exe",
    "olk.exe",
    "hxoutlook.exe",
    "thunderbird.exe",
    "mailspring.exe",
    "whatsapp.exe",
    "whatsapp.root.exe",
    "telegram.exe",
    "signal.exe",
    "messenger.exe",
    "skype.exe",
    "zoom.exe",
    "element.exe",
    "phoneexperiencehost.exe",
    "tug.exe",
    "chrome.exe",
    "msedge.exe",
    "firefox.exe",
    "brave.exe",
    "opera.exe",
    "vivaldi.exe",
    "arc.exe",
];

/// What the prompt leans on, from the program in front.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Focus {
    /// A code editor, IDE or terminal: the coding vocabulary comes first.
    Code,
    /// Chat, mail or a browser: names and everyday phrases come first.
    Chat,
    /// Anything else (Notepad, Word, an unknown program): the commands, then the hints.
    General,
}

/// The focus for a foreground program, from its path or file name. Code is whatever `text.rs`
/// treats as an editor or a terminal, so the two always agree.
pub fn focus_for_process(exe: &str) -> Focus {
    match style_for_process(exe) {
        Style::Editor | Style::Terminal => Focus::Code,
        Style::Prose if CHAT_APPS.contains(&exe_name(exe).as_str()) => Focus::Chat,
        Style::Prose => Focus::General,
    }
}

/// The longest the built-in vocabulary may be, in characters (the hints have the rest of the
/// budget). It's what v0.5.19 measured with: the examples, the commands and most of the coding
/// words, about 135 tokens.
pub const PROMPT_MAX_CHARS: usize = 720;

/// The most prompt tokens whisper.cpp uses: one less than half its 448-token text context.
pub const PROMPT_TOKENS: usize = 223;

/// The most names from the contacts in a prompt (chat and mail; fewer elsewhere).
pub const MAX_NAMES: usize = 30;
/// The most entries in the personal dictionary.
pub const MAX_WORDS: usize = 100;
/// The longest entry (a name or a dictionary word), in characters.
pub const MAX_ENTRY_CHARS: usize = 40;

/// The person's own words for the prompt, already cleaned ([`clean_entry`]): their dictionary
/// first, then their most frequent conversation partners' names, most frequent first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hints {
    pub words: Vec<String>,
    pub names: Vec<String>,
}

impl Hints {
    /// From the saved dictionary and the contact names, cleaned, without duplicates (a name in the
    /// dictionary isn't repeated) and bounded.
    pub fn new(words: &[String], names: &[String]) -> Hints {
        let words = normalize_dictionary(words);
        let mut seen: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
        let mut kept = Vec::new();
        for n in names.iter().filter_map(|n| clean_entry(n)) {
            if kept.len() == MAX_NAMES {
                break;
            }
            let key = n.to_lowercase();
            if !seen.contains(&key) {
                seen.push(key);
                kept.push(n);
            }
        }
        Hints { words, names: kept }
    }

    fn all(&self) -> impl Iterator<Item = &str> {
        self.words.iter().chain(&self.names).map(String::as_str)
    }
}

/// One dictionary entry or contact name as it can go in a prompt: single spaces, letters and
/// digits (any script) and the few marks names and product words use (' - . + # &), at most
/// [`MAX_ENTRY_CHARS`]; `None` when nothing usable is left, it has no letter, or it's too long.
/// Emoji and other symbols are dropped ("Mom ❤️" is "Mom").
pub fn clean_entry(raw: &str) -> Option<String> {
    let kept: String = raw
        .chars()
        .map(|c| if c == '\u{2019}' { '\'' } else { c })
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || matches!(c, '\'' | '-' | '.' | '+' | '#' | '&'))
        .collect();
    let entry = kept.split_whitespace().collect::<Vec<_>>().join(" ");
    let entry = entry
        .trim_matches(|c: char| matches!(c, '\'' | '-' | '.' | '&'))
        .to_string();
    (entry.chars().any(char::is_alphabetic) && entry.chars().count() <= MAX_ENTRY_CHARS).then_some(entry)
}

/// The personal dictionary as it's saved: each entry cleaned, duplicates (ignoring case) dropped,
/// the order kept, at most [`MAX_WORDS`].
pub fn normalize_dictionary(words: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in words.iter().filter_map(|w| clean_entry(w)) {
        if out.len() == MAX_WORDS {
            break;
        }
        if !out.iter().any(|o| o.to_lowercase() == w.to_lowercase()) {
            out.push(w);
        }
    }
    out
}

#[cfg(test)]
/// A rough token count for when whisper.cpp's own tokenizer isn't to hand (tests): a token per
/// four characters of each word, rounded up, and one more for a word with capitals or letters
/// outside ASCII (they split into more pieces). It over-counts common English words, so a prompt
/// it passes fits.
pub fn estimate_tokens(text: &str) -> usize {
    text.split_whitespace()
        .map(|w| w.chars().count().div_ceil(4) + usize::from(w.chars().any(|c| c.is_uppercase() || !c.is_ascii())))
        .sum()
}

#[cfg(test)]
/// Whether `prompt` fits the budget by [`estimate_tokens`].
pub fn fits_estimate(prompt: &str) -> bool {
    estimate_tokens(prompt) <= PROMPT_TOKENS
}

/// Builds a prompt from phrases in priority order.
struct Builder<'f> {
    out: String,
    /// Characters of built-in vocabulary so far (separators included).
    static_chars: usize,
    /// The built-in vocabulary has stopped: it reached [`PROMPT_MAX_CHARS`] or the budget.
    static_full: bool,
    seen: Vec<String>,
    fits: &'f dyn Fn(&str) -> bool,
}

impl<'f> Builder<'f> {
    fn new(fits: &'f dyn Fn(&str) -> bool) -> Self {
        Builder {
            out: String::new(),
            static_chars: 0,
            static_full: false,
            seen: Vec::new(),
            fits,
        }
    }

    fn with(&self, phrase: &str) -> String {
        if self.out.is_empty() {
            phrase.to_string()
        } else {
            format!("{} {phrase}", self.out)
        }
    }

    /// Built-in phrases, in order, until one doesn't fit; then no more of them (never part way
    /// through the list, so the vocabulary is always its most useful start).
    fn statics<'a>(&mut self, phrases: impl IntoIterator<Item = &'a str>) {
        for phrase in phrases {
            if self.static_full {
                return;
            }
            let key = phrase.to_lowercase();
            if self.seen.contains(&key) {
                continue;
            }
            let extra = phrase.len() + usize::from(self.static_chars > 0);
            let candidate = self.with(phrase);
            if self.static_chars + extra > PROMPT_MAX_CHARS || !(self.fits)(&candidate) {
                self.static_full = true;
                return;
            }
            self.out = candidate;
            self.static_chars += extra;
            self.seen.push(key);
        }
    }

    /// Hints, in order, each only if it fits whole; one that doesn't is skipped and the next
    /// (shorter) one may still fit.
    fn hints<'a>(&mut self, entries: impl IntoIterator<Item = &'a str>) {
        for entry in entries {
            let key = entry.to_lowercase();
            if self.seen.contains(&key) {
                continue;
            }
            let candidate = self.with(entry);
            if (self.fits)(&candidate) {
                self.out = candidate;
                self.seen.push(key);
            }
        }
    }
}

/// The prompt for one hold: what to lean on ([`Focus`]), the person's [`Hints`], whether the
/// engine listens in English (Accurate always; Most accurate unless another language is picked)
/// and whether a prompt fits whisper.cpp's budget (`fits`: its tokenizer, or [`fits_estimate`]).
///
/// - **Code**: the examples, the commands and the coding words (what v0.5.19 used), then the
///   dictionary, then up to 10 names.
/// - **General**: the same, with up to 20 names.
/// - **Chat**: up to [`MAX_NAMES`] names, the dictionary, then everyday lines and phrases with the
///   punctuation commands, then the other commands.
/// - **Not English**: the dictionary and the names only: the commands are English words.
///
/// With no hints, Code and General are exactly the v0.5.19 prompt.
pub fn prompt_for(focus: Focus, hints: &Hints, english: bool, fits: &dyn Fn(&str) -> bool) -> String {
    let names_cap = match focus {
        Focus::Code => 10,
        Focus::General => 20,
        Focus::Chat => MAX_NAMES,
    };
    let words = || hints.words.iter().map(String::as_str);
    let names = || hints.names.iter().take(names_cap).map(String::as_str);
    let mut b = Builder::new(fits);
    if !english {
        b.hints(words());
        b.hints(names());
        return b.out;
    }
    match focus {
        Focus::Code | Focus::General => {
            b.statics(EXAMPLES.iter().chain(COMMANDS).chain(CODE_WORDS).copied());
            b.hints(words());
            b.hints(names());
        }
        Focus::Chat => {
            b.hints(names());
            b.hints(words());
            b.statics(
                CHAT_EXAMPLES
                    .iter()
                    .chain(CHAT_COMMANDS)
                    .chain(EVERYDAY)
                    .chain(COMMANDS)
                    .copied(),
            );
        }
    }
    b.out
}

#[cfg(test)]
/// The prompt with no hints, for an unknown program: the v0.5.19 prompt.
pub fn prompt() -> String {
    prompt_for(Focus::General, &Hints::default(), true, &fits_estimate)
}

/// Lower-case words with punctuation dropped, for comparing text with the prompt.
fn plain_words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// The most words someone can say in `secs` of speech, generously: about four a second (spoken
/// commands are short words), plus one.
fn most_words(secs: f32) -> usize {
    (secs.max(0.0) * 4.0) as usize + 1
}

/// Below this much trimmed speech (which includes 0.2 s kept either side of the sound,
/// `audio::trim_silence`), a hold is a cough or a click rather than a name said: under 0.2 s of
/// sound.
pub const LONE_HINT_SECS: f32 = 0.6;

/// Whether what Whisper heard is really its prompt played back. On a cough, a click or a breath
/// it can't make out, Whisper sometimes "continues" its prompt instead of transcribing:
///
/// - an example line, a run of four prompt words, or a run of names or dictionary words exactly
///   as the prompt lists them, when the hold was too short to have said that many words
///   (`speech_secs`, the trimmed recording);
/// - a single name or dictionary word on its own, from under [`LONE_HINT_SECS`] of speech.
///
/// Someone who says the same words still gets them typed: the issue's own test phrases are
/// example lines, "open paren close paren open bracket" is a run of the prompt, and "Priya" said
/// is a name in it.
pub fn is_prompt_echo(heard: &str, speech_secs: f32, prompt: &str, hints: &Hints) -> bool {
    let words = plain_words(heard);
    if words.is_empty() {
        return false;
    }
    let prompt_words = plain_words(prompt);
    let in_prompt = |entry: &[String]| prompt_words.windows(entry.len()).any(|w| w == entry);
    if speech_secs < LONE_HINT_SECS
        && hints.all().any(|h| {
            let h = plain_words(h);
            !h.is_empty() && h == words && in_prompt(&h)
        })
    {
        return true;
    }
    if words.len() <= most_words(speech_secs) {
        return false;
    }
    if EXAMPLES.iter().chain(CHAT_EXAMPLES).any(|e| plain_words(e) == words) {
        return true;
    }
    // Four or more words in a row exactly as they are in the prompt.
    if words.windows(4).any(|run| prompt_words.windows(4).any(|p| p == run)) {
        return true;
    }
    // Names or dictionary words, two or more, lifted from the prompt in its order.
    let hint_words: Vec<String> = hints.all().flat_map(plain_words).collect();
    words.len() >= 2 && words.iter().all(|w| hint_words.contains(w)) && in_prompt(&words)
}

/// First names that are also everyday words: on their own they keep the case they were heard in
/// ("I will call", not "I Will call"), though a whole name ("Will Turner") is still respelled.
const EVERYDAY_NAMES: &[&str] = &[
    "will", "may", "june", "april", "august", "bill", "art", "mark", "rose", "grace", "hope", "joy", "faith", "pat",
    "sue", "jack", "frank", "rich", "dean", "gene", "bob", "sky", "summer", "autumn", "dawn", "eve", "iris", "ivy",
    "lily", "max", "ray", "rob", "sandy", "sunny", "chase", "drew", "early", "an", "page", "major", "king", "young",
    "long", "price", "rice", "bell", "hall", "wood", "west", "north", "love", "star", "mom", "mum", "dad", "nan",
    "honey", "babe", "baby", "home", "work", "office", "me", "us",
];

/// What was heard, with the person's own spellings of their names and words put back where only
/// the case differs: "katelyn" becomes "Katelyn", "kubernetes" "Kubernetes", "priya raman" "Priya
/// Raman". Never changes a letter, only capitals. Whole entries first (a dictionary entry, a full
/// name); then single words of names, except names that are everyday words on their own
/// ([`EVERYDAY_NAMES`]). Whisper copies the prompt's lower case, and Windows' recognizer gets no
/// hints at all, so both are respelled.
pub fn respell(heard: &str, hints: &Hints) -> String {
    // Each word of the text, with what's around it kept as it was.
    let mut tokens: Vec<String> = Vec::new();
    let mut seps: Vec<String> = vec![String::new()];
    let mut word = String::new();
    for c in heard.chars() {
        if c.is_alphanumeric() || c == '\'' {
            word.push(c);
        } else {
            if !word.is_empty() {
                tokens.push(std::mem::take(&mut word));
                seps.push(String::new());
            }
            if let Some(s) = seps.last_mut() {
                s.push(c);
            }
        }
    }
    if !word.is_empty() {
        tokens.push(word);
        seps.push(String::new());
    }
    let split = |e: &'_ str| -> Vec<String> {
        e.split(|c: char| !(c.is_alphanumeric() || c == '\''))
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect()
    };
    // Dictionary entries always; a one-word name only if it isn't an everyday word.
    let entries: Vec<Vec<String>> = hints
        .words
        .iter()
        .map(|w| split(w))
        .chain(hints.names.iter().map(|n| split(n)).filter(|parts| {
            parts.len() > 1
                || parts
                    .first()
                    .is_some_and(|p| !EVERYDAY_NAMES.contains(&p.to_lowercase().as_str()))
        }))
        .filter(|e| !e.is_empty())
        .collect();
    let same = |a: &str, b: &str| a.to_lowercase() == b.to_lowercase();
    let mut fixed = vec![false; tokens.len()];
    // Whole entries, longest first.
    let mut whole: Vec<&Vec<String>> = entries.iter().collect();
    whole.sort_by_key(|e| std::cmp::Reverse(e.len()));
    for entry in whole {
        let n = entry.len();
        let mut i = 0;
        while i + n <= tokens.len() {
            if (0..n).all(|k| !fixed[i + k] && same(&tokens[i + k], &entry[k])) {
                for k in 0..n {
                    tokens[i + k] = entry[k].clone();
                    fixed[i + k] = true;
                }
                i += n;
            } else {
                i += 1;
            }
        }
    }
    // Single words of names (not dictionary entries: those were matched whole above).
    for name in hints.names.iter() {
        for part in name.split_whitespace() {
            let lower = part.to_lowercase();
            if part.chars().count() < 2 || EVERYDAY_NAMES.contains(&lower.as_str()) {
                continue;
            }
            for (t, done) in tokens.iter_mut().zip(fixed.iter_mut()) {
                if !*done && t.to_lowercase() == lower {
                    *t = part.to_string();
                    *done = true;
                }
            }
        }
    }
    let mut out = String::with_capacity(heard.len());
    for (i, sep) in seps.iter().enumerate() {
        out.push_str(sep);
        if let Some(t) = tokens.get(i) {
            out.push_str(t);
        }
    }
    out
}

/// Bracketed words Whisper writes for sounds that aren't speech, when in round brackets.
/// (Square brackets are always a note, never dictation; round ones can be "open paren".)
const NOT_SPEECH: &[&str] = &[
    "silence",
    "blank audio",
    "blank_audio",
    "music",
    "music playing",
    "noise",
    "inaudible",
    "applause",
    "laughs",
    "laughter",
    "coughs",
    "coughing",
    "sighs",
    "typing",
    "keyboard clicking",
    "clicking",
    "beep",
    "wind",
    "breathing",
    "no speech",
];

fn is_note(inner: &str, square: bool) -> bool {
    let words = inner.trim().to_lowercase();
    square || NOT_SPEECH.contains(&words.as_str())
}

/// What Whisper heard, ready for `text::format`: notes for non-speech sounds removed, spaces
/// collapsed, and a lone full stop at the very end dropped (Whisper adds one to most phrases;
/// a spoken "period" or "dot" stays a word and `text.rs` makes it a full stop where it belongs).
pub fn clean(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find(['[', '(', '*']) {
        let open = rest[start..].chars().next().unwrap_or('[');
        let close = match open {
            '[' => ']',
            '(' => ')',
            _ => '*',
        };
        let after = &rest[start + 1..];
        match after.find(close) {
            Some(end)
                if (open != '*' || after[..end].chars().all(|c| c.is_alphabetic() || c == ' '))
                    && (open == '*' || is_note(&after[..end], open == '[')) =>
            {
                out.push_str(&rest[..start]);
                out.push(' ');
                rest = &after[end + 1..];
            }
            _ => {
                out.push_str(&rest[..=start]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    let mut text = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.ends_with('.') && !text.ends_with("..") {
        text.pop();
        let trimmed = text.trim_end().len();
        text.truncate(trimmed);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::text::{format, target_for_process};

    #[test]
    fn every_prompt_command_is_one_text_rs_understands() {
        let code = target_for_process("code.exe");
        for phrase in COMMANDS {
            // Casing commands shape the word after them; the rest become symbols.
            let said = format!("{phrase} widget");
            let typed = format(&said, code);
            assert_ne!(typed, said, "{phrase:?} isn't a command text.rs knows");
            assert!(
                !typed.contains(phrase),
                "{phrase:?} should be turned into something, got {typed:?}"
            );
        }
    }

    #[test]
    fn the_prompt_is_lower_case_words_within_budget() {
        let p = prompt();
        assert!(p.len() <= PROMPT_MAX_CHARS, "{} chars", p.len());
        assert!(p.len() > PROMPT_MAX_CHARS / 2, "uses most of the budget");
        assert_eq!(p, p.to_lowercase(), "no capitals for Whisper to copy");
        assert!(
            p.chars().all(|c| c.is_ascii_lowercase() || c == ' '),
            "no punctuation or digits for Whisper to copy"
        );
        assert!(!p.contains("  ") && !p.starts_with(' ') && !p.ends_with(' '));
        // The phrases this is for.
        for must in [
            "camel case",
            "open paren",
            "new line",
            "dot",
            "equals",
            "check logs",
            "semicolon",
        ] {
            assert!(p.contains(must), "{must:?} missing");
        }
        assert!(p.starts_with(EXAMPLES[0]), "the examples lead");
    }

    #[test]
    fn the_prompt_stops_at_a_whole_phrase() {
        let p = prompt();
        let all: Vec<&str> = EXAMPLES.iter().chain(COMMANDS).chain(CODE_WORDS).copied().collect();
        // Whatever it ends with is a whole phrase from the lists.
        assert!(all.iter().any(|w| p.ends_with(w)), "ends mid-phrase: {p:?}");
        assert_eq!(prompt(), p, "the same every time");
    }

    /// `is_prompt_echo` with the plain prompt and no hints.
    fn echo(heard: &str, secs: f32) -> bool {
        is_prompt_echo(heard, secs, &prompt(), &Hints::default())
    }

    #[test]
    fn the_prompt_played_back_on_a_cough_is_dropped() {
        // An example line from a 0.4 s click: far more words than that could hold.
        assert!(echo("Const camel case user ID equals five semicolon.", 0.4));
        assert!(echo("hello, comma how are you question mark", 0.6));
        // A run of the command list.
        assert!(echo("open bracket close bracket open brace close brace new line", 0.5));
        assert!(echo("camel case snake case pascal case", 0.3));
    }

    #[test]
    fn saying_the_same_words_still_types_them() {
        // The issue's test phrases are EXAMPLES lines: said over a couple of seconds, they stay.
        assert!(!echo("const camel case user id equals five semicolon", 2.6));
        assert!(!echo("hello comma how are you question mark", 2.0));
        // A run of commands, said.
        assert!(!echo("open paren close paren open bracket", 1.5));
        // Short and real, even from a short hold.
        assert!(!echo("check logs", 0.4));
        assert!(!echo("git push", 0.2));
        // Not from the prompt at all, however fast.
        assert!(!echo("the quick brown fox jumps over the lazy dog", 0.5));
        assert!(!echo("", 0.0));
    }

    /// Made-up people and words (CONTRIBUTING: no real personal data).
    fn hints() -> Hints {
        Hints::new(
            &["Kubernetes".into(), "Terraform".into(), "Zephyrine".into()],
            &[
                "Priya Raman".into(),
                "Sam Okafor".into(),
                "Jordan Blake".into(),
                "Mom ❤️".into(),
                "+1 555 0123".into(),
            ],
        )
    }

    #[test]
    fn hints_are_cleaned_bounded_and_never_repeated() {
        let h = hints();
        assert_eq!(h.words, ["Kubernetes", "Terraform", "Zephyrine"]);
        assert_eq!(
            h.names,
            ["Priya Raman", "Sam Okafor", "Jordan Blake", "Mom"],
            "no emoji, no numbers"
        );
        // A name already in the dictionary isn't repeated.
        let h = Hints::new(&["sam okafor".into()], &["Sam Okafor".into(), "Ana".into()]);
        assert_eq!(h.names, ["Ana"]);
        // Bounded.
        let many: Vec<String> = (0..80).map(|i| format!("Person{i}")).collect();
        assert_eq!(Hints::new(&[], &many).names.len(), MAX_NAMES);
        assert_eq!(Hints::new(&[], &[]), Hints::default());
    }

    #[test]
    fn dictionary_entries_are_tidied_and_deduplicated() {
        let raw: Vec<String> = [
            "  Node.js ",
            "node.JS",
            "C#",
            "O’Brien",
            "",
            "!!!",
            "1234",
            "two  words",
            &"x".repeat(41),
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(normalize_dictionary(&raw), ["Node.js", "C#", "O'Brien", "two words"]);
        let many: Vec<String> = (0..150).map(|i| format!("word{i}")).collect();
        assert_eq!(normalize_dictionary(&many).len(), MAX_WORDS);
        assert_eq!(clean_entry("-Ana-"), Some("Ana".into()));
        assert_eq!(clean_entry("José Núñez"), Some("José Núñez".into()), "any script");
        assert_eq!(clean_entry("📞"), None);
    }

    #[test]
    fn coding_apps_lead_with_the_coding_vocabulary() {
        let p = prompt_for(Focus::Code, &hints(), true, &fits_estimate);
        assert!(p.starts_with(&prompt()), "the v0.5.19 prompt, whole, first: {p}");
        assert!(p.contains("Kubernetes") && p.contains("Zephyrine"), "the dictionary");
        assert!(p.contains("Priya Raman"), "and names after it");
        assert!(fits_estimate(&p));
        // At most 10 names in a code editor.
        let many: Vec<String> = (0..30).map(|i| format!("Name{i}")).collect();
        let p = prompt_for(Focus::Code, &Hints::new(&[], &many), true, &|_| true);
        assert!(p.contains("Name9") && !p.contains("Name10"), "{p}");
    }

    #[test]
    fn chat_apps_lead_with_names_and_everyday_words() {
        let p = prompt_for(Focus::Chat, &hints(), true, &fits_estimate);
        assert!(
            p.starts_with("Priya Raman Sam Okafor Jordan Blake Mom Kubernetes"),
            "{p}"
        );
        assert!(p.contains("see you soon") && p.contains("question mark"));
        assert!(
            !p.contains("npm install"),
            "coding words aren't part of a chat prompt: {p}"
        );
        assert!(fits_estimate(&p));
        let many: Vec<String> = (0..40).map(|i| format!("Name{i}")).collect();
        let p = prompt_for(Focus::Chat, &Hints::new(&[], &many), true, &|_| true);
        assert!(p.contains("Name29") && !p.contains("Name30"));
    }

    #[test]
    fn with_no_hints_the_prompt_is_unchanged() {
        // What v0.5.19 measured: examples, commands, then coding words, within 720 characters.
        assert_eq!(
            prompt_for(Focus::Code, &Hints::default(), true, &fits_estimate),
            prompt()
        );
        assert_eq!(prompt_for(Focus::General, &Hints::default(), true, &|_| true), prompt());
    }

    #[test]
    fn another_language_gets_only_the_persons_words() {
        let p = prompt_for(Focus::Chat, &hints(), false, &fits_estimate);
        assert_eq!(
            p,
            "Kubernetes Terraform Zephyrine Priya Raman Sam Okafor Jordan Blake Mom"
        );
        assert_eq!(prompt_for(Focus::Code, &Hints::default(), false, &fits_estimate), "");
    }

    #[test]
    fn the_budget_is_never_exceeded_and_hints_are_never_cut_in_half() {
        // A tight budget (whisper.cpp's tokenizer stands in as a word count here).
        let tight = |s: &str| s.split_whitespace().count() <= 12;
        let many: Vec<String> = (0..40).map(|i| format!("Long Name{i}")).collect();
        let p = prompt_for(Focus::Chat, &Hints::new(&["Zephyrine".into()], &many), true, &tight);
        assert!(tight(&p), "{p}");
        assert_eq!(p.split_whitespace().count(), 12, "filled: {p}");
        assert!(p.starts_with("Long Name0 Long Name1"));
        // Whole entries only: every "Long" has its name after it.
        let w: Vec<&str> = p.split_whitespace().collect();
        for (i, word) in w.iter().enumerate() {
            if *word == "Long" {
                assert!(w.get(i + 1).is_some_and(|n| n.starts_with("Name")), "{p}");
            }
        }
        // One that doesn't fit is skipped; a shorter one after it can still go in.
        let budget = |s: &str| s.len() <= 30;
        let h = Hints::new(&["Supercalifragilistic".into(), "Ana".into(), "Bo".into()], &[]);
        assert_eq!(
            prompt_for(Focus::Chat, &h, false, &budget),
            "Supercalifragilistic Ana Bo"
        );
        let h = Hints::new(
            &["Ana".into(), "Supercalifragilisticexpialidocious".into(), "Bo".into()],
            &[],
        );
        assert_eq!(prompt_for(Focus::Chat, &h, false, &budget), "Ana Bo");
        // The real budget: a full dictionary and names never pass it.
        let words: Vec<String> = (0..MAX_WORDS).map(|i| format!("Dictionaryword{i}")).collect();
        let names: Vec<String> = (0..MAX_NAMES).map(|i| format!("Firstname Lastname{i}")).collect();
        for focus in [Focus::Code, Focus::Chat, Focus::General] {
            let p = prompt_for(focus, &Hints::new(&words, &names), true, &fits_estimate);
            assert!(
                estimate_tokens(&p) <= PROMPT_TOKENS,
                "{focus:?}: {}",
                estimate_tokens(&p)
            );
        }
    }

    #[test]
    fn the_focus_follows_the_program_in_front() {
        for exe in [
            "Code.exe",
            "cursor.exe",
            "idea64.exe",
            "WindowsTerminal.exe",
            "pwsh.exe",
        ] {
            assert_eq!(focus_for_process(exe), Focus::Code, "{exe}");
        }
        for exe in [
            r"C:\Program Files\Slack\slack.exe",
            "ms-teams.exe",
            "Discord.exe",
            "OUTLOOK.EXE",
            "olk.exe",
            "msedge.exe",
            "chrome.exe",
            "firefox.exe",
            "WhatsApp.exe",
            "tug.exe",
        ] {
            assert_eq!(focus_for_process(exe), Focus::Chat, "{exe}");
        }
        for exe in ["notepad.exe", "WINWORD.EXE", "something.exe", ""] {
            assert_eq!(focus_for_process(exe), Focus::General, "{exe}");
        }
        // Every chat app is prose to text.rs (none is an editor or a terminal).
        for exe in CHAT_APPS {
            assert_eq!(style_for_process(exe), Style::Prose, "{exe}");
        }
    }

    #[test]
    fn a_name_or_dictionary_word_played_back_on_a_cough_is_dropped() {
        let h = hints();
        let chat = prompt_for(Focus::Chat, &h, true, &fits_estimate);
        // One name, from a click (under 0.2 s of sound).
        assert!(is_prompt_echo("Priya Raman.", 0.5, &chat, &h));
        assert!(is_prompt_echo("Sam Okafor", 0.45, &chat, &h));
        assert!(is_prompt_echo("Kubernetes", 0.5, &chat, &h));
        // Names in the prompt's order, more than could be said in the time.
        assert!(is_prompt_echo("Priya Raman, Sam Okafor, Jordan Blake.", 0.9, &chat, &h));
        assert!(is_prompt_echo("Sam Okafor Jordan", 0.4, &chat, &h));
        let code = prompt_for(Focus::Code, &h, true, &fits_estimate);
        assert!(is_prompt_echo("Kubernetes Terraform Zephyrine", 0.4, &code, &h));
    }

    #[test]
    fn heard_names_and_words_get_the_persons_capitals_back() {
        let h = Hints::new(
            &["Kubernetes".into(), "Node.js".into(), "iPhone".into()],
            &[
                "Katelyn Moore".into(),
                "Will Turner".into(),
                "Mom".into(),
                "Sean O'Brien".into(),
            ],
        );
        assert_eq!(
            respell("tell katelyn i'm running late", &h),
            "tell Katelyn i'm running late"
        );
        assert_eq!(respell("deploy to kubernetes.", &h), "deploy to Kubernetes.");
        assert_eq!(
            respell("ask katelyn moore, then sean o'brien", &h),
            "ask Katelyn Moore, then Sean O'Brien"
        );
        assert_eq!(
            respell("update node.js on my iphone", &h),
            "update Node.js on my iPhone"
        );
        // A name that's an everyday word stays as heard on its own, but not as a whole name.
        assert_eq!(respell("i will call will turner", &h), "i will call Will Turner");
        assert_eq!(respell("tell mom", &h), "tell mom");
        // Only capitals change: a different spelling is left alone.
        assert_eq!(respell("tell caitlin", &h), "tell caitlin");
        // Words that merely contain a name, punctuation and spacing: untouched.
        assert_eq!(respell("  moorehouse,  katelyns!", &h), "  moorehouse,  katelyns!");
        assert_eq!(respell("", &h), "");
        assert_eq!(respell("check logs", &Hints::default()), "check logs");
    }

    #[test]
    fn a_name_said_is_still_typed() {
        let h = hints();
        let chat = prompt_for(Focus::Chat, &h, true, &fits_estimate);
        assert!(!is_prompt_echo("Priya", 0.9, &chat, &h), "a name said");
        assert!(!is_prompt_echo("Priya Raman", 1.0, &chat, &h));
        assert!(!is_prompt_echo("tell Sam Okafor I'm running late", 1.8, &chat, &h));
        assert!(
            !is_prompt_echo("Sam Okafor Jordan Blake", 1.4, &chat, &h),
            "said, not too fast"
        );
        // Out of the prompt's order: someone talking, not the prompt.
        assert!(!is_prompt_echo("Jordan Sam Priya", 0.5, &chat, &h));
        // A name that isn't in this prompt can't be played back from it.
        let none = prompt_for(Focus::Code, &Hints::default(), true, &fits_estimate);
        assert!(!is_prompt_echo("Priya Raman", 0.5, &none, &h));
        // Part of a name, or a word that isn't a hint, from a short hold: kept.
        assert!(!is_prompt_echo("Okafor", 0.5, &chat, &h));
        assert!(!is_prompt_echo("yes", 0.4, &chat, &h));
    }

    #[test]
    fn notes_for_sounds_that_arent_speech_go() {
        assert_eq!(clean("[BLANK_AUDIO]"), "");
        assert_eq!(clean(" (silence) "), "");
        assert_eq!(clean("[ Silence ] check logs"), "check logs");
        assert_eq!(clean("check logs (music playing)"), "check logs");
        assert_eq!(clean("*coughs* git push"), "git push");
        assert_eq!(clean("[typing] hello [MUSIC]"), "hello");
    }

    #[test]
    fn real_brackets_and_symbols_stay() {
        assert_eq!(clean("print(x)"), "print(x)", "round brackets that aren't a note");
        assert_eq!(clean("open paren x close paren"), "open paren x close paren");
        assert_eq!(clean("a * b"), "a * b");
        assert_eq!(clean("a *b2* c"), "a *b2* c", "not a note: has a digit");
        assert_eq!(clean("( unclosed"), "( unclosed");
        assert_eq!(clean("x [ unclosed"), "x [ unclosed");
    }

    #[test]
    fn the_full_stop_whisper_adds_goes() {
        assert_eq!(clean(" check logs."), "check logs");
        assert_eq!(clean("Hello, how are you?"), "Hello, how are you?", "a question stays");
        assert_eq!(clean("wait..."), "wait...", "an ellipsis stays");
        assert_eq!(
            clean("const user id equals five semicolon ."),
            "const user id equals five semicolon"
        );
        assert_eq!(clean("  lots   of   space  "), "lots of space");
        assert_eq!(clean(""), "");
        assert_eq!(clean("."), "");
    }

    #[test]
    fn cleaned_whisper_text_formats_like_windows_text() {
        let code = target_for_process("code.exe");
        assert_eq!(
            format(&clean(" const camel case user id equals five semicolon."), code),
            "const userId = five;"
        );
        assert_eq!(format(&clean("check logs."), code), "check logs");
        let prose = target_for_process("notepad.exe");
        assert_eq!(
            format(&clean(" hello comma how are you question mark"), prose),
            "Hello, how are you?"
        );
    }
}
