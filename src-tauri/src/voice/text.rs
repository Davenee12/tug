//! Voice typing's text rules: turning what the recognizer heard into what gets typed.
//!
//! Pure (plain strings in, plain strings out) and tested, so the rules can be read and changed
//! without a microphone:
//!
//! - **Spoken symbols**: "open paren" → `(`, "dot" → `.`, "equals" → ` = `, "new line", …, with
//!   code-style spacing (`print("hi")`, `user.name`, `x = 5;`).
//! - **Casing commands**: "camel case user id" → `userId`, also snake, pascal, kebab and constant
//!   case, "capitalize" and "all caps" for one word, and "literal" to type a command word as a word.
//! - **Where it's going**: in a code editor or terminal (by the foreground program's name) the
//!   recognizer's sentence capitals are undone; elsewhere the first word is capitalised. In a
//!   terminal "new line" is left out, so dictation can never run a command by itself; elsewhere a
//!   new line is typed as Shift+Enter (a line break in editors and chat boxes, never Send).
//!
//! Nothing here is logged: the text is the person's.

/// Where the words are going, from the foreground program (see [`style_for_process`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// Mail, chat, documents: sentence style.
    Prose,
    /// A code editor or IDE: no sentence capitals.
    Editor,
    /// A terminal: like an editor, and no new lines (Enter would run the command).
    Terminal,
}

impl Style {
    fn is_code(self) -> bool {
        matches!(self, Style::Editor | Style::Terminal)
    }
}

/// Code editors and IDEs, by executable name (lowercase).
const EDITORS: &[&str] = &[
    "code.exe",
    "code - insiders.exe",
    "vscodium.exe",
    "cursor.exe",
    "windsurf.exe",
    "zed.exe",
    "devenv.exe",
    "idea64.exe",
    "idea.exe",
    "pycharm64.exe",
    "webstorm64.exe",
    "rider64.exe",
    "clion64.exe",
    "goland64.exe",
    "rustrover64.exe",
    "phpstorm64.exe",
    "datagrip64.exe",
    "studio64.exe",
    "fleet.exe",
    "sublime_text.exe",
    "notepad++.exe",
    "gvim.exe",
    "nvim-qt.exe",
    "neovide.exe",
    "emacs.exe",
    "ssms.exe",
    "azuredatastudio.exe",
];

/// Terminals and shells, by executable name (lowercase).
const TERMINALS: &[&str] = &[
    "windowsterminal.exe",
    "wt.exe",
    "openconsole.exe",
    "conhost.exe",
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "wezterm-gui.exe",
    "alacritty.exe",
    "mintty.exe",
    "hyper.exe",
    "tabby.exe",
    "warp.exe",
    "kitty.exe",
];

/// The style for a foreground program, from its path or file name ("C:\…\Code.exe" or "Code.exe").
pub fn style_for_process(exe: &str) -> Style {
    let name = exe
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(exe)
        .trim()
        .to_ascii_lowercase();
    if TERMINALS.contains(&name.as_str()) {
        Style::Terminal
    } else if EDITORS.contains(&name.as_str()) {
        Style::Editor
    } else {
        Style::Prose
    }
}

