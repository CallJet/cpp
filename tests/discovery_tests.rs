//! Tree-sitter 구문 후보 탐색 엔진 단위 테스트
//! Unit tests for Tree-sitter discovery engine

use std::fs;
use tempfile::tempdir;

use calljet::cli::ProjectInput;
use calljet::discovery::DiscoveryIndex;
use calljet::model::{
    CandidateCallKind, CandidateSymbolKind, Language, Symbol, SymbolId, SymbolQuery,
};
use calljet::project::ProjectContext;

#[test]
fn test_discovery_c_plain_functions_and_calls() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let src_file = root.join("main.c");
    let c_code = r#"
        #include <stdio.h>

        void callee_func() {
            printf("inside callee\n");
        }

        void caller_func() {
            callee_func();
        }

        int main() {
            caller_func();
            return 0;
        }
    "#;
    fs::write(&src_file, c_code).unwrap();

    let db_path = root.join("compile_commands.json");
    fs::write(
        &db_path,
        serde_json::json!([
            {
                "directory": root.to_str().unwrap(),
                "file": "main.c",
                "command": "gcc -c main.c"
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

    let index = DiscoveryIndex::build(&project);

    // 심볼 발견 확인
    let query_callee = SymbolQuery::parse("callee_func");
    let callee_syms = index.matching_symbols(&query_callee);
    assert_eq!(
        callee_syms.len(),
        1,
        "callee_func 후보 심볼 1개 발견되어야 함"
    );

    let query_caller = SymbolQuery::parse("caller_func");
    let caller_syms = index.matching_symbols(&query_caller);
    assert_eq!(
        caller_syms.len(),
        1,
        "caller_func 후보 심볼 1개 발견되어야 함"
    );

    let caller_sym_id = caller_syms[0];
    let caller_sym = index.symbols.get(&caller_sym_id).unwrap();
    assert_eq!(caller_sym.name, "caller_func");
    assert_eq!(caller_sym.language, Language::C);
    assert_eq!(caller_sym.syntactic_kind, CandidateSymbolKind::Function);

    // 순방향 호출 탐색: caller_func -> callee_func
    let caller_callees = index.candidate_callees(caller_sym_id);
    assert!(
        !caller_callees.is_empty(),
        "caller_func 내부의 호출식이 발견되어야 함"
    );
    let call_site = index.calls.get(&caller_callees[0]).unwrap();
    assert_eq!(call_site.callee_spelling, "callee_func");
    assert_eq!(call_site.syntax_hint, CandidateCallKind::Direct);

    // 역방향 호출자 탐색: callee_func를 호출하는 candidate_callers
    let dummy_target_symbol = Symbol {
        id: SymbolId::clang_usr(Language::C, "c:@F@callee_func"),
        name: "callee_func".to_string(),
        qualified_name: None,
        namespace: None,
        class_name: None,
        signature: None,
        declaration: None,
        definition: None,
    };
    let reverse_calls = index.candidate_callers(&dummy_target_symbol);
    assert!(
        !reverse_calls.is_empty(),
        "callee_func의 역방향 호출 후보가 발견되어야 함"
    );
    let reverse_call_site = index.calls.get(&reverse_calls[0]).unwrap();
    assert_eq!(reverse_call_site.caller, caller_sym_id);
}

#[test]
fn test_discovery_cpp_namespaces_classes_and_methods() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let src_file = root.join("engine.cpp");
    let cpp_code = r#"
        namespace Core {
            namespace Net {
                class Socket {
                public:
                    void connect(const char* host);
                    void sendData() {
                        connect("127.0.0.1");
                    }
                };
            }

            void launch() {
                Net::Socket sock;
                sock.sendData();
            }
        }
    "#;
    fs::write(&src_file, cpp_code).unwrap();

    let db_path = root.join("compile_commands.json");
    fs::write(
        &db_path,
        serde_json::json!([
            {
                "directory": root.to_str().unwrap(),
                "file": "engine.cpp",
                "command": "clang++ -std=c++17 -c engine.cpp"
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

    let index = DiscoveryIndex::build(&project);

    // 중첩 네임스페이스 및 클래스 메서드 발견 확인
    let query_connect = SymbolQuery::parse("Core::Net::Socket::connect");
    let connect_candidates = index.matching_symbols(&query_connect);
    assert!(
        !connect_candidates.is_empty(),
        "Core::Net::Socket::connect 후보 심볼 발견되어야 함"
    );

    let query_send = SymbolQuery::parse("sendData");
    let send_candidates = index.matching_symbols(&query_send);
    assert!(!send_candidates.is_empty());
    let send_sym = index.symbols.get(&send_candidates[0]).unwrap();
    assert_eq!(send_sym.name, "sendData");
    assert_eq!(
        send_sym.qualifier_hint,
        Some("Core::Net::Socket::".to_string())
    );
    assert_eq!(send_sym.owner_hint, Some("Socket".to_string()));

    // launch 함수에서 멤버 호출(sock.sendData()) 확인
    let query_launch = SymbolQuery::parse("Core::launch");
    let launch_candidates = index.matching_symbols(&query_launch);
    assert!(!launch_candidates.is_empty());
    let launch_id = launch_candidates[0];
    assert_eq!(
        index.symbols.get(&launch_id).unwrap().syntactic_kind,
        CandidateSymbolKind::Function,
        "네임스페이스 내부 자유 함수는 메서드로 분류하면 안 됨"
    );

    let launch_calls = index.candidate_callees(launch_id);
    let member_call = launch_calls
        .iter()
        .map(|id| index.calls.get(id).unwrap())
        .find(|c| c.callee_spelling == "sendData")
        .expect("sendData 멤버 호출이 발견되어야 함");
    assert_eq!(member_call.syntax_hint, CandidateCallKind::Member);
    let expression_point = member_call.expression.start.point.unwrap();
    let callee_point = member_call
        .callee_location
        .as_ref()
        .and_then(|location| location.point)
        .expect("멤버 이름의 정확한 위치가 기록되어야 함");
    assert_eq!(callee_point.line, expression_point.line);
    assert!(callee_point.column > expression_point.column);
}

#[test]
fn test_discovery_overloaded_functions() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let src_file = root.join("overload.cpp");
    let cpp_code = r#"
        void compute(int x) {}
        void compute(double y) {}
        void run() {
            compute(42);
            compute(3.14);
        }
    "#;
    fs::write(&src_file, cpp_code).unwrap();

    let db_path = root.join("compile_commands.json");
    fs::write(
        &db_path,
        serde_json::json!([
            {
                "directory": root.to_str().unwrap(),
                "file": "overload.cpp",
                "command": "clang++ -c overload.cpp"
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

    let index = DiscoveryIndex::build(&project);

    // 단말 이름 'compute'로 2개의 서로 다른 후보 심볼이 수집되어야 함 (SDS 6.3 중복제거 불변식)
    let query = SymbolQuery::parse("compute");
    let compute_candidates = index.matching_symbols(&query);
    assert_eq!(
        compute_candidates.len(),
        2,
        "오버로드된 compute 심볼 2개가 별개의 후보로 등록되어야 함"
    );
    assert_ne!(compute_candidates[0], compute_candidates[1]);
}

#[test]
fn test_discovery_malformed_source_handling() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let src_file = root.join("broken.cpp");
    // 의도적인 문법 오류 코드
    let broken_code = r#"
        void valid_func() {
            printf("ok");
        }

        void broken_func( { // 문법 에러 발생 영역
            ??? !!!
        }

        void another_valid() {
            valid_func();
        }
    "#;
    fs::write(&src_file, broken_code).unwrap();

    let db_path = root.join("compile_commands.json");
    fs::write(
        &db_path,
        serde_json::json!([
            {
                "directory": root.to_str().unwrap(),
                "file": "broken.cpp",
                "command": "clang++ -c broken.cpp"
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

    // 파싱 중 패닉하지 않고 온전한 부분의 후보 심볼/호출을 복구하여 인덱싱해야 함
    let index = DiscoveryIndex::build(&project);

    let query_valid = SymbolQuery::parse("valid_func");
    let valid_syms = index.matching_symbols(&query_valid);
    assert!(
        !valid_syms.is_empty(),
        "문법 에러가 있어도 정상 함수 valid_func는 복구 추출되어야 함"
    );
}

#[test]
fn test_query_discovery_parses_only_text_prefilter_matches() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let target_file = root.join("target.cpp");
    let unrelated_a = root.join("unrelated_a.cpp");
    let unrelated_b = root.join("unrelated_b.cpp");

    fs::write(&target_file, "void target_method() {}\n").unwrap();
    fs::write(&unrelated_a, "void unrelated_alpha() {}\n").unwrap();
    fs::write(&unrelated_b, "void unrelated_beta() {}\n").unwrap();

    let db_path = root.join("compile_commands.json");
    fs::write(
        &db_path,
        serde_json::json!([
            {
                "directory": root.to_str().unwrap(),
                "file": "target.cpp",
                "command": "clang++ -c target.cpp"
            },
            {
                "directory": root.to_str().unwrap(),
                "file": "unrelated_a.cpp",
                "command": "clang++ -c unrelated_a.cpp"
            },
            {
                "directory": root.to_str().unwrap(),
                "file": "unrelated_b.cpp",
                "command": "clang++ -c unrelated_b.cpp"
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

    let mut index = DiscoveryIndex::default();
    assert!(index.source_files.is_empty());

    let query = SymbolQuery::parse("target_method");
    index.discover_query(&project, &query);

    assert_eq!(index.source_files_inspected, 3);
    assert_eq!(index.source_files.len(), 1);
    assert_eq!(index.source_files[0], fs::canonicalize(target_file).unwrap());
    assert_eq!(index.matching_symbols(&query).len(), 1);
}
