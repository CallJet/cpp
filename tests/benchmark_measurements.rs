//! PoC 성능 계측 및 아키텍처 가설 검증 벤치마크 테스트
//! Performance instrumentation and hypothesis validation benchmark tests

use std::fs;
use std::time::Instant;
use tempfile::tempdir;

use calljet::cli::ProjectInput;
use calljet::model::{QueryRequest, SymbolQuery};
use calljet::project::ProjectContext;
use calljet::query::QueryEngine;
use calljet::semantic::clang::{ensure_libclang_loaded, ClangProvider};

#[test]
fn test_poc_demand_driven_efficiency_hypothesis() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let mut compile_entries = Vec::new();

    // 1. 타겟 호출 체인이 속한 2개의 TU 생성
    let chain1 = src_dir.join("chain1.cpp");
    fs::write(&chain1, "void target_function() {}").unwrap();
    compile_entries.push(serde_json::json!({
        "directory": root.to_str().unwrap(),
        "file": "src/chain1.cpp",
        "command": "clang++ -c src/chain1.cpp"
    }));

    let chain2 = src_dir.join("chain2.cpp");
    fs::write(
        &chain2,
        "void target_function(); void caller_function() { target_function(); }",
    )
    .unwrap();
    compile_entries.push(serde_json::json!({
        "directory": root.to_str().unwrap(),
        "file": "src/chain2.cpp",
        "command": "clang++ -c src/chain2.cpp"
    }));

    // 2. 쿼리와 무관한 10개의 대형 독립 TU 생성 (Unrelated TUs)
    for i in 0..10 {
        let unrelated = src_dir.join(format!("unrelated_{i}.cpp"));
        fs::write(
            &unrelated,
            format!("void unrelated_fn_{i}() {{ int x = {i}; }}"),
        )
        .unwrap();
        compile_entries.push(serde_json::json!({
            "directory": root.to_str().unwrap(),
            "file": format!("src/unrelated_{i}.cpp"),
            "command": format!("clang++ -c src/unrelated_{i}.cpp")
        }));
    }

    let db_path = root.join("compile_commands.json");
    fs::write(
        &db_path,
        serde_json::to_string_pretty(&compile_entries).unwrap(),
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

    let start_time = Instant::now();
    let res = engine
        .execute(QueryRequest::Callers {
            target: SymbolQuery::parse("target_function"),
            max_depth: None,
            verified_only: false,
        })
        .unwrap();
    let duration = start_time.elapsed();

    let total_tus = project.compilation_db.all_source_files().len();
    let parsed_tus = engine.provider.tu_parse_count;

    println!("========================================================");
    println!("CallJet PoC Architectural Hypothesis Benchmark Results");
    println!("========================================================");
    println!("Total Available TUs in Project: {total_tus}");
    println!(
        "Tree-sitter Discovered Candidates: {}",
        res.metrics.candidate_call_sites
    );
    println!("Actual Clang Parsed TUs: {parsed_tus}");
    println!(
        "TU Reduction Ratio: {:.1}% saved from expensive Clang verification",
        (1.0 - (parsed_tus as f64 / total_tus as f64)) * 100.0
    );
    println!("Total Query Duration: {duration:?}");
    println!("========================================================");

    // 핵심 아키텍처 가설 검증:
    // 전체 12개 TU 중 오직 관련된 2개 TU 이하만 Clang 시맨틱 파싱되어야 하며,
    // 10개의 무관한 TU는 Clang 파싱을 완전히 건너뛰어야 함!
    assert_eq!(total_tus, 12);
    assert!(parsed_tus <= 2, "포커스된 쿼리는 무관한 10개 TU를 파싱하지 않고 오직 관련 TU({parsed_tus}개)만 파싱해야 함!");
    assert!(parsed_tus < total_tus);
}
