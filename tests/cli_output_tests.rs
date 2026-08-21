//! CLI 출력 포맷팅 및 결과 시맨틱 통합 테스트
//! Integration tests for CLI output formatting and result semantics

use std::fs;
use tempfile::tempdir;

use calljet::cli::ProjectInput;
use calljet::model::{QueryRequest, SymbolQuery};
use calljet::project::ProjectContext;
use calljet::query::QueryEngine;
use calljet::render::{HumanRenderer, RenderOptions};
use calljet::semantic::clang::{ensure_libclang_loaded, ClangProvider};

#[test]
fn test_cli_output_renderer_callers_and_explain() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let src_file = root.join("app.cpp");
    let code = r#"
        void target() {}
        void caller() { target(); }
    "#;
    fs::write(&src_file, code).unwrap();

    let db_path = root.join("compile_commands.json");
    fs::write(
        &db_path,
        serde_json::json!([
            {
                "directory": root.to_str().unwrap(),
                "file": "app.cpp",
                "command": "clang++ -c app.cpp"
            }
        ])
        .to_string(),
    )
    .unwrap();

    let project = ProjectContext::load(ProjectInput {
        source_root: root.to_path_buf(),
        compile_commands_path: db_path,
    })
    .unwrap();

    if !ensure_libclang_loaded() {
        return;
    }

    let provider = ClangProvider::new();
    let mut engine = QueryEngine::new(&project, provider);
    let renderer = HumanRenderer::new();

    // 1. callers 렌더링 검증
    let res = engine
        .execute(QueryRequest::Callers {
            target: SymbolQuery::parse("target"),
            max_depth: None,
            verified_only: false,
        })
        .unwrap();

    let rendered = renderer.render(&project, &res);
    assert_eq!(
        rendered.exit_code, 0,
        "성공적인 callers 쿼리는 exit code 0이어야 함"
    );
    assert_eq!(rendered.stdout, "caller\ntarget\n");
    assert!(!rendered.stdout.contains("->"));
    assert!(!rendered.stdout.contains("[CONFIRMED]"));
    assert!(!rendered.stdout.contains("상태: 분석 완료"));
    assert!(!rendered.stdout.contains("Directory :"));
    assert!(!rendered.stdout.contains("app.cpp"));

    // 2. explain 렌더링 검증
    let explain_res = engine
        .execute(QueryRequest::Explain {
            caller: SymbolQuery::parse("caller"),
            callee: SymbolQuery::parse("target"),
        })
        .unwrap();

    let rendered_explain = renderer.render_with_options(
        &project,
        &explain_res,
        RenderOptions {
            verbosity: 2,
            ..RenderOptions::default()
        },
    );
    assert_eq!(rendered_explain.exit_code, 0);
    assert!(rendered_explain.stdout.contains("표현식: `target()`"));
    assert!(rendered_explain.stdout.contains("사유: ExactReference"));
}

#[test]
fn test_cli_exit_code_on_truncated_and_no_result() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let src_file = root.join("empty.cpp");
    fs::write(&src_file, "void foo() {}").unwrap();

    let db_path = root.join("compile_commands.json");
    fs::write(
        &db_path,
        serde_json::json!([
            {
                "directory": root.to_str().unwrap(),
                "file": "empty.cpp",
                "command": "clang++ -c empty.cpp"
            }
        ])
        .to_string(),
    )
    .unwrap();

    let project = ProjectContext::load(ProjectInput {
        source_root: root.to_path_buf(),
        compile_commands_path: db_path,
    })
    .unwrap();

    if !ensure_libclang_loaded() {
        return;
    }

    let provider = ClangProvider::new();
    let mut engine = QueryEngine::new(&project, provider);
    let renderer = HumanRenderer::new();

    // callees of foo (결과 없음 -> NoResult)
    let res_no_result = engine
        .execute(QueryRequest::Callees {
            source: SymbolQuery::parse("foo"),
            max_depth: None,
            verified_only: false,
        })
        .unwrap();

    let rendered_no_result = renderer.render(&project, &res_no_result);
    assert_eq!(
        rendered_no_result.exit_code, 0,
        "NoResult는 정상 종료(exit code 0)이어야 함 (FR-070)"
    );
    assert!(rendered_no_result.stdout.contains("결과 없음 (No Result)"));
}

