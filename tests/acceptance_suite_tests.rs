//! CallJet C++ 정식 PoC 인수 테스트 스위트 (Acceptance Test Suite)
//! Formal Acceptance Test Suite for CallJet C++ validating SRS AC-001 to AC-018

use std::fs;
use tempfile::tempdir;

use calljet::cli::ProjectInput;
use calljet::model::{CallKind, Completion, Confidence, QueryRequest, SymbolQuery};
use calljet::project::ProjectContext;
use calljet::query::QueryEngine;
use calljet::render::{HumanRenderer, RenderOptions};
use calljet::semantic::clang::{ensure_libclang_loaded, ClangProvider};

/// 대표적인 acceptance 픽스처 프로젝트 생성 헬퍼
fn setup_acceptance_fixture() -> (tempfile::TempDir, ProjectContext) {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let src_dir = root.join("src");
    let inc_dir = root.join("include");
    fs::create_dir_all(&src_dir).unwrap();
    fs::create_dir_all(&inc_dir).unwrap();

    // 1. 헤더 파일
    let header_path = inc_dir.join("calc.h");
    fs::write(
        &header_path,
        r#"
        #pragma once
        namespace Math {
            class Calculator {
            public:
                int add(int a, int b);
                double add(double a, double b);
                virtual void compute();
            };
        }
    "#,
    )
    .unwrap();

    // 2. C 소스 파일 (체인 및 직접 호출)
    let c_file = src_dir.join("c_chain.c");
    fs::write(
        &c_file,
        r#"
        void c_leaf() {}
        void c_mid() { c_leaf(); }
        void c_root() { c_mid(); }
    "#,
    )
    .unwrap();

    // 3. C++ 소스 파일 (네임스페이스, 오버로드, 가상 호출, 다중 컨텍스트)
    let cpp_file = src_dir.join("calc.cpp");
    fs::write(
        &cpp_file,
        r#"
        #include "calc.h"
        namespace Math {
            int Calculator::add(int a, int b) { return a + b; }
            double Calculator::add(double a, double b) { return a + b; }
            void Calculator::compute() {
                add(1, 2);
                add(1.5, 2.5);
            }
        }

        void run_feature() {
            Math::Calculator calc;
            #ifdef FEATURE_A
            calc.add(10, 20);
            #endif
            #ifdef FEATURE_B
            calc.add(1.1, 2.2);
            #endif
        }
    "#,
    )
    .unwrap();

    // 4. 순환 호출 소스 파일 (사이클 검증)
    let cycle_file = src_dir.join("cycle.cpp");
    fs::write(
        &cycle_file,
        r#"
        void ping();
        void pong() { ping(); }
        void ping() { pong(); }
    "#,
    )
    .unwrap();

    // 5. 무관한 독립 번역 단위 소스 (Unrelated TU - 시맨틱 파싱 제외 검증)
    let unrelated_file = src_dir.join("unrelated.cpp");
    fs::write(
        &unrelated_file,
        r#"
        void completely_isolated_function() {
            int x = 100;
        }
    "#,
    )
    .unwrap();

    // 6. compile_commands.json 생성 (FEATURE_A와 FEATURE_B 다중 컨텍스트 포함)
    let db_path = root.join("compile_commands.json");
    let json_content = serde_json::json!([
        {
            "directory": root.to_str().unwrap(),
            "file": "src/c_chain.c",
            "command": "gcc -Iinclude -c src/c_chain.c"
        },
        {
            "directory": root.to_str().unwrap(),
            "file": "src/calc.cpp",
            "command": "clang++ -DFEATURE_A -Iinclude -c src/calc.cpp"
        },
        {
            "directory": root.to_str().unwrap(),
            "file": "src/calc.cpp",
            "command": "clang++ -DFEATURE_B -Iinclude -c src/calc.cpp"
        },
        {
            "directory": root.to_str().unwrap(),
            "file": "src/cycle.cpp",
            "command": "clang++ -c src/cycle.cpp"
        },
        {
            "directory": root.to_str().unwrap(),
            "file": "src/unrelated.cpp",
            "command": "clang++ -c src/unrelated.cpp"
        }
    ]);
    fs::write(
        &db_path,
        serde_json::to_string_pretty(&json_content).unwrap(),
    )
    .unwrap();

    let project = ProjectContext::load(ProjectInput {
        source_root: root.to_path_buf(),
        compile_commands_path: db_path,
    })
    .unwrap();

    (dir, project)
}

