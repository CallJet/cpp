//! 온디맨드 쿼리 및 순회 엔진 단위 및 통합 테스트
//! Unit and integration tests for on-demand query engine

use std::fs;
use tempfile::tempdir;

use calljet::cli::ProjectInput;
use calljet::diagnostic::{FatalError, QueryError};
use calljet::model::{CallKind, Completion, Confidence, QueryRequest, SymbolQuery};
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
    assert_eq!(res.edges.len(), 2, "2-hop 호출자 체인이 모두 탐색되어야 함");

    // 2. trace(leaf) -> root_fn -> mid -> leaf 경로 자동 구성
    let trace_req = QueryRequest::Trace {
        target: SymbolQuery::parse("leaf"),
        max_depth: None,
        verified_only: false,
    };
    let trace_res = engine.execute(trace_req).unwrap();
    assert_eq!(trace_res.completion, Completion::Complete);
    assert_eq!(trace_res.paths.len(), 1);
    assert_eq!(trace_res.paths[0].nodes.len(), 3);
    assert_eq!(trace_res.paths[0].edges.len(), 2);

    // 3. callees(root_fn) -> mid -> leaf
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
    assert_eq!(
        res_callees.edges.len(),
        2,
        "2-hop 피호출자 체인이 모두 탐색되어야 함"
    );

    // 4. path(root_fn, leaf) -> root_fn -> mid -> leaf
    let path_req = QueryRequest::Path {
        source: SymbolQuery::parse("root_fn"),
        target: SymbolQuery::parse("leaf"),
        max_depth: None,
        verified_only: false,
    };
    let path_res = engine.execute(path_req).unwrap();
    assert_eq!(path_res.completion, Completion::Complete);
    assert_eq!(path_res.paths.len(), 1);
    assert_eq!(path_res.paths[0].nodes.len(), 3);
    assert_eq!(path_res.paths[0].edges.len(), 2);
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

#[test]
fn test_query_engine_rejects_inactive_preprocessor_symbol() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let src_file = root.join("conditional.cpp");
    fs::write(
        &src_file,
        r#"
        #ifdef ENABLE_HIDDEN
        void hidden_target() {}
        #endif

        void visible_target() {}
        "#,
    )
    .unwrap();

    let db_path = root.join("compile_commands.json");
    fs::write(
        &db_path,
        serde_json::json!([{
            "directory": root.to_str().unwrap(),
            "file": "conditional.cpp",
            "command": "clang++ -std=c++17 -c conditional.cpp"
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

    let mut engine = QueryEngine::new(&project, ClangProvider::new());
    let error = engine
        .execute(QueryRequest::Callers {
            target: SymbolQuery::parse("hidden_target"),
            max_depth: None,
            verified_only: false,
        })
        .expect_err("비활성 전처리기 분기의 구문 후보를 심볼로 되살리면 안 됨");

    assert!(matches!(
        error,
        FatalError::Query(QueryError::SymbolNotFound { query }) if query == "hidden_target"
    ));

    let visible = engine
        .execute(QueryRequest::Callees {
            source: SymbolQuery::parse("visible_target"),
            max_depth: None,
            verified_only: false,
        })
        .expect("같은 TU의 활성 심볼은 정상 해석되어야 함");
    assert_eq!(visible.completion, Completion::NoResult);
}

#[test]
fn test_virtual_call_and_trace_match_cross_tu_derived_override() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let header_file = root.join("base.hpp");
    fs::write(
        &header_file,
        r#"
        #pragma once

        struct Base {
            virtual void target() {}
        };
        "#,
    )
    .unwrap();

    let derived_file = root.join("derived.cpp");
    fs::write(
        &derived_file,
        r#"
        #include "base.hpp"

        struct Derived : Base {
            void target() override {}
        };
        "#,
    )
    .unwrap();

    let caller_file = root.join("caller.cpp");
    fs::write(
        &caller_file,
        r#"
        #include "base.hpp"

        void invoke(Base& value) {
            value.target();
        }
        "#,
    )
    .unwrap();

    let db_path = root.join("compile_commands.json");
    fs::write(
        &db_path,
        serde_json::json!([
            {
                "directory": root.to_str().unwrap(),
                "file": "caller.cpp",
                "command": "clang++ -std=c++17 -I. -c caller.cpp"
            },
            {
                "directory": root.to_str().unwrap(),
                "file": "derived.cpp",
                "command": "clang++ -std=c++17 -I. -c derived.cpp"
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

    let mut engine = QueryEngine::new(&project, ClangProvider::new());
    let result = engine
        .execute(QueryRequest::Callers {
            target: SymbolQuery::parse("Derived::target"),
            max_depth: None,
            verified_only: false,
        })
        .unwrap();

    assert_eq!(result.completion, Completion::Complete);
    let derived = result
        .symbols
        .values()
        .find(|symbol| symbol.display_name() == "Derived::target")
        .expect("요청한 derived override 심볼이 필요함");
    let edge = result
        .edges
        .iter()
        .find(|edge| edge.kind == CallKind::Virtual)
        .expect("base 참조를 통한 virtual 호출 엣지가 필요함");

    assert_eq!(edge.confidence, Confidence::Possible);
    assert_eq!(edge.callee.as_ref(), Some(&derived.id));

    let candidate_names = edge
        .evidence_by_context
        .values()
        .flat_map(|evidence| evidence.candidate_targets.iter())
        .map(|symbol| symbol.display_name().to_string())
        .collect::<Vec<_>>();
    assert!(candidate_names.iter().any(|name| name == "Base::target"));
    assert!(candidate_names
        .iter()
        .any(|name| name == "Derived::target"));

    let mut trace_engine = QueryEngine::new(&project, ClangProvider::new());
    let trace = trace_engine
        .execute(QueryRequest::Trace {
            target: SymbolQuery::parse("Derived::target"),
            max_depth: None,
            verified_only: false,
        })
        .unwrap();

    assert_eq!(trace.completion, Completion::Complete);
    assert_eq!(trace.paths.len(), 1);
    let path_names = trace.paths[0]
        .nodes
        .iter()
        .map(|id| {
            trace
                .symbols
                .get(id)
                .expect("trace 노드의 심볼 메타데이터가 필요함")
                .display_name()
        })
        .collect::<Vec<_>>();
    assert_eq!(path_names, vec!["invoke", "Derived::target"]);

    let mut path_engine = QueryEngine::new(&project, ClangProvider::new());
    let path = path_engine
        .execute(QueryRequest::Path {
            source: SymbolQuery::parse("invoke"),
            target: SymbolQuery::parse("Derived::target"),
            max_depth: None,
            verified_only: false,
        })
        .unwrap();

    assert_eq!(path.completion, Completion::Complete);
    assert_eq!(path.paths.len(), 1);
    let path_names = path.paths[0]
        .nodes
        .iter()
        .map(|id| {
            path.symbols
                .get(id)
                .expect("path 노드의 심볼 메타데이터가 필요함")
                .display_name()
        })
        .collect::<Vec<_>>();
    assert_eq!(path_names, vec!["invoke", "Derived::target"]);
}
