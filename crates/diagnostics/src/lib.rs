//! nexus-diagnostics
//!
//! Source spans, diagnostics (errors/warnings/notes) and a human-friendly,
//! colored diagnostic renderer used by every stage of the NEXUS compiler
//! (lexer, parser, type checker, interpreter).
//!
//! The design goal is a rustc-like experience: every diagnostic carries a
//! precise [`Span`], a short headline message, an optional `help` note, and
//! optional secondary/related spans. Diagnostics are collected into a
//! [`DiagnosticBag`] rather than aborting on the first error, so that a
//! single `nexus check` invocation can surface many problems at once.

/// Byte-offset + line/column location range inside a single source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub col: usize,
}

impl Span {
    pub fn new(start: usize, end: usize, line: usize, col: usize) -> Self {
        Self { start, end, line, col }
    }

    /// Produces a span that covers both `self` and `other`.
    pub fn to(&self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
            line: self.line,
            col: self.col,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Note,
}

impl Severity {
    fn label(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Note => "note",
        }
    }

    fn color(self) -> &'static str {
        match self {
            Severity::Error => "\x1b[1;31m",   // bold red
            Severity::Warning => "\x1b[1;33m", // bold yellow
            Severity::Note => "\x1b[1;36m",    // bold cyan
        }
    }
}

/// A single secondary annotation attached to a diagnostic (e.g. "previous
/// definition was here").
#[derive(Debug, Clone)]
pub struct Related {
    pub span: Span,
    pub message: String,
}

/// A single compiler diagnostic: an error, warning or note with a precise
/// source location, an optional actionable `help` suggestion, and any number
/// of related secondary spans.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub span: Option<Span>,
    pub help: Option<String>,
    pub related: Vec<Related>,
}

impl Diagnostic {
    pub fn error(code: &'static str, message: impl Into<String>, span: Span) -> Self {
        Self {
            severity: Severity::Error,
            code,
            message: message.into(),
            span: Some(span),
            help: None,
            related: Vec::new(),
        }
    }

    pub fn warning(code: &'static str, message: impl Into<String>, span: Span) -> Self {
        Self {
            severity: Severity::Warning,
            code,
            message: message.into(),
            span: Some(span),
            help: None,
            related: Vec::new(),
        }
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn with_related(mut self, span: Span, message: impl Into<String>) -> Self {
        self.related.push(Related { span, message: message.into() });
        self
    }
}

/// Collects diagnostics produced while compiling a single source file.
#[derive(Debug, Default, Clone)]
pub struct DiagnosticBag {
    diagnostics: Vec<Diagnostic>,
}

impl DiagnosticBag {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, diag: Diagnostic) {
        self.diagnostics.push(diag);
    }

    pub fn extend(&mut self, other: DiagnosticBag) {
        self.diagnostics.extend(other.diagnostics);
    }

    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == Severity::Error)
    }

    pub fn error_count(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.severity == Severity::Error).count()
    }

    pub fn warning_count(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.severity == Severity::Warning).count()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics.iter()
    }

    pub fn into_vec(self) -> Vec<Diagnostic> {
        self.diagnostics
    }

    /// Renders every collected diagnostic against `source`, in the classic
    /// "file:line:col: error[E001]: message" + source-context style.
    pub fn render(&self, file_name: &str, source: &str) -> String {
        let lines: Vec<&str> = source.lines().collect();
        let mut out = String::new();
        for diag in &self.diagnostics {
            render_impl(&mut out, file_name, &lines, diag);
        }
        if !self.diagnostics.is_empty() {
            out.push_str(&format!(
                "\n{}{} error(s), {} warning(s){}\n",
                "\x1b[1m",
                self.error_count(),
                self.warning_count(),
                RESET
            ));
        }
        out
    }
}

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const BLUE: &str = "\x1b[1;34m";

fn render_impl(out: &mut String, file_name: &str, lines: &[&str], diag: &Diagnostic) {
    let sev = diag.severity;
    out.push_str(&format!(
        "{color}{label}{reset}{bold}[{code}]: {msg}{reset}\n",
        color = sev.color(),
        label = sev.label(),
        code = diag.code,
        msg = diag.message,
        bold = BOLD,
        reset = RESET
    ));

    if let Some(span) = diag.span {
        out.push_str(&format!(
            "{blue}  -->{reset} {file}:{line}:{col}\n",
            blue = BLUE,
            reset = RESET,
            file = file_name,
            line = span.line,
            col = span.col
        ));
        push_source_context(out, lines, span, sev.color());
    }

    for rel in &diag.related {
        out.push_str(&format!(
            "{blue}  note:{reset} {msg} ({file}:{line}:{col})\n",
            blue = BLUE,
            reset = RESET,
            msg = rel.message,
            file = file_name,
            line = rel.span.line,
            col = rel.span.col
        ));
    }

    if let Some(help) = &diag.help {
        out.push_str(&format!("{dim}  = help:{reset} {help}\n", dim = DIM, reset = RESET, help = help));
    }
    out.push('\n');
}

fn push_source_context(out: &mut String, lines: &[&str], span: Span, color: &str) {
    if span.line == 0 || span.line > lines.len() {
        return;
    }
    let line_str = lines[span.line - 1];
    let gutter = format!("{}", span.line);
    let pad = " ".repeat(gutter.len());
    out.push_str(&format!("{dim}{pad} |{reset}\n", dim = DIM, pad = pad, reset = RESET));
    out.push_str(&format!(
        "{dim}{gutter} |{reset} {line}\n",
        dim = DIM,
        gutter = gutter,
        reset = RESET,
        line = line_str
    ));
    let width = span.end.saturating_sub(span.start).max(1).min(line_str.len().saturating_sub(span.col.saturating_sub(1)).max(1));
    let caret_pad = " ".repeat(span.col.saturating_sub(1));
    let carets = "^".repeat(width.max(1));
    out.push_str(&format!(
        "{dim}{pad} |{reset} {caret_pad}{color}{carets}{reset}\n",
        dim = DIM,
        pad = pad,
        reset = RESET,
        caret_pad = caret_pad,
        color = color,
        carets = carets
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_merge() {
        let a = Span::new(0, 3, 1, 1);
        let b = Span::new(5, 8, 1, 6);
        let c = a.to(b);
        assert_eq!(c.start, 0);
        assert_eq!(c.end, 8);
    }

    #[test]
    fn bag_counts_errors_and_warnings() {
        let mut bag = DiagnosticBag::new();
        bag.push(Diagnostic::error("E001", "bad thing", Span::new(0, 1, 1, 1)));
        bag.push(Diagnostic::warning("W001", "meh", Span::new(0, 1, 1, 1)));
        assert!(bag.has_errors());
        assert_eq!(bag.error_count(), 1);
        assert_eq!(bag.warning_count(), 1);
    }
}
