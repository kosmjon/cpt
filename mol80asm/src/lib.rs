//! mol80asm — assembler/transpiler for CPT Corporation's MOL80.
//!
//! This early milestone implements the front end: a column-aware lexer and a recursive-descent
//! parser producing an AST for the confirmed subset of the MOL80 grammar. Code generation, the
//! ISA table, label resolution, and output emitters are future work tracked in the spec.

pub mod ast;
pub mod codegen;
pub mod diag;
pub mod emit;
pub mod isa;
pub mod lexer;
pub mod parser;
pub mod symbols;
pub mod token;

use ast::Program;
use codegen::Assembly;
use diag::Diagnostics;
use lexer::Lexer;
use parser::Parser;

pub use codegen::Relocation;

/// Result of a front-end run: the parsed program and any diagnostics collected.
pub struct ParseOutput {
    pub program: Program,
    pub diags: Diagnostics,
}

/// Lex and parse MOL80 source into an AST, collecting diagnostics (never panics on bad input).
pub fn parse_source(file: &str, src: &str) -> ParseOutput {
    let mut diags = Diagnostics::new(file);
    let tokens = Lexer::new(&mut diags).tokenize(src);
    let program = Parser::new(tokens, &mut diags).parse_program();
    ParseOutput { program, diags }
}

/// Result of a full assembly: the emitted bytes (if successful) and diagnostics.
pub struct AssembleOutput {
    pub assembly: Option<Assembly>,
    pub diags: Diagnostics,
}

/// Lex, parse, and assemble MOL80 source into 8080 bytes.
pub fn assemble_source(file: &str, src: &str) -> AssembleOutput {
    let mut diags = Diagnostics::new(file);
    let tokens = Lexer::new(&mut diags).tokenize(src);
    let program = Parser::new(tokens, &mut diags).parse_program();
    let assembly = if diags.has_errors() {
        None
    } else {
        codegen::assemble(&program, &mut diags)
    };
    AssembleOutput { assembly, diags }
}

/// Result of transpilation: standard 8080 assembly text (if parsing succeeded) + diagnostics.
pub struct TranspileOutput {
    pub asm: Option<String>,
    pub diags: Diagnostics,
}

/// Lex, parse, and transpile MOL80 source to standard Intel 8080 assembly text.
pub fn transpile_source(file: &str, src: &str) -> TranspileOutput {
    let mut diags = Diagnostics::new(file);
    let tokens = Lexer::new(&mut diags).tokenize(src);
    let program = Parser::new(tokens, &mut diags).parse_program();
    let asm = if diags.has_errors() {
        None
    } else {
        Some(emit::transpile(&program))
    };
    TranspileOutput { asm, diags }
}

/// Convenience: just the token stream (used by tests and the `--tokens` CLI mode).
pub fn tokenize_source(file: &str, src: &str) -> (Vec<token::Token>, Diagnostics) {
    let mut diags = Diagnostics::new(file);
    let tokens = Lexer::new(&mut diags).tokenize(src);
    (tokens, diags)
}