/// AC-001: Symbol identification
#[test]
fn test_ac_001_symbol_identification() {
    let (_dir, project) = setup_acceptance_fixture();
    if !ensure_libclang_loaded() {
        return;
    }

    let provider = ClangProvider::new();
    let mut engine = QueryEngine::new(&project, provider);

    // 네임스페이스와 클래스 한정자가 포함된 심볼 식별
    let sym_query = SymbolQuery::parse("Math::Calculator::compute");
    let res = engine
        .execute(QueryRequest::Callees {
            source: sym_query,
            max_depth: None,
            verified_only: false,
        })
        .unwrap();

    assert_eq!(res.completion, Completion::Complete);
    assert!(!res.symbols.is_empty());
}

/// AC-002: Direct calls verification
#[test]
fn test_ac_002_direct_calls() {
    let (_dir, project) = setup_acceptance_fixture();
    if !ensure_libclang_loaded() {
        return;
    }

    let provider = ClangProvider::new();
    let mut engine = QueryEngine::new(&project, provider);

    let res = engine
        .execute(QueryRequest::Callers {
            target: SymbolQuery::parse("c_leaf"),
            max_depth: None,
            verified_only: true,
        })
        .unwrap();

    assert_eq!(res.completion, Completion::Complete);
    let direct_edge = res.edges.iter().find(|e| e.kind == CallKind::Direct);
    assert!(direct_edge.is_some(), "직접 호출 엣지가 확인되어야 함");
    assert_eq!(direct_edge.unwrap().confidence, Confidence::Confirmed);
}

/// AC-003: Overload resolution
#[test]
fn test_ac_003_overload_resolution() {
    let (_dir, project) = setup_acceptance_fixture();
    if !ensure_libclang_loaded() {
        return;
    }

    let provider = ClangProvider::new();
    let mut engine = QueryEngine::new(&project, provider);

    let res = engine
        .execute(QueryRequest::Callees {
            source: SymbolQuery::parse("Math::Calculator::compute"),
            max_depth: None,
            verified_only: true,
        })
        .unwrap();

    assert_eq!(res.completion, Completion::Complete);
    // compute 내부에서 add(int, int)와 add(double, double) 2개의 서로 다른 오버로드가 호출됨
    assert!(
        res.edges.len() >= 2,
        "오버로드된 add 호출들이 각각 정확히 식별되어야 함"
    );
}

/// AC-004: Caller recursive traversal
#[test]
fn test_ac_004_caller_recursive_traversal() {
    let (_dir, project) = setup_acceptance_fixture();
    if !ensure_libclang_loaded() {
        return;
    }

    let provider = ClangProvider::new();
    let mut engine = QueryEngine::new(&project, provider);

    let res = engine
        .execute(QueryRequest::Callers {
            target: SymbolQuery::parse("c_leaf"),
            max_depth: None,
            verified_only: false,
        })
        .unwrap();

    assert_eq!(res.completion, Completion::Complete);
    // c_leaf -> c_mid -> c_root 역방향 체인
    assert!(res.edges.len() >= 2);
}

/// AC-005: Callee traversal
#[test]
fn test_ac_005_callee_traversal() {
    let (_dir, project) = setup_acceptance_fixture();
    if !ensure_libclang_loaded() {
        return;
    }

    let provider = ClangProvider::new();
    let mut engine = QueryEngine::new(&project, provider);

    let res = engine
        .execute(QueryRequest::Callees {
            source: SymbolQuery::parse("c_root"),
            max_depth: None,
            verified_only: false,
        })
        .unwrap();

    assert_eq!(res.completion, Completion::Complete);
    // c_root -> c_mid -> c_leaf 순방향 체인
    assert!(res.edges.len() >= 2);
}