/// How a symbol sits against its neighbours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Space {
    /// `(` `[` `{` `@` `#`: nothing after it.
    Open,
    /// `)` `]` `}`: nothing before it.
    Close,
    /// `.` `_` `-` `/` `::`: nothing either side.
    Join,
    /// `,` `;` `:` `?` `!` and a full stop: nothing before, a space after.
    Trail,
    /// `=` `+` `==` `->`: a space either side.
    Spaced,
    /// `"` `'` `` ` ``: opens, then closes.
    Quote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Casing {
    Camel,
    Pascal,
    Snake,
    Kebab,
    Constant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Sym(&'static str, Space),
    NewLine(&'static str),
    Case(Casing),
    Capitalize,
    AllCaps,
    Literal,
    Filler,
}

use Command::*;
use Space::*;

/// Spoken phrases, matched longest first, whole words, ignoring case.
const PHRASES: &[(&str, Command)] = &[
    // Brackets
    ("open paren", Sym("(", Open)),
    ("open parenthesis", Sym("(", Open)),
    ("left paren", Sym("(", Open)),
    ("left parenthesis", Sym("(", Open)),
    ("close paren", Sym(")", Close)),
    ("close parenthesis", Sym(")", Close)),
    ("right paren", Sym(")", Close)),
    ("right parenthesis", Sym(")", Close)),
    ("open bracket", Sym("[", Open)),
    ("open square bracket", Sym("[", Open)),
    ("left bracket", Sym("[", Open)),
    ("close bracket", Sym("]", Close)),
    ("close square bracket", Sym("]", Close)),
    ("right bracket", Sym("]", Close)),
    ("open brace", Sym("{", Open)),
    ("open curly", Sym("{", Open)),
    ("open curly brace", Sym("{", Open)),
    ("left brace", Sym("{", Open)),
    ("close brace", Sym("}", Close)),
    ("close curly", Sym("}", Close)),
    ("close curly brace", Sym("}", Close)),
    ("right brace", Sym("}", Close)),
    ("open angle", Sym("<", Open)),
    ("open angle bracket", Sym("<", Open)),
    ("close angle", Sym(">", Close)),
    ("close angle bracket", Sym(">", Close)),
    // Quotes
    ("quote", Sym("\"", Quote)),
    ("double quote", Sym("\"", Quote)),
    ("open quote", Sym("\"", Open)),
    ("close quote", Sym("\"", Close)),
    ("end quote", Sym("\"", Close)),
    ("single quote", Sym("'", Quote)),
    ("backtick", Sym("`", Quote)),
    ("back tick", Sym("`", Quote)),
    // Joiners
    ("dot", Sym(".", Join)),
    ("underscore", Sym("_", Join)),
    ("dash", Sym("-", Join)),
    ("hyphen", Sym("-", Join)),
    ("slash", Sym("/", Join)),
    ("forward slash", Sym("/", Join)),
    ("backslash", Sym("\\", Join)),
    ("back slash", Sym("\\", Join)),
    ("double colon", Sym("::", Join)),
    // Punctuation
    ("period", Sym(".", Trail)),
    ("full stop", Sym(".", Trail)),
    ("comma", Sym(",", Trail)),
    ("colon", Sym(":", Trail)),
    ("semicolon", Sym(";", Trail)),
    ("semi colon", Sym(";", Trail)),
    ("question mark", Sym("?", Trail)),
    ("exclamation mark", Sym("!", Trail)),
    ("exclamation point", Sym("!", Trail)),
    ("percent sign", Sym("%", Trail)),
    // Operators
    ("equals", Sym("=", Spaced)),
    ("equal sign", Sym("=", Spaced)),
    ("equals sign", Sym("=", Spaced)),
    ("double equals", Sym("==", Spaced)),
    ("triple equals", Sym("===", Spaced)),
    ("not equals", Sym("!=", Spaced)),
    ("not equal", Sym("!=", Spaced)),
    ("plus", Sym("+", Spaced)),
    ("plus sign", Sym("+", Spaced)),
    ("plus equals", Sym("+=", Spaced)),
    ("minus", Sym("-", Spaced)),
    ("minus sign", Sym("-", Spaced)),
    ("minus equals", Sym("-=", Spaced)),
    ("star", Sym("*", Spaced)),
    ("asterisk", Sym("*", Spaced)),
    ("arrow", Sym("->", Spaced)),
    ("fat arrow", Sym("=>", Spaced)),
    ("greater than", Sym(">", Spaced)),
    ("less than", Sym("<", Spaced)),
    ("greater than or equal", Sym(">=", Spaced)),
    ("less than or equal", Sym("<=", Spaced)),
    ("ampersand", Sym("&", Spaced)),
    ("double ampersand", Sym("&&", Spaced)),
    ("pipe", Sym("|", Spaced)),
    ("double pipe", Sym("||", Spaced)),
    // Prefixes
    ("at sign", Sym("@", Open)),
    ("at symbol", Sym("@", Open)),
    ("hash", Sym("#", Open)),
    ("hash sign", Sym("#", Open)),
    ("pound sign", Sym("#", Open)),
    ("dollar sign", Sym("$", Open)),
    ("bang", Sym("!", Open)),
    ("tilde", Sym("~", Open)),
    // Lines
    ("new line", NewLine("\n")),
    ("newline", NewLine("\n")),
    ("next line", NewLine("\n")),
    ("new paragraph", NewLine("\n\n")),
    // Casing
    ("camel case", Case(Casing::Camel)),
    ("camelcase", Case(Casing::Camel)),
    // How Windows' recognizer often hears them before it's used to a voice (nonsense otherwise).
    ("cattle case", Case(Casing::Camel)),
    ("camels case", Case(Casing::Camel)),
    ("rascal case", Case(Casing::Pascal)),
    ("kabob case", Case(Casing::Kebab)),
    ("kebob case", Case(Casing::Kebab)),
    ("pascal case", Case(Casing::Pascal)),
    ("snake case", Case(Casing::Snake)),
    ("kebab case", Case(Casing::Kebab)),
    ("constant case", Case(Casing::Constant)),
    ("screaming snake case", Case(Casing::Constant)),
    ("capitalize", Capitalize),
    ("all caps", AllCaps),
    ("literal", Literal),
    // Fillers
    ("um", Filler),
    ("umm", Filler),
    ("uh", Filler),
    ("uhm", Filler),
    ("erm", Filler),
];

/// The longest phrase in [`PHRASES`] starting at `words[i]`: `(command, words used)`.
fn match_phrase(words: &[String], i: usize) -> Option<(Command, usize)> {
    let mut best: Option<(Command, usize)> = None;
    for (phrase, cmd) in PHRASES {
        let n = phrase.split(' ').count();
        if i + n > words.len() || best.is_some_and(|(_, m)| m >= n) {
            continue;
        }
        let hit = phrase
            .split(' ')
            .zip(&words[i..i + n])
            .all(|(p, w)| w.trim_matches(is_edge_punct).eq_ignore_ascii_case(p));
        if hit {
            best = Some((*cmd, n));
        }
    }
    best
}

/// A symbol the recognizer typed itself (it writes "," for "comma"), with the spacing it gets.
fn engine_symbol(tok: &str, style: Style) -> Option<Command> {
    Some(match tok {
        "(" | "[" | "{" => Sym(static_sym(tok), Open),
        ")" | "]" | "}" => Sym(static_sym(tok), Close),
        "," | ";" | ":" | "?" | "!" => Sym(static_sym(tok), Trail),
        "." if style.is_code() => Sym(".", Join),
        "." => Sym(".", Trail),
        "\"" => Sym("\"", Quote),
        "=" | "+" => Sym(static_sym(tok), Spaced),
        "\n" => NewLine("\n"),
        _ => return None,
    })
}

fn static_sym(tok: &str) -> &'static str {
    match tok {
        "(" => "(",
        "[" => "[",
        "{" => "{",
        ")" => ")",
        "]" => "]",
        "}" => "}",
        "," => ",",
        ";" => ";",
        ":" => ":",
        "?" => "?",
        "!" => "!",
        "=" => "=",
        "+" => "+",
        _ => "",
    }
}

/// Punctuation the recognizer may glue to a word ("world." "Hello,"), split off as its own token.
fn is_edge_punct(c: char) -> bool {
    matches!(
        c,
        '.' | ',' | ';' | ':' | '?' | '!' | '(' | ')' | '[' | ']' | '{' | '}' | '"'
    )
}

/// Words and the punctuation glued to their edges, as separate tokens; a line break is a token.
fn tokens(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line_part in raw.replace("\r\n", "\n").split_inclusive('\n') {
        let (body, nl) = match line_part.strip_suffix('\n') {
            Some(b) => (b, true),
            None => (line_part, false),
        };
        for word in body.split_whitespace() {
            let lead: String = word.chars().take_while(|c| is_edge_punct(*c)).collect();
            let rest = &word[lead.len()..];
            let core = rest.trim_end_matches(is_edge_punct);
            let trail = &rest[core.len()..];
            out.extend(lead.chars().map(String::from));
            if !core.is_empty() {
                out.push(core.to_string());
            }
            out.extend(trail.chars().map(String::from));
        }
        if nl {
            out.push("\n".to_string());
        }
    }
    out
}

/// One piece of output before spacing is decided.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Item {
    /// A word; `fixed` when a command shaped it (its case is left alone).
    Word {
        text: String,
        fixed: bool,
    },
    Sym(&'static str, Space),
    NewLine(&'static str),
}

fn case_words(words: &[String], casing: Casing) -> String {
    let lower: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
    let cap = |w: &str| {
        let mut c = w.chars();
        c.next()
            .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
            .unwrap_or_default()
    };
    match casing {
        Casing::Camel => lower
            .iter()
            .enumerate()
            .map(|(i, w)| if i == 0 { w.clone() } else { cap(w) })
            .collect(),
        Casing::Pascal => lower.iter().map(|w| cap(w)).collect(),
        Casing::Snake => lower.join("_"),
        Casing::Kebab => lower.join("-"),
        Casing::Constant => lower.join("_").to_uppercase(),
    }
}

/// Read the tokens into items: commands become symbols and shaped words.
fn items(toks: &[String], style: Style) -> Vec<Item> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        if let Some(sym) = engine_symbol(&toks[i], style) {
            push_command(&mut out, sym);
            i += 1;
            continue;
        }
        let Some((cmd, n)) = match_phrase(toks, i) else {
            out.push(Item::Word {
                text: toks[i].clone(),
                fixed: false,
            });
            i += 1;
            continue;
        };
        match cmd {
            Filler => i += n,
            Literal => {
                // "literal dot": the next word as a word, never a command.
                match toks.get(i + n) {
                    Some(next) if engine_symbol(next, style).is_none() => {
                        out.push(Item::Word {
                            text: next.clone(),
                            fixed: true,
                        });
                        i += n + 1;
                    }
                    _ => {
                        out.push(Item::Word {
                            text: toks[i].clone(),
                            fixed: false,
                        });
                        i += 1;
                    }
                }
            }
            Capitalize | AllCaps => match plain_word(toks, i + n, style) {
                Some(w) => {
                    let text = if cmd == AllCaps {
                        w.to_uppercase()
                    } else {
                        case_words(std::slice::from_ref(w), Casing::Pascal)
                    };
                    out.push(Item::Word { text, fixed: true });
                    i += n + 1;
                }
                None => {
                    out.push(Item::Word {
                        text: toks[i].clone(),
                        fixed: false,
                    });
                    i += 1;
                }
            },
            Case(casing) => {
                // The words up to the next symbol or command, joined in that case.
                let mut j = i + n;
                let mut words = Vec::new();
                while let Some(w) = plain_word(toks, j, style) {
                    words.push(w.clone());
                    j += 1;
                }
                if words.is_empty() {
                    // Just the words "camel case": type them as said.
                    out.extend(toks[i..i + n].iter().map(|t| Item::Word {
                        text: t.clone(),
                        fixed: false,
                    }));
                } else {
                    out.push(Item::Word {
                        text: case_words(&words, casing),
                        fixed: true,
                    });
                }
                i = j.max(i + n);
            }
            other => {
                push_command(&mut out, other);
                i += n;
            }
        }
    }
    out
}

