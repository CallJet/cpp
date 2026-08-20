//! 온디맨드 쿼리 및 순회 엔진 단위 및 통합 테스트
//! Unit and integration tests for on-demand query engine

use std::fs;
use tempfile::tempdir;

use calljet::cli::ProjectInput;
use calljet::model::{Completion, QueryRequest, SymbolQuery};
use calljet::project::ProjectContext;
use calljet::query::QueryEngine;
use calljet::semantic::clang::{ensure_libclang_loaded, ClangProvider};

#[test]
fn test_query_engine_callers_and_callees() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let src_file = root.join("chain.cpp");
    let code = r#"
        void leaf() {}
        void mid() { leaf(); }
        void root_fn() { mid(); }
    "#;
    fs::write(&src_file, code).unwrap();

    let db_path = root.join("compile_commands.json");
    fs::write(
        &db_path,
        serde_json::json!([
            {
                "directory": root.to_str().unwrap(),
                "file": "chain.cpp",
                "command": "clang++ -c chain.cpp"
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

    // 1. callers(leaf) -> mid -> root_fn
    let callers_req = QueryRequest::Callers {
        target: SymbolQuery::parse("leaf"),
        max_depth: None,
        verified_only: false,
    };
    let res = engine.execute(callers_req).unwrap();
    assert_eq!(res.completion, Completion::Complete);
    assert!(
        !res.edges.is_empty(),
        "leaf의 호출자 엣지가 검증 반환되어야 함"
    );

    // 2. callees(root_fn) -> mid -> leaf
    let callees_req = QueryRequest::Callees {
        source: SymbolQuery::parse("root_fn"),
        max_depth: None,
        verified_only: false,
    };
    let res_callees = engine.execute(callees_req).unwrap();
    assert_eq!(res_callees.completion, Completion::Complete);
    assert!(
        !res_callees.edges.is_empty(),
        "root_fn의 피호출자 엣지가 검증 반환되어야 함"
    );
}

#[test]
fn test_query_engine_path_and_cycle_handling() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let src_file = root.join("cycle.cpp");
    let code = r#"
        void b();
        void a() { b(); }
        void b() { a(); }
        void target() { a(); }
    "#;
    fs::write(&src_file, code).unwrap();

    let db_path = root.join("compile_commands.json");
    fs::write(
        &db_path,
        serde_json::json!([
            {
                "directory": root.to_str().unwrap(),
                "file": "cycle.cpp",
                "command": "clang++ -c cycle.cpp"
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

    // 사이클이 존재하는 그래프에서 무한 루프 없이 정상 종료되어야 함 (FR-023, FR-036)
    let callers_cycle = QueryRequest::Callers {
        target: SymbolQuery::parse("a"),
        max_depth: None,
        verified_only: false,
    };
    let res = engine.execute(callers_cycle).unwrap();
    assert_eq!(res.completion, Completion::Complete);
}

#[test]
fn test_query_engine_explain() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let src_file = root.join("explain.cpp");
    let code = r#"
        void target_fn() {}
        void caller_fn() { target_fn(); }
    "#;
    fs::write(&src_file, code).unwrap();

    let db_path = root.join("compile_commands.json");
    fs::write(
        &db_path,
        serde_json::json!([
            {
                "directory": root.to_str().unwrap(),
                "file": "explain.cpp",
                "command": "clang++ -c explain.cpp"
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

    let explain_req = QueryRequest::Explain {
        caller: SymbolQuery::parse("caller_fn"),
        callee: SymbolQuery::parse("target_fn"),
    };
    let res = engine.execute(explain_req).unwrap();
    assert_eq!(res.completion, Completion::Complete);
    assert_eq!(res.edges.len(), 1, "정확히 1개의 엣지가 설명되어야 함");
    assert_eq!(
        res.edges[0].confidence,
        calljet::model::Confidence::Confirmed
    );
}
