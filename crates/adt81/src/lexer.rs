//! The ADT'81 lexer: source text to tokens, by the lexical rules at the end
//! of `crates/adt81/grammar.lark`.
//!
//! Matches what Lark's basic lexer does with that grammar, so that the Rust
//! parser and the grammar gate see the same tokens: the longest symbol wins,
//! a word in [`KEYWORDS`], in any case, is a keyword, a number glued to `i`,
//! `j` or `k` is imaginary, and `--` opens a comment only if another `--`
//! closes it on the same line (otherwise it is two minus signs).

use std::fmt;

/// Every keyword of the grammar, as its string literals write them; a word
/// is looked up upper-cased.
pub const KEYWORDS: &[&str] = &[
    "ACTIVATE",
    "AFTER",
    "AND",
    "APPR",
    "AS",
    "ASSERT",
    "ASSOC",
    "AT",
    "AWAKE",
    "BEFORE",
    "BEGIN",
    "BOOLEAN",
    "BUILD",
    "CALL",
    "CANCEL",
    "CASE",
    "CATTRIBUTE",
    "CHAIN",
    "CHARACTER",
    "CIRCULAR",
    "CLASS",
    "CLINK",
    "COMPL",
    "COMPLEX",
    "COMPOSE",
    "COMPOUND",
    "COMPUTED",
    "CONSTANTE",
    "COROUTINE",
    "CREATE",
    "DELAY",
    "DESIGN",
    "DESTROY",
    "DISSOC",
    "DO",
    "EACH",
    "ELSE",
    "EMBRYO",
    "EMPTY",
    "END",
    "ENDCASE",
    "ENDEACH",
    "ENDIF",
    "ENDLOOP",
    "ENDPAR",
    "ENDVECT",
    "EXCLUDE",
    "EXIT",
    "EXTERNAL",
    "EXTRACT",
    "FALSE",
    "FLOAT",
    "FOR",
    "FROM",
    "FUNCTION",
    "FUNCTOR",
    "FUZZY",
    "GAUSS",
    "HATTRIBUTE",
    "HCLASS",
    "HIBERN",
    "HLINK",
    "HOLD",
    "IF",
    "IN",
    "INCLUDE",
    "INIT",
    "INOUT",
    "INSERT",
    "INTEGER",
    "INTER",
    "INTO",
    "KILL",
    "LET",
    "LINEAR",
    "LIST",
    "LOCAL",
    "LONG",
    "LOOP",
    "MATRIX",
    "MOD",
    "MULT",
    "NAND",
    "NEXT",
    "NOR",
    "NOT",
    "NOTIN",
    "OATTRIBUTE",
    "OBJECT",
    "OF",
    "OLINK",
    "OR",
    "OTHERS",
    "OUT",
    "OUTOF",
    "PAR",
    "PARBEGIN",
    "PASSIVATE",
    "PAUSE",
    "PRINT",
    "PROB",
    "PROGRAM",
    "PROTOTYPE",
    "PURGE",
    "QUATERNION",
    "RATIONAL",
    "REACTIVATE",
    "READ",
    "REFLEXIVE",
    "RELOOP",
    "REPROC",
    "RESET",
    "RESTART",
    "RESULT",
    "RESUME",
    "RESUR",
    "RETURN",
    "RETURNS",
    "REVERSE",
    "SINGLE",
    "SPARSE",
    "SPECIFY",
    "STEP",
    "STOP",
    "STRING",
    "STRUCTURE",
    "SUBJ",
    "SUBROUTINE",
    "TENSOR",
    "TERMINATE",
    "THEN",
    "THIS",
    "TO",
    "TRUE",
    "TUPLE",
    "UNION",
    "UNIT",
    "UNTIL",
    "VECT",
    "VECTOR",
    "WHEN",
    "WHILE",
    "WITH",
    "XOR",
];

/// Every symbol of the grammar, longest first, the order they are tried in.
pub const SYMBOLS: &[&str] = &[
    "[+]", "[*]", "[.]", "[^]", ":=", "/=", "<=", ">=", "//", "+-", ";", ",", "(", ")", ":", "[",
    "]", ".", "'", "=", "<", ">", "+", "-", "*", "/", "^", "&", "{", "}", "%", "@",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Ident,
    Keyword,
    /// Digits only.
    Integer,
    /// Digits, a point, maybe digits: `0.` is a float.
    Float,
    /// A number with its imaginary unit glued on, `2i`, `3.5j`; the text
    /// keeps the unit. Integers take `i` only (Gauss integers).
    Imaginary(Unit),
    /// A string literal; the text keeps its quotes.
    String,
    Symbol,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    I,
    J,
    K,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token<'a> {
    pub kind: Kind,
    pub text: &'a str,
    /// 1-based, as editors count.
    pub line: usize,
    pub column: usize,
}

