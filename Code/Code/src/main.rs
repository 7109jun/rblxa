#![cfg_attr(not(windows), allow(dead_code))]

#[cfg(not(windows))]
compile_error!("RBLXA is Windows-only; build this executable on Windows.");

use std::{fs, path::PathBuf, process::ExitCode};
use clap::{Parser, Subcommand};
use rblxa::{build_file, format_file, inspect::{inspect, inspect_rbxl}, parse_file, parse::parse_str, Result};

#[derive(Parser, Debug)]
#[command(name="rblxa", version, about="RBLXA TOML-like Roblox Place compiler")]
struct Cli { #[command(subcommand)] command: Command }

#[derive(Subcommand, Debug)]
enum Command {
    Build { input: PathBuf, #[arg(short, long)] output: Option<PathBuf> },
    Check { input: PathBuf },
    Format { input: PathBuf, #[arg(short, long)] write: bool },
    Inspect { input: PathBuf },
    Version,
}

fn main() -> ExitCode {
    match run() { Ok(()) => ExitCode::SUCCESS, Err(e) => { eprintln!("error: {e}"); ExitCode::FAILURE } }
}

fn run() -> Result<()> {
    let cli=Cli::parse();
    match cli.command {
        Command::Build { input, output } => {
            let out=output.unwrap_or_else(|| input.with_extension("rbxl"));
            build_file(&input,&out)?;
            println!("built {} -> {}", input.display(), out.display());
        }
        Command::Check { input } => {
            let doc = parse_file(&input)?;
            rblxa::script::validate_script_references(&doc, input.parent().unwrap_or(std::path::Path::new(".")))?;
            rblxa::resolver::ResolvedParents::resolve(&doc)?;
            println!("RBLXA: valid\nObjects: {}\nErrors: 0", doc.objects.len());
        }
        Command::Format { input, write } => { let s=format_file(&input,write)?; if !write { print!("{s}"); } else { println!("formatted {}", input.display()); } }
        Command::Inspect { input } => {
            if input.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("rbxl")).unwrap_or(false) {
                print!("{}", inspect_rbxl(&input)?);
            } else {
                let source = fs::read_to_string(&input)?;
                let doc = parse_str(&source, Some(input.clone()))?;
                print!("{}", inspect(&doc)?);
            }
        }
        Command::Version => println!("rblxa 1.0.0"),
    }
    Ok(())
}
