//! What the accurate engine is told before it listens, and the tidying of what it hears. Pure
//! and tested; nothing here is logged.
//!
//! **The prompt.** whisper.cpp takes "previous text" that biases what it expects to hear next.
//! tug gives it the spoken commands `text.rs` understands ("camel case", "open paren", "new line",
//! "dot", "equals"…) and everyday coding words ("check logs", "git", "npm"…), all lower case with
//! no punctuation: Whisper continues in the style of the prompt, so it writes "comma" as the word
//! and leaves out capitals and full stops it would otherwise add, and `text.rs` turns the words
//! into symbols exactly as it does for Windows' recognizer.
//!
//! **The tidy-up.** Whisper marks sounds that aren't speech ("[BLANK_AUDIO]", "(silence)") and
//! sometimes ends a phrase with a full stop nobody said; both go before `text::format` runs.

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

/// The longest prompt, in characters. whisper.cpp keeps at most half its text context (224
/// tokens) of prompt; common English words are about four characters a token, so this stays well
/// inside it and nothing useful is cut from the front.
pub const PROMPT_MAX_CHARS: usize = 720;

/// The prompt: the examples, then the commands, then the coding words, lower case and space
/// separated, stopping before [`PROMPT_MAX_CHARS`] (never part way through a phrase).
pub fn prompt() -> String {
    let mut out = String::new();
    let mut seen: Vec<&str> = Vec::new();
    for phrase in EXAMPLES.iter().chain(COMMANDS).chain(CODE_WORDS) {
        if seen.contains(phrase) {
            continue;
        }
        let extra = phrase.len() + usize::from(!out.is_empty());
        if out.len() + extra > PROMPT_MAX_CHARS {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(phrase);
        seen.push(phrase);
    }
    out
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

/// Whether what Whisper heard is really its prompt played back. On a cough, a click or a breath
/// it can't make out, Whisper sometimes "continues" its prompt instead of transcribing: an
/// EXAMPLES line, or a run of the command list. That's dropped, but only when the hold was too
/// short to have said it (`speech_secs`, the trimmed recording): the issue's own test phrases
/// are EXAMPLES lines, and "open paren close paren open bracket" is a run of the prompt, and
/// someone saying them must still get them typed.
pub fn is_prompt_echo(heard: &str, speech_secs: f32) -> bool {
    let words = plain_words(heard);
    if words.len() <= most_words(speech_secs) {
        return false;
    }
    if EXAMPLES.iter().any(|e| plain_words(e) == words) {
        return true;
    }
    // Four or more words in a row exactly as they are in the prompt.
    let prompt = plain_words(&prompt());
    words.windows(4).any(|run| prompt.windows(4).any(|p| p == run))
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

    #[test]
    fn the_prompt_played_back_on_a_cough_is_dropped() {
        // An example line from a 0.4 s click: far more words than that could hold.
        assert!(is_prompt_echo("Const camel case user ID equals five semicolon.", 0.4));
        assert!(is_prompt_echo("hello, comma how are you question mark", 0.6));
        // A run of the command list.
        assert!(is_prompt_echo(
            "open bracket close bracket open brace close brace new line",
            0.5
        ));
        assert!(is_prompt_echo("camel case snake case pascal case", 0.3));
    }

    #[test]
    fn saying_the_same_words_still_types_them() {
        // The issue's test phrases are EXAMPLES lines: said over a couple of seconds, they stay.
        assert!(!is_prompt_echo("const camel case user id equals five semicolon", 2.6));
        assert!(!is_prompt_echo("hello comma how are you question mark", 2.0));
        // A run of commands, said.
        assert!(!is_prompt_echo("open paren close paren open bracket", 1.5));
        // Short and real, even from a short hold.
        assert!(!is_prompt_echo("check logs", 0.4));
        assert!(!is_prompt_echo("git push", 0.2));
        // Not from the prompt at all, however fast.
        assert!(!is_prompt_echo("the quick brown fox jumps over the lazy dog", 0.5));
        assert!(!is_prompt_echo("", 0.0));
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
