//! nexus-lexer
//!
//! Converts NEXUS source text into a stream of [`Token`]s. The lexer never
//! aborts on a bad character or an unterminated string: it records a
//! diagnostic, synthesizes a reasonable token (or skips one character) and
//! keeps going, so downstream stages can still make progress and the user
//! sees every lexical problem in one pass.

use nexus_diagnostics::{Diagnostic, DiagnosticBag, Span};

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Literals
    Int(i64),
    Float(f64),
    Str(String),
    Ident(String),

    // Keywords
    KwFn,
    KwLet,
    KwMut,
    KwReturn,
    KwIf,
    KwElse,
    KwWhile,
    KwFor,
    KwIn,
    KwStruct,
    KwTrue,
    KwFalse,
    KwBreak,
    KwContinue,
    KwInt,
    KwFloat,
    KwBool,
    KwString,
    KwUnit,

    // Punctuation
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Semicolon,
    Dot,
    DotDot,
    Arrow, // ->

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eq,
    EqEq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    AndAnd,
    OrOr,
    Not,

    Eof,
}

impl TokenKind {
    pub fn describe(&self) -> String {
        match self {
            TokenKind::Int(n) => format!("integer `{n}`"),
            TokenKind::Float(n) => format!("float `{n}`"),
            TokenKind::Str(s) => format!("string \"{s}\""),
            TokenKind::Ident(s) => format!("identifier `{s}`"),
            TokenKind::Eof => "end of file".to_string(),
            other => format!("`{}`", token_text(other)),
        }
    }
}

fn token_text(kind: &TokenKind) -> &'static str {
    use TokenKind::*;
    match kind {
        KwFn => "fn",
        KwLet => "let",
        KwMut => "mut",
        KwReturn => "return",
        KwIf => "if",
        KwElse => "else",
        KwWhile => "while",
        KwFor => "for",
        KwIn => "in",
        KwStruct => "struct",
        KwTrue => "true",
        KwFalse => "false",
        KwBreak => "break",
        KwContinue => "continue",
        KwInt => "int",
        KwFloat => "float",
        KwBool => "bool",
        KwString => "string",
        KwUnit => "unit",
        LParen => "(",
        RParen => ")",
        LBrace => "{",
        RBrace => "}",
        LBracket => "[",
        RBracket => "]",
        Comma => ",",
        Colon => ":",
        Semicolon => ";",
        Dot => ".",
        DotDot => "..",
        Arrow => "->",
        Plus => "+",
        Minus => "-",
        Star => "*",
        Slash => "/",
        Percent => "%",
        Eq => "=",
        EqEq => "==",
        NotEq => "!=",
        Lt => "<",
        Gt => ">",
        LtEq => "<=",
        GtEq => ">=",
        AndAnd => "&&",
        OrOr => "||",
        Not => "!",
        _ => "?",
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

pub struct Lexer<'a> {
    _src: &'a str,
    chars: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
    diagnostics: DiagnosticBag,
}

