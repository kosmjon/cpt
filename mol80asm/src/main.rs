//! CLI entry point for mol80asm.
//!
//! Usage:
//!   mol80asm <file.mol>              Assemble and print a hex listing of the output bytes.
//!   mol80asm --tokens <file.mol>     Print the token stream.
//!   mol80asm --ast <file.mol>        Print the parsed AST.
//!   mol80asm --bin <out> <file.mol>  Assemble and write a raw binary image to <out>.
//!
//! Diagnostics are reported with file/line/column; a non-zero exit code indicates errors and
//! no binary is written on failure (Req 6.4).

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let result = match args.as_slice() {
        [flag, path] if flag == "--tokens" => run_tokens(path),
        [flag, path] if flag == "--ast" => run_ast(path),
        [flag, path] if flag == "--asm" => run_asm(path),
        [flag, out, path] if flag == "--bin" => run_bin(path, out),
        [path] => run_listing(path),
        _ => {
            eprintln!("usage: mol80asm [--tokens|--ast|--asm|--bin <out>] <file.mol>");
            return ExitCode::from(2);
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(()) => ExitCode::FAILURE,
    }
}

fn read(path: &str) -> Result<String, ()> {
    std::fs::read_to_string(path).map_err(|e| {
        eprintln!("error: cannot read {}: {}", path, e);
    })
}

fn run_tokens(path: &str) -> Result<(), ()> {
    let src = read(path)?;
    let (tokens, diags) = mol80asm::tokenize_source(path, &src);
    for t in &tokens {
        println!("{}:{}  {:?}", t.span.line, t.span.col, t.kind);
    }
    report(&diags)
}

fn run_ast(path: &str) -> Result<(), ()> {
    let src = read(path)?;
    let out = mol80asm::parse_source(path, &src);
    for line in &out.program.lines {
        println!("{:?}", line);
    }
    report(&out.diags)
}

fn run_asm(path: &str) -> Result<(), ()> {
    let src = read(path)?;
    let out = mol80asm::transpile_source(path, &src);
    if let Some(asm) = &out.asm {
        print!("{}", asm);
    }
    report(&out.diags)
}

fn run_listing(path: &str) -> Result<(), ()> {
    let src = read(path)?;
    let out = mol80asm::assemble_source(path, &src);
    if let Some(asm) = &out.assembly {
        print_hex(asm.origin, &asm.bytes);
    }
    report(&out.diags)
}

fn run_bin(path: &str, out_path: &str) -> Result<(), ()> {
    let src = read(path)?;
    let out = mol80asm::assemble_source(path, &src);
    match &out.assembly {
        Some(asm) => {
            if let Err(e) = std::fs::write(out_path, &asm.bytes) {
                eprintln!("error: cannot write {}: {}", out_path, e);
                return Err(());
            }
            eprintln!("wrote {} bytes to {}", asm.bytes.len(), out_path);
            report(&out.diags)
        }
        None => {
            // Assembly failed: report and write nothing (Req 6.4).
            let _ = report(&out.diags);
            Err(())
        }
    }
}

/// Print a simple address + hex-byte listing.
fn print_hex(origin: u16, bytes: &[u8]) {
    for (i, chunk) in bytes.chunks(16).enumerate() {
        let addr = origin as usize + i * 16;
        let hex: Vec<String> = chunk.iter().map(|b| format!("{:02X}", b)).collect();
        println!("{:04X}:  {}", addr, hex.join(" "));
    }
    println!("({} bytes)", bytes.len());
}

fn report(diags: &mol80asm::diag::Diagnostics) -> Result<(), ()> {
    if !diags.is_empty() {
        eprint!("{}", diags.render());
        eprintln!("{} diagnostic(s)", diags.len());
    }
    if diags.has_errors() {
        Err(())
    } else {
        Ok(())
    }
}
