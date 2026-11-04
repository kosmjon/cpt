//! Diagnostics: collectible errors/warnings with source positions.
//!
//! The assembler collects diagnostics rather than panicking on the first error, so a single
//! run can report as many problems as possible (Req 7.2).

use crate::token::Span;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Error => write!(f, "error"),
            Severity::Warning => write!(f, "warning"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub span: Span,
    /// The offending source text, if available.
    pub snippet: Option<String>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>, span: Span) -> Self {
        Diagnostic {
            severity: Severity::Error,
            message: message.into(),
            span,
            snippet: None,
        }
    }

    pub fn with_snippet(mut self, snippet: impl Into<String>) -> Self {
        self.snippet = Some(snippet.into());
        self
    }
}

/// A sink that accumulates diagnostics during lexing/parsing/codegen.
#[derive(Debug, Default)]
pub struct Diagnostics {
    items: Vec<Diagnostic>,
    file: String,
}

impl Diagnostics {
    pub fn new(file: impl Into<String>) -> Self {
        Diagnostics {
            items: Vec::new(),
            file: file.into(),
        }
    }

    pub fn push(&mut self, d: Diagnostic) {
        self.items.push(d);
    }

    pub fn error(&mut self, message: impl Into<String>, span: Span) {
        self.push(Diagnostic::error(message, span));
    }

    pub fn has_errors(&self) -> bool {
        self.items.iter().any(|d| d.severity == Severity::Error)
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn items(&self) -> &[Diagnostic] {
        &self.items
    }

    /// Render all diagnostics as `file:line:col: severity: message` lines.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for d in &self.items {
            out.push_str(&format!(
                "{}:{}:{}: {}: {}",
                self.file, d.span.line, d.span.col, d.severity, d.message
            ));
            if let Some(s) = &d.snippet {
                out.push_str(&format!("  [{}]", s));
            }
            out.push('\n');
        }
        out
    }
}
