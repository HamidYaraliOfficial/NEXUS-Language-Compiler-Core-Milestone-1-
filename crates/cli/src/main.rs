//! `nexus` — the command-line front end for the NEXUS compiler pipeline.
//!
//! Implemented subcommands (each one fully real, none stubbed):
//!   nexus run <file.nex>          lex -> parse -> type-check -> interpret
//!   nexus check <file.nex>        lex -> parse -> type-check, report only
//!   nexus emit-tokens <file.nex>  dump the raw token stream
//!   nexus emit-ast <file.nex>     dump the parsed AST (pretty-printed)
//!   nexus test <file.nex>         run every zero-argument `test_*` function
//!   nexus repl                    interactive read-eval-print loop
//!   nexus help                    usage
//!
//! Deliberately NOT implemented here (see the workspace README roadmap):
//! native/LLVM code generation, `nexus fmt`/`nexus lint`, the package
//! manager, the LSP server and the desktop IDE. Rather than ship those as
//! hollow stubs, this milestone keeps the CLI's surface limited to what is
//! genuinely, fully working end to end.

use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write};
use std::process::ExitCode;

use nexus_ast::{pretty, Item};
use nexus_interpreter::Interpreter;
use nexus_lexer::Lexer;

const RED: &str = "\x1b[1;31m";
const GREEN: &str = "\x1b[1;32m";
const CYAN: &str = "\x1b[1;36m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(|s| s.as_str());
    let rest = if args.is_empty() { &args[..] } else { &args[1..] };

    match cmd {
        Some("run") => cmd_run(rest),
        Some("check") => cmd_check(rest),
        Some("emit-tokens") => cmd_emit_tokens(rest),
        Some("emit-ast") => cmd_emit_ast(rest),
        Some("test") => cmd_test(rest),
        Some("repl") => cmd_repl(),
        Some("help") | Some("--help") | Some("-h") | None => {
            print_help();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("{RED}error{RESET}: unknown command `{other}`\n");
            print_help();
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    println!(
        r#"{CYAN}NEXUS{RESET} — a real, working compiler front end for the NEXUS language (milestone 1)

USAGE:
    nexus <COMMAND> [ARGS]

COMMANDS:
    run <file.nex>          Compile and execute a NEXUS program
    check <file.nex>        Lex, parse and type-check without running
    emit-tokens <file.nex>  Print the raw token stream
    emit-ast <file.nex>     Print the parsed AST
    test <file.nex>         Run every zero-argument `test_*` function
    repl                    Start an interactive session
    help                    Show this message
"#
    );
}

fn read_source(path: &str) -> Result<String, ExitCode> {
    fs::read_to_string(path).map_err(|e| {
        eprintln!("{RED}error{RESET}: could not read `{path}`: {e}");
        ExitCode::FAILURE
    })
}

fn require_arg<'a>(args: &'a [String], usage: &str) -> Result<&'a str, ExitCode> {
    match args.first() {
        Some(a) => Ok(a.as_str()),
        None => {
            eprintln!("{RED}error{RESET}: {usage}");
            Err(ExitCode::FAILURE)
        }
    }
}

// ---- nexus check --------------------------------------------------------

fn cmd_check(args: &[String]) -> ExitCode {
    let path = match require_arg(args, "usage: nexus check <file.nex>") {
        Ok(p) => p,
        Err(code) => return code,
    };
    let source = match read_source(path) {
        Ok(s) => s,
        Err(code) => return code,
    };

    let (program, parse_diags) = nexus_parser::parse(&source);
    if !parse_diags.is_empty() {
        print!("{}", parse_diags.render(path, &source));
    }
    if parse_diags.has_errors() {
        return ExitCode::FAILURE;
    }

    let type_diags = nexus_typeck::check_program(&program);
    if !type_diags.is_empty() {
        print!("{}", type_diags.render(path, &source));
    }
    if type_diags.has_errors() {
        return ExitCode::FAILURE;
    }

    println!("{GREEN}✓{RESET} no errors found in {path}");
    ExitCode::SUCCESS
}

// ---- nexus run -----------------------------------------------------------