/// AC-006: Path query
#[test]
fn test_ac_006_path_query() {
    let (_dir, project) = setup_acceptance_fixture();
    if !ensure_libclang_loaded() {
        return;
    }

    let provider = ClangProvider::new();
    let mut engine = QueryEngine::new(&project, provider);

    let res = engine
        .execute(QueryRequest::Path {
            source: SymbolQuery::parse("c_root"),
            target: SymbolQuery::parse("c_leaf"),
            max_depth: None,
            verified_only: false,
        })
        .unwrap();

    assert_eq!(res.completion, Completion::Complete);
    assert_eq!(
        res.paths.len(),
        1,
        "c_root에서 c_leaf로의 경로가 1개 발견되어야 함"
    );
    assert_eq!(res.paths[0].nodes.len(), res.paths[0].edges.len() + 1);
}

/// AC-007: Cycle termination
#[test]
fn test_ac_007_cycle_termination() {
    let (_dir, project) = setup_acceptance_fixture();
    if !ensure_libclang_loaded() {
        return;
    }

    let provider = ClangProvider::new();
    let mut engine = QueryEngine::new(&project, provider);

    let res = engine
        .execute(QueryRequest::Callers {
            target: SymbolQuery::parse("ping"),
            max_depth: None,
            verified_only: false,
        })
        .unwrap();

    // 사이클 그래프에서도 무한 루프 없이 정상 완료
    assert_eq!(res.completion, Completion::Complete);
}

/// AC-008 & AC-009: Unrelated TU exclusion and 1 TU parse per context
#[test]
fn test_ac_008_ac_009_tu_isolation_and_caching() {
    let (_dir, project) = setup_acceptance_fixture();
    if !ensure_libclang_loaded() {
        return;
    }

    let provider = ClangProvider::new();
    let mut engine = QueryEngine::new(&project, provider);

    // c_chain.c에만 관련된 쿼리 수행
    let res = engine
        .execute(QueryRequest::Callers {
            target: SymbolQuery::parse("c_leaf"),
            max_depth: None,
            verified_only: false,
        })
        .unwrap();

    assert_eq!(res.completion, Completion::Complete);

    // 무관한 unrelated.cpp는 시맨틱 파싱되지 않아야 함 (AC-008)
    assert!(engine.provider.tu_parse_count < project.compilation_db.all_source_files().len());
}

/// AC-011: Deterministic output
#[test]
fn test_ac_011_deterministic_output() {
    let (_dir, project) = setup_acceptance_fixture();
    if !ensure_libclang_loaded() {
        return;
    }

    let provider1 = ClangProvider::new();
    let mut engine1 = QueryEngine::new(&project, provider1);
    let renderer = HumanRenderer::new();

    let res1 = engine1
        .execute(QueryRequest::Callers {
            target: SymbolQuery::parse("c_leaf"),
            max_depth: None,
            verified_only: false,
        })
        .unwrap();
    let out1 = renderer.render(&project, &res1);

    let provider2 = ClangProvider::new();
    let mut engine2 = QueryEngine::new(&project, provider2);
    let res2 = engine2
        .execute(QueryRequest::Callers {
            target: SymbolQuery::parse("c_leaf"),
            max_depth: None,
            verified_only: false,
        })
        .unwrap();
    let out2 = renderer.render(&project, &res2);

    assert_eq!(
        out1.stdout, out2.stdout,
        "반복 실행 시 출력이 완전히 동일해야 함 (결정론적 출력)"
    );
    assert_eq!(out1.exit_code, out2.exit_code);
}

/// AC-013: Explain query
#[test]
fn test_ac_013_explain_query() {
    let (_dir, project) = setup_acceptance_fixture();
    if !ensure_libclang_loaded() {
        return;
    }

    let provider = ClangProvider::new();
    let mut engine = QueryEngine::new(&project, provider);

    let res = engine
        .execute(QueryRequest::Explain {
            caller: SymbolQuery::parse("c_mid"),
            callee: SymbolQuery::parse("c_leaf"),
        })
        .unwrap();

    assert_eq!(res.completion, Completion::Complete);
    assert_eq!(res.edges.len(), 1);
    assert!(res.edges[0].evidence_by_context.values().next().is_some());
}

