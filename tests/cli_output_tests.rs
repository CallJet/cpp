//! CLI 출력 포맷팅 및 결과 시맨틱 통합 테스트
//! Integration tests for CLI output formatting and result semantics

use std::fs;
use tempfile::tempdir;

use calljet::cli::ProjectInput;
use calljet::model::{QueryRequest, SymbolQuery};
use calljet::project::ProjectContext;
use calljet::query::QueryEngine;
use calljet::render::HumanRenderer;
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
    assert!(rendered.stdout.contains("[CONFIRMED]"));
    assert!(rendered.stdout.contains("caller -> target"));
    assert!(rendered.stdout.contains("상태: 분석 완료"));

    // 2. explain 렌더링 검증
    let explain_res = engine
        .execute(QueryRequest::Explain {
            caller: SymbolQuery::parse("caller"),
            callee: SymbolQuery::parse("target"),
        })
        .unwrap();

    let rendered_explain = renderer.render(&project, &explain_res);
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
