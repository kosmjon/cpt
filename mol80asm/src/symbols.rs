//! Symbol table and internal-label allocation for the two-pass assembler.

use crate::diag::{Diagnostic, Diagnostics};
use crate::token::Span;
use std::collections::HashMap;

/// Maps label names to their resolved 16-bit addresses.
#[derive(Debug, Default)]
pub struct SymbolTable {
    map: HashMap<String, u16>,
    /// Where each label was defined, for duplicate diagnostics.
    defined_at: HashMap<String, Span>,
    /// Counter for compiler-generated internal labels (conditional-block skip targets).
    internal_counter: u32,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Define a user label at `addr`. Reports a duplicate-label error (Req 1.4) if it already
    /// exists, keeping the first definition.
    pub fn define(&mut self, name: &str, addr: u16, span: Span, diags: &mut Diagnostics) {
        if let Some(prev) = self.defined_at.get(name) {
            diags.push(
                Diagnostic::error(
                    format!(
                        "duplicate label '{}' (first defined at line {})",
                        name, prev.line
                    ),
                    span,
                )
                .with_snippet(name.to_string()),
            );
            return;
        }
        self.map.insert(name.to_string(), addr);
        self.defined_at.insert(name.to_string(), span);
    }

    /// Look up a label's address.
    pub fn get(&self, name: &str) -> Option<u16> {
        self.map.get(name).copied()
    }

    /// Allocate a fresh internal label name that cannot collide with user identifiers.
    ///
    /// User labels come from the label field (identifier characters only). Internal labels use
    /// a `@` prefix, which the lexer never produces for a user label, guaranteeing no clash
    /// (Req 4.5).
    pub fn fresh_internal(&mut self) -> String {
        let name = format!("@L{:04}", self.internal_counter);
        self.internal_counter += 1;
        name
    }
}