impl Token<'_> {
    /// The name an identifier stands for; see [`fold_name`].
    pub fn name(&self) -> Option<String> {
        (self.kind == Kind::Ident).then(|| fold_name(self.text))
    }
}

/// ADT'81 is case-insensitive: `opscrc`, `Opscrc` and `OPSCRC` are one
/// name, and compare equal once folded; `loop` is the keyword `LOOP`.
pub fn fold_name(text: &str) -> String {
    text.to_ascii_uppercase()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub message: String,
    pub line: usize,
    pub column: usize,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.column, self.message)
    }
}

impl std::error::Error for Error {}

/// Splits `source` into tokens, comments and white space dropped.
pub fn tokenize(source: &str) -> Result<Vec<Token<'_>>, Error> {
    let mut lexer = Lexer {
        source,
        pos: 0,
        line: 1,
        column: 1,
    };
    let mut tokens = Vec::new();
    while let Some(token) = lexer.next_token()? {
        tokens.push(token);
    }
    Ok(tokens)
}

struct Lexer<'a> {
    source: &'a str,
    pos: usize,
    line: usize,
    column: usize,
}

impl<'a> Lexer<'a> {
    fn rest(&self) -> &'a str {
        &self.source[self.pos..]
    }

    fn peek(&self, ahead: usize) -> Option<u8> {
        self.source.as_bytes().get(self.pos + ahead).copied()
    }

    /// Moves `len` bytes on, keeping line and column.
    fn advance(&mut self, len: usize) -> &'a str {
        let text = &self.source[self.pos..self.pos + len];
        for c in text.chars() {
            if c == '\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
        }
        self.pos += len;
        text
    }

    fn error(&self, message: impl Into<String>) -> Error {
        Error {
            message: message.into(),
            line: self.line,
            column: self.column,
        }
    }

    fn next_token(&mut self) -> Result<Option<Token<'a>>, Error> {
        self.skip_blanks();
        let Some(c) = self.peek(0) else {
            return Ok(None);
        };
        let (line, column) = (self.line, self.column);
        let (kind, len) = if c.is_ascii_alphabetic() {
            let len = self.ident_len();
            let word = &self.rest()[..len];
            let kind = if KEYWORDS.binary_search(&fold_name(word).as_str()).is_ok() {
                Kind::Keyword
            } else {
                Kind::Ident
            };
            (kind, len)
        } else if c.is_ascii_digit() {
            self.number()
        } else if c == b'"' {
            match self.rest()[1..].find(['"', '\n']) {
                Some(end) if self.rest().as_bytes()[1 + end] == b'"' => (Kind::String, end + 2),
                _ => return Err(self.error("string not closed on its line")),
            }
        } else if let Some(symbol) = SYMBOLS.iter().find(|s| self.rest().starts_with(**s)) {
            (Kind::Symbol, symbol.len())
        } else {
            let ch = self.rest().chars().next().unwrap_or_default();
            return Err(self.error(format!("unexpected character {ch:?}")));
        };
        let text = self.advance(len);
        Ok(Some(Token {
            kind,
            text,
            line,
            column,
        }))
    }

    /// White space, and comments: from `--` to the next `--` on the line.
    fn skip_blanks(&mut self) {
        loop {
            let rest = self.rest();
            let blank = rest.len()
                - rest
                    .trim_start_matches([' ', '\t', '\r', '\n', '\x0c'])
                    .len();
            if blank > 0 {
                self.advance(blank);
                continue;
            }
            if let Some(body) = rest.strip_prefix("--") {
                let line = &body[..body.find('\n').unwrap_or(body.len())];
                if let Some(end) = line.find("--") {
                    self.advance(2 + end + 2);
                    continue;
                }
            }
            return;
        }
    }

    fn ident_len(&self) -> usize {
        self.rest()
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
            .count()
    }

    fn digits_from(&self, at: usize) -> usize {
        self.source.as_bytes()[self.pos + at..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count()
    }

    fn number(&self) -> (Kind, usize) {
        let mut len = self.digits_from(0);
        let float = self.peek(len) == Some(b'.');
        if float {
            len += 1 + self.digits_from(len + 1);
        }
        let unit = match self.peek(len) {
            Some(b'i') => Some(Unit::I),
            Some(b'j') if float => Some(Unit::J),
            Some(b'k') if float => Some(Unit::K),
            _ => None,
        };
        let glued = self
            .peek(len + 1)
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_');
        match unit {
            Some(unit) if !glued => (Kind::Imaginary(unit), len + 1),
            _ if float => (Kind::Float, len),
            _ => (Kind::Integer, len),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<(Kind, &str)> {
        tokenize(source)
            .unwrap()
            .into_iter()
            .map(|t| (t.kind, t.text))
            .collect()
    }

    #[test]
    fn keywords_are_sorted_for_binary_search() {
        assert!(KEYWORDS.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn keywords_and_symbols_match_the_grammar() {
        let grammar = include_str!("../grammar.lark");
        let mut words = Vec::new();
        let mut symbols = Vec::new();
        // Rule lines only: comments and terminal definitions (regular
        // expressions, upper case names) hold quotes that are not literals.
        let rules = grammar.lines().filter(|l| {
            let l = l.trim_start();
            !l.starts_with("//")
                && !l.starts_with('%')
                && !l.starts_with(|c: char| c.is_ascii_uppercase())
        });
        for line in rules {
            for (i, literal) in line.split('"').enumerate() {
                if i % 2 == 1 && !literal.is_empty() {
                    if literal.bytes().all(|b| b.is_ascii_uppercase()) {
                        words.push(literal);
                    } else {
                        symbols.push(literal);
                    }
                }
            }
        }
        words.sort_unstable();
        words.dedup();
        symbols.sort_unstable();
        symbols.dedup();
        assert_eq!(words, KEYWORDS);
        let mut ours = SYMBOLS.to_vec();
        ours.sort_unstable();
        assert_eq!(symbols, ours);
    }

    #[test]
    fn words() {
        assert_eq!(
            kinds("LOOP x_1 Loop"),
            [
                (Kind::Keyword, "LOOP"),
                (Kind::Ident, "x_1"),
                (Kind::Keyword, "Loop")
            ]
        );
    }

    #[test]
    fn case_insensitive() {
        let tokens = tokenize("opscrc OPSCRC Opscrc loop Loop LOOP looping \"Arc\"").unwrap();
        let names: Vec<_> = tokens.iter().map(Token::name).collect();
        let opscrc = Some("OPSCRC".to_string());
        assert_eq!(names[..3], [opscrc.clone(), opscrc.clone(), opscrc]);
        assert!(tokens[3..6].iter().all(|t| t.kind == Kind::Keyword));
        assert_eq!(names[6], Some("LOOPING".to_string()));
        assert_eq!(names[7], None);
    }

    #[test]
    fn numbers() {
        assert_eq!(
            kinds("12 0. 3.25 2i 3.5j 1.0k 2j 4ix"),
            [
                (Kind::Integer, "12"),
                (Kind::Float, "0."),
                (Kind::Float, "3.25"),
                (Kind::Imaginary(Unit::I), "2i"),
                (Kind::Imaginary(Unit::J), "3.5j"),
                (Kind::Imaginary(Unit::K), "1.0k"),
                (Kind::Integer, "2"),
                (Kind::Ident, "j"),
                (Kind::Integer, "4"),
                (Kind::Ident, "ix"),
            ]
        );
    }

    #[test]
    fn longest_symbol_wins() {
        assert_eq!(
            kinds("a:=b/=c//d+-e[+]f[g]"),
            [
                (Kind::Ident, "a"),
                (Kind::Symbol, ":="),
                (Kind::Ident, "b"),
                (Kind::Symbol, "/="),
                (Kind::Ident, "c"),
                (Kind::Symbol, "//"),
                (Kind::Ident, "d"),
                (Kind::Symbol, "+-"),
                (Kind::Ident, "e"),
                (Kind::Symbol, "[+]"),
                (Kind::Ident, "f"),
                (Kind::Symbol, "["),
                (Kind::Ident, "g"),
                (Kind::Symbol, "]"),
            ]
        );
    }

    #[test]
    fn comments_close_on_their_line() {
        assert_eq!(
            kinds("a -- note -- b"),
            [(Kind::Ident, "a"), (Kind::Ident, "b")]
        );
        assert_eq!(
            kinds("a -- b\nc --"),
            [
                (Kind::Ident, "a"),
                (Kind::Symbol, "-"),
                (Kind::Symbol, "-"),
                (Kind::Ident, "b"),
                (Kind::Ident, "c"),
                (Kind::Symbol, "-"),
                (Kind::Symbol, "-"),
            ]
        );
    }

    #[test]
    fn strings_keep_their_quotes() {
        assert_eq!(
            kinds(r#""carried by" @"ok""#),
            [
                (Kind::String, "\"carried by\""),
                (Kind::Symbol, "@"),
                (Kind::String, "\"ok\"")
            ]
        );
    }

    #[test]
    fn positions() {
        let tokens = tokenize("a\n  b").unwrap();
        assert_eq!((tokens[1].line, tokens[1].column), (2, 3));
    }

    #[test]
    fn errors() {
        assert_eq!(
            tokenize("x := \"open").unwrap_err().to_string(),
            "1:6: string not closed on its line"
        );
        assert_eq!(
            tokenize("a\n é").unwrap_err().to_string(),
            "2:2: unexpected character 'é'"
        );
    }
}