fn cmd_run(args: &[String]) -> ExitCode {
    let path = match require_arg(args, "usage: nexus run <file.nex>") {
        Ok(p) => p,
        Err(code) => return code,
    };
    let source = match read_source(path) {
        Ok(s) => s,
        Err(code) => return code,
    };

    let (program, parse_diags) = nexus_parser::parse(&source);
    if !parse_diags.is_empty() {
        print!("{}", parse_diags.render(path, &source));
    }
    if parse_diags.has_errors() {
        return ExitCode::FAILURE;
    }

    let type_diags = nexus_typeck::check_program(&program);
    if !type_diags.is_empty() {
        print!("{}", type_diags.render(path, &source));
    }
    if type_diags.has_errors() {
        return ExitCode::FAILURE;
    }

    let stdout = io::stdout();
    let mut interp = Interpreter::new(&program, move |line: &str| {
        let mut lock = stdout.lock();
        let _ = writeln!(lock, "{line}");
    });

    match interp.run_main() {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!(
                "{RED}runtime error{RESET} at {path}:{}:{}: {}",
                e.span.line, e.span.col, e.message
            );
            ExitCode::FAILURE
        }
    }
}

// ---- nexus emit-tokens -----------------------------------------------------

fn cmd_emit_tokens(args: &[String]) -> ExitCode {
    let path = match require_arg(args, "usage: nexus emit-tokens <file.nex>") {
        Ok(p) => p,
        Err(code) => return code,
    };
    let source = match read_source(path) {
        Ok(s) => s,
        Err(code) => return code,
    };

    let (tokens, diags) = Lexer::new(&source).tokenize();
    for tok in &tokens {
        println!(
            "{DIM}{:>4}:{:<3}{RESET} {}",
            tok.span.line,
            tok.span.col,
            tok.kind.describe()
        );
    }
    if !diags.is_empty() {
        print!("{}", diags.render(path, &source));
    }
    if diags.has_errors() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

// ---- nexus emit-ast --------------------------------------------------------

fn cmd_emit_ast(args: &[String]) -> ExitCode {
    let path = match require_arg(args, "usage: nexus emit-ast <file.nex>") {
        Ok(p) => p,
        Err(code) => return code,
    };
    let source = match read_source(path) {
        Ok(s) => s,
        Err(code) => return code,
    };

    let (program, diags) = nexus_parser::parse(&source);
    print!("{}", pretty::print_program(&program));
    if !diags.is_empty() {
        print!("{}", diags.render(path, &source));
    }
    if diags.has_errors() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

// ---- nexus test ------------------------------------------------------------

fn cmd_test(args: &[String]) -> ExitCode {
    let path = match require_arg(args, "usage: nexus test <file.nex>") {
        Ok(p) => p,
        Err(code) => return code,
    };
    let source = match read_source(path) {
        Ok(s) => s,
        Err(code) => return code,
    };

    let (program, parse_diags) = nexus_parser::parse(&source);
    if !parse_diags.is_empty() {
        print!("{}", parse_diags.render(path, &source));
    }
    if parse_diags.has_errors() {
        return ExitCode::FAILURE;
    }

    let type_diags = nexus_typeck::check_program(&program);
    if !type_diags.is_empty() {
        print!("{}", type_diags.render(path, &source));
    }
    if type_diags.has_errors() {
        return ExitCode::FAILURE;
    }

    let test_names: Vec<&str> = program
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Function(f) if f.name.starts_with("test_") && f.params.is_empty() => Some(f.name.as_str()),
            _ => None,
        })
        .collect();

    if test_names.is_empty() {
        println!("no `test_*` functions found in {path}");
        return ExitCode::SUCCESS;
    }

    let mut passed = 0usize;
    let mut failed = 0usize;
    for name in &test_names {
        print!("test {name} ... ");
        let _ = io::stdout().flush();
        let mut interp = Interpreter::new(&program, |_line: &str| {});
        match interp.run_function(name) {
            Ok(_) => {
                println!("{GREEN}ok{RESET}");
                passed += 1;
            }
            Err(e) => {
                println!("{RED}FAILED{RESET} — {}", e.message);
                failed += 1;
            }
        }
    }

    println!("\n{passed} passed, {failed} failed");
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

// ---- nexus repl -----------------------------------------------------------
//
// The REPL re-evaluates the whole accumulated session from scratch on every
// line (items + all previously accepted statements, wrapped in an implicit
// `main`). Because evaluation is deterministic and the language subset has
// no I/O other than `print`, the first N printed lines of a re-run are
// always identical to the previous run's output — so only the tail past
// `last_output_len` is new, and that is exactly what gets shown. A rejected
// (parse/type/runtime-error) line is never committed to the session.

fn cmd_repl() -> ExitCode {
    println!("{CYAN}NEXUS REPL{RESET} — type an expression or statement, or `:quit` to exit.");
    let mut items: Vec<String> = Vec::new();
    let mut stmts: Vec<String> = Vec::new();
    let mut last_output_len = 0usize;

    loop {
        print!("nexus> ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        if io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
            println!();
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == ":quit" || trimmed == ":q" {
            break;
        }

        if is_item_start(trimmed) {
            let mut candidate_items = items.clone();
            candidate_items.push(trimmed.to_string());
            let source = build_session_source(&candidate_items, &stmts);
            match check_session(&source) {
                Ok(()) => items = candidate_items,
                Err(msg) => print!("{msg}"),
            }
            continue;
        }

        let stmt_line = prepare_statement(trimmed);
        let mut candidate_stmts = stmts.clone();
        candidate_stmts.push(stmt_line);
        let source = build_session_source(&items, &candidate_stmts);

        match run_session(&source) {
            Ok(output) => {
                for l in &output[last_output_len.min(output.len())..] {
                    println!("{l}");
                }
                last_output_len = output.len();
                stmts = candidate_stmts;
            }
            Err(msg) => print!("{msg}"),
        }
    }
    ExitCode::SUCCESS
}

fn is_item_start(line: &str) -> bool {
    starts_with_word(line, "fn") || starts_with_word(line, "struct")
}

fn starts_with_word(s: &str, word: &str) -> bool {
    s.strip_prefix(word).is_some_and(|rest| rest.is_empty() || !rest.chars().next().unwrap().is_alphanumeric())
}

const STMT_KEYWORDS: &[&str] = &["let", "return", "if", "while", "for", "break", "continue"];

fn prepare_statement(line: &str) -> String {
    if line.starts_with('{') {
        return line.to_string();
    }
    if STMT_KEYWORDS.iter().any(|kw| starts_with_word(line, kw)) {
        if line.ends_with('}') || line.ends_with(';') {
            line.to_string()
        } else {
            format!("{line};")
        }
    } else {
        // Bare expression: auto-print its value, REPL-style.
        let expr = line.trim_end_matches(';').trim();
        format!("print({expr});")
    }
}

fn build_session_source(items: &[String], stmts: &[String]) -> String {
    let mut out = String::new();
    for item in items {
        let _ = writeln!(out, "{item}");
    }
    let _ = writeln!(out, "fn main() -> unit {{");
    for stmt in stmts {
        let _ = writeln!(out, "    {stmt}");
    }
    let _ = writeln!(out, "}}");
    out
}

fn check_session(source: &str) -> Result<(), String> {
    let (program, parse_diags) = nexus_parser::parse(source);
    if parse_diags.has_errors() {
        return Err(parse_diags.render("<repl>", source));
    }
    let type_diags = nexus_typeck::check_program(&program);
    if type_diags.has_errors() {
        return Err(type_diags.render("<repl>", source));
    }
    Ok(())
}

fn run_session(source: &str) -> Result<Vec<String>, String> {
    let (program, parse_diags) = nexus_parser::parse(source);
    if parse_diags.has_errors() {
        return Err(parse_diags.render("<repl>", source));
    }
    let type_diags = nexus_typeck::check_program(&program);
    if type_diags.has_errors() {
        return Err(type_diags.render("<repl>", source));
    }

    let mut output = Vec::new();
    {
        let mut interp = Interpreter::new(&program, |line: &str| output.push(line.to_string()));
        if let Err(e) = interp.run_main() {
            return Err(format!("{RED}runtime error{RESET}: {} (line {})\n", e.message, e.span.line));
        }
    }
    Ok(output)
}