#[test]
fn test_text_output_separates_directory_namespace_class_and_function() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();
    let src_file = src_dir.join("member.cpp");
    fs::write(
        &src_file,
        r#"
        namespace App {
            class Service {
            public:
                void target() {}
            };

            class Controller {
            public:
                void run(Service& service) {
                    service.target();
                }
            };
        }
        "#,
    )
    .unwrap();

    let db_path = root.join("compile_commands.json");
    fs::write(
        &db_path,
        serde_json::json!([{
            "directory": root.to_str().unwrap(),
            "file": "src/member.cpp",
            "command": "clang++ -std=c++17 -c src/member.cpp"
        }])
        .to_string(),
    )
    .unwrap();

    let project = ProjectContext::load(ProjectInput {
        source_root: root.to_path_buf(),
        compile_commands_path: db_path,
    })
    .unwrap();
    if !ensure_libclang_loaded() {
        return;
    }

    let provider = ClangProvider::new();
    let mut engine = QueryEngine::new(&project, provider);
    let result = engine
        .execute(QueryRequest::Callees {
            source: SymbolQuery::parse("App::Controller::run"),
            max_depth: Some(1),
            verified_only: false,
        })
        .unwrap();
    let renderer = HumanRenderer::new();
    let compact = renderer.render(&project, &result);

    assert_eq!(
        compact.stdout,
        "App::Controller::run\nApp::Service::target\n"
    );
    assert!(!compact.stdout.contains("->"));
    assert!(!compact.stdout.contains("Directory :"));
    assert!(!compact.stdout.contains("member.cpp"));

    let trace_result = engine
        .execute(QueryRequest::Trace {
            target: SymbolQuery::parse("App::Service::target"),
            max_depth: None,
            verified_only: false,
        })
        .unwrap();
    let compact_trace = renderer.render(&project, &trace_result);
    assert_eq!(
        compact_trace.stdout,
        "App::Controller::run\nApp::Service::target\n"
    );
    assert!(!compact_trace.stdout.contains("->"));
    assert!(!compact_trace.stdout.contains("member.cpp"));

    let rendered = renderer.render_with_options(
        &project,
        &result,
        RenderOptions {
            verbosity: 1,
            ..RenderOptions::default()
        },
    );

    assert!(rendered.stdout.contains(
        "Relation #1: App::Controller::run -> App::Service::target [CONFIRMED]"
    ));
    assert!(rendered.stdout.contains("Directory : src"));
    assert!(rendered.stdout.contains("FullSymbol: App::Controller::run"));
    assert!(rendered.stdout.contains("Namespace : App"));
    assert!(rendered.stdout.contains("Class     : Controller"));
    assert!(rendered.stdout.contains("Function  : run"));
    assert!(rendered.stdout.contains("FullSymbol: App::Service::target"));
    assert!(rendered.stdout.contains("Class     : Service"));
    assert!(!rendered.stdout.contains("표현식:"));
    assert!(!rendered.stdout.contains("[UNRESOLVED]"));

    let very_verbose = renderer.render_with_options(
        &project,
        &result,
        RenderOptions {
            verbosity: 2,
            ..RenderOptions::default()
        },
    );
    assert!(very_verbose.stdout.contains("표현식: `service.target()`"));
    assert!(very_verbose.stdout.contains("사유: ExactReference"));
    assert!(very_verbose.stdout.contains("Contexts"));
    assert!(very_verbose.stdout.contains("Evidence"));
    assert!(very_verbose.stdout.contains("Spelling: src/member.cpp"));
    assert!(very_verbose.stdout.contains("[상세 번역 단위(TU) 리포트]"));
    assert!(very_verbose.stdout.contains("[성능 및 비용 지표"));
}
