//! CallJet CLI 메인 진입점
//! CallJet CLI main entry point

use clap::Parser;
use std::process;

use calljet::cli::Cli;
use calljet::project::ProjectContext;
use calljet::query::QueryEngine;
use calljet::render::HumanRenderer;
use calljet::semantic::clang::ClangProvider;

fn main() {
    let cli = Cli::parse();

    let (input, request, render_options) = match cli.into_execution_plan() {
        Ok(req) => req,
        Err(err) => {
            eprintln!("입력 오류: {err}");
            process::exit(1);
        }
    };

    let project = match ProjectContext::load(input) {
        Ok(p) => p,
        Err(err) => {
            eprintln!("프로젝트 초기화 오류: {err}");
            process::exit(1);
        }
    };

    let provider = ClangProvider::new();
    let mut engine = QueryEngine::new(&project, provider);

    let result = match engine.execute(request) {
        Ok(res) => res,
        Err(err) => {
            eprintln!("쿼리 실행 실패: {err}");
            process::exit(1);
        }
    };

    let output_file = render_options.output_file.clone();
    let renderer = HumanRenderer::new();
    let rendered = renderer.render_with_options(&project, &result, render_options);

    if let Some(path) = output_file {
        if let Err(e) = std::fs::write(&path, &rendered.stdout) {
            eprintln!("출력 파일 작성 실패 ('{}'): {e}", path.display());
            process::exit(1);
        }
    } else if !rendered.stdout.is_empty() {
        print!("{}", rendered.stdout);
    }

    if !rendered.stderr.is_empty() {
        eprint!("{}", rendered.stderr);
    }

    process::exit(rendered.exit_code);
}
