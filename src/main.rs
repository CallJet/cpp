//! CallJet CLI 메인 진입점
//! CallJet CLI main entry point

use clap::Parser;
use std::process;

use calljet::cli::Cli;
use calljet::console::{missing_compilation_database_help, CALLJET_ASCII_ART};
use calljet::diagnostic::InputError;
use calljet::project::ProjectContext;
use calljet::query::QueryEngine;
use calljet::render::HumanRenderer;
use calljet::semantic::clang::ClangProvider;

fn main() {
    eprint!("{CALLJET_ASCII_ART}");

    let cli = Cli::parse();

    let (input, request, render_options) = match cli.into_execution_plan() {
        Ok(req) => req,
        Err(err) => {
            eprintln!("[CallJet] input error: {err}");
            process::exit(1);
        }
    };

    eprintln!("[CallJet] source root: {}", input.source_root.display());
    eprintln!(
        "[CallJet] compilation database: {}",
        input.compile_commands_path.display()
    );
    eprintln!("[CallJet] loading project...");

    let project = match ProjectContext::load(input) {
        Ok(p) => p,
        Err(err) => {
            eprintln!("[CallJet] project initialization failed: {err}");
            if let InputError::InvalidCompilationDatabase { path, .. } = &err {
                if !path.exists() {
                    eprintln!();
                    eprintln!("{}", missing_compilation_database_help(path));
                }
            }
            process::exit(1);
        }
    };

    eprintln!(
        "[CallJet] project loaded: {} compilation unit(s)",
        project.compilation_db.all_source_files().len()
    );
    eprintln!("[CallJet] discovering candidates...");

    let provider = ClangProvider::new();
    let mut engine = QueryEngine::new(&project, provider);

    eprintln!("[CallJet] executing query...");
    let result = match engine.execute(request) {
        Ok(res) => res,
        Err(err) => {
            eprintln!("[CallJet] query failed: {err}");
            process::exit(1);
        }
    };

    let output_file = render_options.output_file.clone();
    let renderer = HumanRenderer::new();
    let rendered = renderer.render_with_options(&project, &result, render_options);

    if let Some(path) = output_file {
        if let Err(e) = std::fs::write(&path, &rendered.stdout) {
            eprintln!("[CallJet] failed to write output ('{}'): {e}", path.display());
            process::exit(1);
        }
    } else if !rendered.stdout.is_empty() {
        print!("{}", rendered.stdout);
    }

    if !rendered.stderr.is_empty() {
        eprint!("{}", rendered.stderr);
    }

    eprintln!("[CallJet] finished with exit code {}", rendered.exit_code);
    process::exit(rendered.exit_code);
}