/// `toks[i]` if it's an ordinary word (not a symbol, not the start of a command).
fn plain_word(toks: &[String], i: usize, style: Style) -> Option<&String> {
    let t = toks.get(i)?;
    (engine_symbol(t, style).is_none() && match_phrase(toks, i).is_none()).then_some(t)
}

fn push_command(out: &mut Vec<Item>, cmd: Command) {
    match cmd {
        Sym(s, sp) => out.push(Item::Sym(s, sp)),
        NewLine(s) => out.push(Item::NewLine(s)),
        _ => {}
    }
}

/// Settle each quote as opening or closing: the first of a kind opens, the next closes.
fn settle_quotes(items: &mut [Item]) {
    let mut open: Vec<&'static str> = Vec::new();
    for item in items.iter_mut() {
        match item {
            Item::Sym(s, sp @ Quote) => {
                let s = *s;
                if let Some(pos) = open.iter().position(|q| *q == s) {
                    open.remove(pos);
                    *sp = Close;
                } else {
                    open.push(s);
                    *sp = Open;
                }
            }
            Item::Sym(s, Open) if *s == "\"" => open.push(*s),
            Item::Sym(s, Close) if *s == "\"" => {
                let s = *s;
                if let Some(pos) = open.iter().position(|q| *q == s) {
                    open.remove(pos);
                }
            }
            Item::NewLine(_) => open.clear(),
            _ => {}
        }
    }
}