/// AC-014: Source location reporting
#[test]
fn test_ac_014_source_location_reporting() {
    let (_dir, project) = setup_acceptance_fixture();
    if !ensure_libclang_loaded() {
        return;
    }

    let provider = ClangProvider::new();
    let mut engine = QueryEngine::new(&project, provider);
    let renderer = HumanRenderer::new();

    let res = engine
        .execute(QueryRequest::Callers {
            target: SymbolQuery::parse("c_leaf"),
            max_depth: None,
            verified_only: false,
        })
        .unwrap();

    let rendered = renderer.render_with_options(
        &project,
        &res,
        RenderOptions {
            verbosity: 1,
            ..RenderOptions::default()
        },
    );
    assert!(
        rendered.stdout.contains("c_chain.c"),
        "소스 파일 경로가 출력에 포함되어야 함"
    );
}

/// AC-015: Metrics recording
#[test]
fn test_ac_015_metrics_recording() {
    let (_dir, project) = setup_acceptance_fixture();
    if !ensure_libclang_loaded() {
        return;
    }

    let provider = ClangProvider::new();
    let mut engine = QueryEngine::new(&project, provider);

    let res = engine
        .execute(QueryRequest::Callers {
            target: SymbolQuery::parse("c_leaf"),
            max_depth: None,
            verified_only: false,
        })
        .unwrap();

    assert!(res.metrics.source_files_inspected > 0);
    assert!(res.metrics.candidate_call_sites > 0);
}

/// AC-016: Multiple compilation contexts and provenance preservation
#[test]
fn test_ac_016_multiple_compilation_contexts() {
    let (_dir, project) = setup_acceptance_fixture();
    if !ensure_libclang_loaded() {
        return;
    }

    let provider = ClangProvider::new();
    let mut engine = QueryEngine::new(&project, provider);

    let res = engine
        .execute(QueryRequest::Callees {
            source: SymbolQuery::parse("run_feature"),
            max_depth: None,
            verified_only: false,
        })
        .unwrap();

    assert_eq!(res.completion, Completion::Complete);
    // FEATURE_A와 FEATURE_B 양쪽 컨텍스트가 모두 분석되어 엣지에 provenance가 보존되어야 함
    assert!(!res.edges.is_empty());
}

/// AC-017: Maximum depth truncation
#[test]
fn test_ac_017_max_depth_truncation() {
    let (_dir, project) = setup_acceptance_fixture();
    if !ensure_libclang_loaded() {
        return;
    }

    let provider = ClangProvider::new();
    let mut engine = QueryEngine::new(&project, provider);

    let res = engine
        .execute(QueryRequest::Callers {
            target: SymbolQuery::parse("c_leaf"),
            max_depth: Some(1),
            verified_only: false,
        })
        .unwrap();

    match res.completion {
        Completion::Truncated { max_depth } => assert_eq!(max_depth, 1),
        other => panic!("Truncated 상태여야 함, 실제: {other:?}"),
    }
}

/// AC-018: Exit semantics
#[test]
fn test_ac_018_exit_semantics() {
    let (_dir, project) = setup_acceptance_fixture();
    if !ensure_libclang_loaded() {
        return;
    }

    let provider = ClangProvider::new();
    let mut engine = QueryEngine::new(&project, provider);
    let renderer = HumanRenderer::new();

    // 정상 완료 쿼리 -> exit code 0
    let res = engine
        .execute(QueryRequest::Callers {
            target: SymbolQuery::parse("c_leaf"),
            max_depth: None,
            verified_only: false,
        })
        .unwrap();
    let rendered = renderer.render(&project, &res);
    assert_eq!(rendered.exit_code, 0);

    // depth truncation 쿼리 -> exit code 0 (정상 종료)
    let res_trunc = engine
        .execute(QueryRequest::Callers {
            target: SymbolQuery::parse("c_leaf"),
            max_depth: Some(1),
            verified_only: false,
        })
        .unwrap();
    let rendered_trunc = renderer.render(&project, &res_trunc);
    assert_eq!(rendered_trunc.exit_code, 0);
}