fn keyword(ident: &str) -> Option<TokenKind> {
    use TokenKind::*;
    Some(match ident {
        "fn" => KwFn,
        "let" => KwLet,
        "mut" => KwMut,
        "return" => KwReturn,
        "if" => KwIf,
        "else" => KwElse,
        "while" => KwWhile,
        "for" => KwFor,
        "in" => KwIn,
        "struct" => KwStruct,
        "true" => KwTrue,
        "false" => KwFalse,
        "break" => KwBreak,
        "continue" => KwContinue,
        "int" => KwInt,
        "float" => KwFloat,
        "bool" => KwBool,
        "string" => KwString,
        "unit" => KwUnit,
        _ => return None,
    })
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Self {
            _src: src,
            chars: src.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
            diagnostics: DiagnosticBag::new(),
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek2(&self) -> Option<char> {
        self.chars.get(self.pos + 1).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn here(&self) -> (usize, usize) {
        (self.line, self.col)
    }

    /// Tokenizes the whole input, always terminating with `Eof`, and returns
    /// the token stream together with any lexical diagnostics collected
    /// along the way.
    pub fn tokenize(mut self) -> (Vec<Token>, DiagnosticBag) {
        let mut tokens = Vec::new();
        loop {
            self.skip_trivia();
            let (line, col) = self.here();
            let start = self.pos;
            let Some(c) = self.peek() else {
                tokens.push(Token { kind: TokenKind::Eof, span: Span::new(start, start, line, col) });
                break;
            };

            let kind = if c.is_ascii_digit() {
                self.lex_number()
            } else if c == '"' {
                self.lex_string()
            } else if is_ident_start(c) {
                self.lex_ident_or_keyword()
            } else {
                self.lex_operator()
            };

            if let Some(kind) = kind {
                let end = self.pos;
                tokens.push(Token { kind, span: Span::new(start, end, line, col) });
            }
        }
        (tokens, self.diagnostics)
    }

    fn skip_trivia(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() => {
                    self.bump();
                }
                Some('/') if self.peek2() == Some('/') => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.bump();
                    }
                }
                Some('/') if self.peek2() == Some('*') => {
                    self.bump();
                    self.bump();
                    loop {
                        match self.peek() {
                            None => break,
                            Some('*') if self.peek2() == Some('/') => {
                                self.bump();
                                self.bump();
                                break;
                            }
                            _ => {
                                self.bump();
                            }
                        }
                    }
                }
                _ => break,
            }
        }
    }

    fn lex_number(&mut self) -> Option<TokenKind> {
        let start = self.pos;
        while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
            self.bump();
        }
        let mut is_float = false;
        if self.peek() == Some('.') && matches!(self.peek2(), Some(c) if c.is_ascii_digit()) {
            is_float = true;
            self.bump();
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.bump();
            }
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        if is_float {
            Some(TokenKind::Float(text.parse().unwrap_or(0.0)))
        } else {
            match text.parse::<i64>() {
                Ok(v) => Some(TokenKind::Int(v)),
                Err(_) => {
                    let (line, col) = self.here();
                    self.diagnostics.push(
                        Diagnostic::error(
                            "E0001",
                            format!("integer literal `{text}` is out of range"),
                            Span::new(start, self.pos, line, col),
                        )
                        .with_help("NEXUS integers are 64-bit signed (`int`)"),
                    );
                    Some(TokenKind::Int(0))
                }
            }
        }
    }

    fn lex_string(&mut self) -> Option<TokenKind> {
        let (line, col) = self.here();
        let start = self.pos;
        self.bump(); // opening quote
        let mut s = String::new();
        loop {
            match self.peek() {
                None | Some('\n') => {
                    self.diagnostics.push(Diagnostic::error(
                        "E0002",
                        "unterminated string literal",
                        Span::new(start, self.pos, line, col),
                    ).with_help("add a closing `\"`"));
                    break;
                }
                Some('"') => {
                    self.bump();
                    break;
                }
                Some('\\') => {
                    self.bump();
                    match self.bump() {
                        Some('n') => s.push('\n'),
                        Some('t') => s.push('\t'),
                        Some('r') => s.push('\r'),
                        Some('\\') => s.push('\\'),
                        Some('"') => s.push('"'),
                        Some(other) => s.push(other),
                        None => break,
                    }
                }
                Some(c) => {
                    s.push(c);
                    self.bump();
                }
            }
        }
        Some(TokenKind::Str(s))
    }

    fn lex_ident_or_keyword(&mut self) -> Option<TokenKind> {
        let start = self.pos;
        while matches!(self.peek(), Some(c) if is_ident_continue(c)) {
            self.bump();
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        Some(keyword(&text).unwrap_or(TokenKind::Ident(text)))
    }

    fn lex_operator(&mut self) -> Option<TokenKind> {
        use TokenKind::*;
        let (line, col) = self.here();
        let c = self.bump().unwrap();
        let kind = match c {
            '(' => LParen,
            ')' => RParen,
            '{' => LBrace,
            '}' => RBrace,
            '[' => LBracket,
            ']' => RBracket,
            ',' => Comma,
            ':' => Colon,
            ';' => Semicolon,
            '.' => {
                if self.peek() == Some('.') {
                    self.bump();
                    DotDot
                } else {
                    Dot
                }
            }
            '+' => Plus,
            '-' => {
                if self.peek() == Some('>') {
                    self.bump();
                    Arrow
                } else {
                    Minus
                }
            }
            '*' => Star,
            '/' => Slash,
            '%' => Percent,
            '=' => {
                if self.peek() == Some('=') {
                    self.bump();
                    EqEq
                } else {
                    Eq
                }
            }
            '!' => {
                if self.peek() == Some('=') {
                    self.bump();
                    NotEq
                } else {
                    Not
                }
            }
            '<' => {
                if self.peek() == Some('=') {
                    self.bump();
                    LtEq
                } else {
                    Lt
                }
            }
            '>' => {
                if self.peek() == Some('=') {
                    self.bump();
                    GtEq
                } else {
                    Gt
                }
            }
            '&' if self.peek() == Some('&') => {
                self.bump();
                AndAnd
            }
            '|' if self.peek() == Some('|') => {
                self.bump();
                OrOr
            }
            other => {
                self.diagnostics.push(Diagnostic::error(
                    "E0003",
                    format!("unexpected character `{other}`"),
                    Span::new(self.pos - 1, self.pos, line, col),
                ));
                return None;
            }
        };
        Some(kind)
    }
}

fn is_ident_start(c: char) -> bool {
    c == '_' || c.is_alphabetic()
}

fn is_ident_continue(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<TokenKind> {
        let (toks, diags) = Lexer::new(src).tokenize();
        assert!(diags.is_empty(), "unexpected diagnostics: {:?}", diags.into_vec());
        toks.into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn lexes_let_statement() {
        let k = kinds("let x = 42;");
        assert_eq!(
            k,
            vec![
                TokenKind::KwLet,
                TokenKind::Ident("x".into()),
                TokenKind::Eq,
                TokenKind::Int(42),
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn lexes_float_and_operators() {
        let k = kinds("1.5 + 2 * (3 - 4) <= 10 && true");
        assert!(k.contains(&TokenKind::Float(1.5)));
        assert!(k.contains(&TokenKind::AndAnd));
        assert!(k.contains(&TokenKind::LtEq));
    }

    #[test]
    fn lexes_string_with_escapes() {
        let k = kinds("\"hello\\nworld\"");
        assert_eq!(k[0], TokenKind::Str("hello\nworld".to_string()));
    }

    #[test]
    fn reports_unterminated_string() {
        let (_, diags) = Lexer::new("\"oops").tokenize();
        assert!(diags.has_errors());
    }

    #[test]
    fn reports_unexpected_character() {
        let (_, diags) = Lexer::new("let x = 5 @ 3;").tokenize();
        assert!(diags.has_errors());
    }

    #[test]
    fn skips_comments() {
        let k = kinds("// comment\nlet x = 1; /* block */");
        assert_eq!(k[0], TokenKind::KwLet);
    }
}