/// Whether a symbol attaches to the word before it. In code, a call's `(` and an index's `[`
/// hug the name (`print(`, `items[`); in prose they're spaced like words.
fn hugs_left(item: &Item, style: Style) -> bool {
    match item {
        Item::Sym(s, Open) => style.is_code() && matches!(*s, "(" | "[" | "<"),
        Item::Sym(_, Close | Join | Trail) | Item::NewLine(_) => true,
        _ => false,
    }
}

fn hugs_right(item: &Item) -> bool {
    matches!(item, Item::Sym(_, Open | Join) | Item::NewLine(_))
}

fn ends_sentence(item: &Item) -> bool {
    matches!(
        item,
        Item::Sym(".", Trail) | Item::Sym("?" | "!", Trail) | Item::NewLine(_)
    )
}

/// "Hello" (as the recognizer capitalises a sentence) but not "I", "API" or "iPhone".
fn sentence_capital(w: &str) -> bool {
    let mut c = w.chars();
    match c.next() {
        Some(f) if f.is_uppercase() => {
            let rest = c.as_str();
            !rest.is_empty() && !rest.chars().any(char::is_uppercase)
        }
        _ => false,
    }
}

fn set_first(w: &str, upper: bool) -> String {
    let mut c = w.chars();
    match c.next() {
        Some(f) if upper => f.to_uppercase().collect::<String>() + c.as_str(),
        Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Turn what the recognizer heard into the text to type, for where it's going.
pub fn format(raw: &str, style: Style) -> String {
    let toks = tokens(raw);
    let mut items = items(&toks, style);
    if style == Style::Terminal {
        // Enter runs the command: in a terminal, a spoken new line is left out.
        items.retain(|i| !matches!(i, Item::NewLine(_)));
    }
    settle_quotes(&mut items);

    let mut out = String::new();
    let mut prev: Option<&Item> = None;
    let mut sentence_start = true;
    for item in &items {
        if let Some(p) = prev {
            if !hugs_right(p) && !hugs_left(item, style) {
                out.push(' ');
            }
        }
        match item {
            Item::Word { text, fixed } => {
                let text = if *fixed || !sentence_start {
                    text.clone()
                } else if style.is_code() {
                    if sentence_capital(text) {
                        set_first(text, false)
                    } else {
                        text.clone()
                    }
                } else {
                    set_first(text, true)
                };
                out.push_str(&text);
                sentence_start = false;
            }
            Item::Sym(s, _) => {
                out.push_str(s);
                if ends_sentence(item) {
                    sentence_start = true;
                }
            }
            Item::NewLine(s) => {
                out.push_str(s);
                sentence_start = true;
            }
        }
        prev = Some(item);
    }
    out.trim_matches(' ').to_string()
}

/// A keystroke to send: a character, or a line break.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stroke {
    /// One UTF-16 unit, typed as a Unicode character (layout-independent).
    Unit(u16),
    /// Shift+Enter: a new line in editors and chat boxes, where plain Enter would send.
    NewLine,
}

/// The strokes that type `text`. Line breaks become Shift+Enter; carriage returns are dropped.
pub fn strokes(text: &str) -> Vec<Stroke> {
    let mut out = Vec::new();
    for ch in text.chars() {
        match ch {
            '\n' => out.push(Stroke::NewLine),
            '\r' => {}
            c => {
                let mut buf = [0u16; 2];
                out.extend(c.encode_utf16(&mut buf).iter().map(|u| Stroke::Unit(*u)));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(s: &str) -> String {
        format(s, Style::Editor)
    }
    fn prose(s: &str) -> String {
        format(s, Style::Prose)
    }

    #[test]
    fn styles_from_the_foreground_program() {
        assert_eq!(
            style_for_process(r"C:\Program Files\Microsoft VS Code\Code.exe"),
            Style::Editor
        );
        assert_eq!(style_for_process("cursor.exe"), Style::Editor);
        assert_eq!(style_for_process("Cursor.EXE"), Style::Editor);
        assert_eq!(style_for_process("devenv.exe"), Style::Editor);
        assert_eq!(style_for_process("WindowsTerminal.exe"), Style::Terminal);
        assert_eq!(style_for_process(r"C:\Windows\System32\cmd.exe"), Style::Terminal);
        assert_eq!(style_for_process("pwsh.exe"), Style::Terminal);
        assert_eq!(style_for_process("chrome.exe"), Style::Prose);
        assert_eq!(style_for_process("OUTLOOK.EXE"), Style::Prose);
        assert_eq!(style_for_process(""), Style::Prose);
    }

    #[test]
    fn spoken_symbols_with_code_spacing() {
        assert_eq!(
            code("print open paren quote hello quote close paren"),
            r#"print("hello")"#
        );
        assert_eq!(code("self dot name"), "self.name");
        assert_eq!(code("x equals 5 semicolon"), "x = 5;");
        assert_eq!(code("x plus equals 1"), "x += 1");
        assert_eq!(code("if a double equals b open brace"), "if a == b {");
        assert_eq!(code("items open bracket 0 close bracket"), "items[0]");
        assert_eq!(code("a fat arrow b"), "a => b");
        assert_eq!(code("a not equals b"), "a != b");
        assert_eq!(code("src slash lib slash main dot rs"), "src/lib/main.rs");
        assert_eq!(code("std double colon io"), "std::io");
        assert_eq!(code("dash dash force"), "--force");
        assert_eq!(code("at sign param"), "@param");
        assert_eq!(code("bang is ready"), "!is ready");
        assert_eq!(code("single quote a single quote"), "'a'");
        assert_eq!(code("backtick npm test backtick"), "`npm test`");
    }

    #[test]
    fn casing_commands() {
        assert_eq!(code("camel case user id"), "userId");
        assert_eq!(code("pascal case user id"), "UserId");
        assert_eq!(code("snake case user id"), "user_id");
        assert_eq!(code("kebab case user id"), "user-id");
        assert_eq!(code("constant case max retries"), "MAX_RETRIES");
        assert_eq!(
            code("Camel case User ID"),
            "userId",
            "the recognizer's capitals don't leak in"
        );
        assert_eq!(
            code("const camel case user id equals 5 semicolon"),
            "const userId = 5;",
            "casing stops at the next symbol"
        );
        assert_eq!(
            code("user dot camel case get name open paren close paren"),
            "user.getName()"
        );
        assert_eq!(code("snake case get user open paren close paren"), "get_user()");
        assert_eq!(code("capitalize react component"), "React component");
        assert_eq!(code("all caps id"), "ID");
        // Nothing to shape: the words as said.
        assert_eq!(prose("I like camel case"), "I like camel case");
        assert_eq!(code("camel case comma"), "camel case,");
    }

    #[test]
    fn what_the_recognizer_really_writes() {
        // Captured from Windows' recognizer (text-to-speech saying the commands).
        assert_eq!(code("print (quote hello quote)"), r#"print("hello")"#);
        assert_eq!(code("const cattle case user ID equals five;"), "const userId = five;");
        assert_eq!(prose("hello, how are you?\nThanks."), "Hello, how are you?\nThanks.");
        assert_eq!(code("hello, how are you?\nThanks."), "hello, how are you?\nthanks.");
    }

    #[test]
    fn literal_types_a_command_word_as_a_word() {
        assert_eq!(code("literal dot"), "dot");
        assert_eq!(prose("press the literal period key"), "Press the period key");
        assert_eq!(code("literal"), "literal");
    }

    #[test]
    fn code_apps_lose_sentence_capitals() {
        assert_eq!(code("Hello world"), "hello world");
        assert_eq!(code("Fix the bug period Then ship it"), "fix the bug. then ship it");
        // Real capitals stay.
        assert_eq!(code("I think the API is down"), "I think the API is down");
        assert_eq!(code("use React here"), "use React here");
    }

    #[test]
    fn prose_reads_like_a_sentence() {
        assert_eq!(prose("hello comma how are you question mark"), "Hello, how are you?");
        assert_eq!(prose("this is great period new line thanks"), "This is great.\nThanks");
        assert_eq!(prose("see you soon exclamation mark"), "See you soon!");
        assert_eq!(prose("visit example dot com"), "Visit example.com");
        assert_eq!(prose("2 plus 2 equals 4"), "2 + 2 = 4");
        assert_eq!(prose("one new paragraph two"), "One\n\nTwo");
        // No full stop is invented.
        assert_eq!(prose("on my way"), "On my way");
    }

    #[test]
    fn symbols_the_recognizer_typed_itself() {
        assert_eq!(prose("Hello, world."), "Hello, world.");
        assert_eq!(code("Call foo ( )"), "call foo()");
        assert_eq!(prose("one\ntwo"), "One\nTwo");
        assert_eq!(code("a ."), "a.");
    }

    #[test]
    fn terminals_never_get_a_new_line() {
        assert_eq!(
            format("git status new line rm -rf", Style::Terminal),
            "git status rm -rf"
        );
        assert_eq!(format("ls\nrm", Style::Terminal), "ls rm");
        assert_eq!(format("Git status", Style::Terminal), "git status");
    }

    #[test]
    fn fillers_and_empty_input() {
        assert_eq!(code("um open paren uh close paren"), "()");
        assert_eq!(prose(""), "");
        assert_eq!(prose("   "), "");
        assert_eq!(prose("um"), "");
    }

    #[test]
    fn phrases_are_whole_words() {
        // "dots" and "plussed" aren't commands.
        assert_eq!(prose("three dots"), "Three dots");
        assert_eq!(code("pluses"), "pluses");
        // Longest phrase wins over its prefix.
        assert_eq!(code("a greater than or equal b"), "a >= b");
        assert_eq!(code("a greater than b"), "a > b");
    }

    #[test]
    fn strokes_type_text_and_line_breaks() {
        assert_eq!(
            strokes("a\r\nb"),
            vec![Stroke::Unit(0x61), Stroke::NewLine, Stroke::Unit(0x62)]
        );
        // Outside the BMP: a surrogate pair.
        assert_eq!(strokes("😀").len(), 2);
        assert!(strokes("").is_empty());
    }
}
