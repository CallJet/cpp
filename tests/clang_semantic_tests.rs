//! Clang 시맨틱 검증 레이어 단위 및 통합 테스트
//! Unit and integration tests for Clang semantic verification layer

use std::fs;
use tempfile::tempdir;

use calljet::cli::ProjectInput;
use calljet::discovery::DiscoveryIndex;
use calljet::model::SymbolQuery;
use calljet::project::ProjectContext;
use calljet::semantic::clang::{ensure_libclang_loaded, ClangProvider};
use calljet::semantic::{SemanticProvider, VerificationBatch};

#[test]
fn test_clang_provider_tu_caching_invariant() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let src_file = root.join("main.cpp");
    let code = r#"
        void foo() {}
        void bar() { foo(); }
        void baz() { foo(); }
    "#;
    fs::write(&src_file, code).unwrap();

    let db_path = root.join("compile_commands.json");
    fs::write(
        &db_path,
        serde_json::json!([
            {
                "directory": root.to_str().unwrap(),
                "file": "main.cpp",
                "command": "clang++ -c main.cpp"
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

    let discovery = DiscoveryIndex::build(&project);
    let mut provider = ClangProvider::new();

    if !ensure_libclang_loaded() {
        println!("libclang not available in this environment; skipping native TU parse test");
        return;
    }

    let foo_query = SymbolQuery::parse("foo");
    let foo_cands = discovery.matching_symbols(&foo_query);
    assert!(!foo_cands.is_empty());

    // 1. Symbol resolution 시도 (첫 TU 파싱)
    let res = provider.resolve_symbols(&project, &discovery, foo_cands);
    assert!(!res.symbols.is_empty(), "foo 심볼이 해석되어야 함");

    let initial_parse_count = provider.tu_parse_count;
    assert_eq!(
        initial_parse_count, 1,
        "첫 컴파일 컨텍스트 파싱 횟수는 1이어야 함"
    );

    // 2. 같은 컴파일 컨텍스트에 속한 여러 후보 호출 검증 (bar->foo, baz->foo)
    let context_key = project.compilation_db.contexts_for_source(&src_file)[0]
        .key
        .clone();
    let mut all_calls = Vec::new();
    for &id in discovery.calls.keys() {
        all_calls.push(id);
    }

    let batch = VerificationBatch {
        context: context_key,
        symbols: foo_cands.to_vec(),
        calls: all_calls,
    };

    let verify_res = provider.verify_calls(&project, &discovery, batch);
    assert!(!verify_res.edges.is_empty(), "호출 엣지가 검증되어야 함");

    // 핵심 불변식: TU 파싱 횟수가 증가하지 않고 캐시가 사용되어야 함 (FR-045, NFR-001)
    assert_eq!(
        provider.tu_parse_count, 1,
        "동일한 Translation Unit에 대해 반복 파싱이 발생하지 않아야 함 (FR-045 불변식)"
    );
    assert!(provider.tu_cache_hits >= 1, "TU 캐시 히트가 발생해야 함");
}
