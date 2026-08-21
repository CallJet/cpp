//! 컴파일 데이터베이스 및 프로젝트 컨텍스트 단위 테스트
//! Unit tests for compilation database and project context

use std::fs;
use tempfile::tempdir;

use calljet::cli::ProjectInput;
use calljet::compile_db::CompilationDb;
use calljet::diagnostic::{AnalysisCause, Diagnostic, InputError};
use calljet::project::ProjectContext;

#[test]
fn test_missing_compile_commands() {
    let dir = tempdir().unwrap();
    let missing_path = dir.path().join("compile_commands.json");

    let result = CompilationDb::load(&missing_path);
    assert!(result.is_err());
    match result.unwrap_err() {
        InputError::InvalidCompilationDatabase { path, reason } => {
            assert_eq!(path, missing_path);
            assert!(reason.contains("존재하지 않습니다"));
        }
        err => panic!("예상치 못한 에러: {err:?}"),
    }
}

#[test]
fn test_malformed_json_compile_commands() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("compile_commands.json");
    fs::write(&db_path, "{ malformed json }").unwrap();

    let result = CompilationDb::load(&db_path);
    assert!(result.is_err());
    match result.unwrap_err() {
        InputError::InvalidCompilationDatabase { reason, .. } => {
            assert!(reason.contains("JSON"));
        }
        err => panic!("예상치 못한 에러: {err:?}"),
    }
}

#[test]
fn test_project_context_continues_without_compilation_database() {
    let dir = tempdir().unwrap();
    let source_file = dir.path().join("standalone.cpp");
    fs::write(&source_file, "void standalone() {}\n").unwrap();

    let project = ProjectContext::load(ProjectInput {
        source_root: dir.path().to_path_buf(),
        compile_commands_path: dir.path().join("compile_commands.json"),
    })
    .expect("컴파일 데이터베이스가 없어도 Tree-sitter 프로젝트는 로드되어야 함");

    assert!(project.compilation_db.all_source_files().is_empty());
    assert!(project.compilation_db.diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic,
            Diagnostic::Analysis(issue)
                if matches!(
                    &issue.cause,
                    AnalysisCause::MissingCompilationContext
                )
        )
    }));
    assert_eq!(project.source_files(), vec![fs::canonicalize(source_file).unwrap()]);
}

#[test]
fn test_valid_compile_commands_with_multiple_contexts() {
    let dir = tempdir().unwrap();
    let src_dir = dir.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let file_a = src_dir.join("main.cpp");
    let file_b = src_dir.join("helper.cpp");
    fs::write(&file_a, "int main() { return 0; }").unwrap();
    fs::write(&file_b, "void helper() {}").unwrap();

    let db_path = dir.path().join("compile_commands.json");
    let json_content = serde_json::json!([
        {
            "directory": dir.path().to_str().unwrap(),
            "file": "src/main.cpp",
            "command": "clang++ -DFEATURE_A -Iinclude -c src/main.cpp -o src/main.o"
        },
        {
            "directory": dir.path().to_str().unwrap(),
            "file": "src/main.cpp",
            "arguments": ["clang++", "-DFEATURE_B", "-Iinclude", "-c", "src/main.cpp", "-o", "src/main.o"]
        },
        {
            "directory": dir.path().to_str().unwrap(),
            "file": "src/helper.cpp",
            "command": "clang++ -Iinclude -c src/helper.cpp -o src/helper.o"
        },
        // 중복 항목 (정확히 동일한 엔트리 -> 중복 제거 확인)
        {
            "directory": dir.path().to_str().unwrap(),
            "file": "src/helper.cpp",
            "command": "clang++ -Iinclude -c src/helper.cpp -o src/helper.o"
        }
    ]);

    fs::write(
        &db_path,
        serde_json::to_string_pretty(&json_content).unwrap(),
    )
    .unwrap();

    let db = CompilationDb::load(&db_path).unwrap();

    // 1. main.cpp의 다중 컨텍스트 확인 (FEATURE_A와 FEATURE_B 모두 보존)
    let main_contexts = db.contexts_for_source(&file_a);
    assert_eq!(
        main_contexts.len(),
        2,
        "main.cpp는 2개의 서로 다른 컨텍스트를 가져야 함"
    );
    assert_ne!(
        main_contexts[0].key, main_contexts[1].key,
        "두 컨텍스트의 CompilationKey는 달라야 함"
    );

    // Clang 인자 정규화 확인 (빌드 출력 -o, -c 제거, -D, -I 유지)
    let args_0: Vec<String> = main_contexts[0]
        .clang_args
        .iter()
        .map(|s| s.to_string_lossy().to_string())
        .collect();
    let args_1: Vec<String> = main_contexts[1]
        .clang_args
        .iter()
        .map(|s| s.to_string_lossy().to_string())
        .collect();
    assert!(args_0.contains(&"-DFEATURE_A".to_string()));
    assert!(args_0.contains(&"-Iinclude".to_string()));
    assert!(!args_0.contains(&"-o".to_string()));
    assert!(!args_0.contains(&"-c".to_string()));

    assert!(args_1.contains(&"-DFEATURE_B".to_string()));

    // 2. helper.cpp의 중복 엔트리 제거 확인 (1개만 존재해야 함)
    let helper_contexts = db.contexts_for_source(&file_b);
    assert_eq!(
        helper_contexts.len(),
        1,
        "helper.cpp는 중복 제거되어 1개의 컨텍스트만 가져야 함"
    );
}

#[test]
fn test_command_preserves_windows_backslashes_inside_double_quotes() {
    let dir = tempdir().unwrap();
    let source_file = dir.path().join("main.c");
    fs::write(&source_file, "int main(void) { return 0; }").unwrap();

    let db_path = dir.path().join("compile_commands.json");
    let json_content = serde_json::json!([{
        "directory": dir.path().to_str().unwrap(),
        "file": "main.c",
        "command": r#"clang "-IC:\Program Files\SDK\include" -c main.c"#
    }]);
    fs::write(
        &db_path,
        serde_json::to_string_pretty(&json_content).unwrap(),
    )
    .unwrap();

    let db = CompilationDb::load(&db_path).unwrap();
    let context = &db.contexts_for_source(&source_file)[0];
    let args = context
        .clang_args
        .iter()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect::<Vec<_>>();

    assert_eq!(args, vec![r#"-IC:\Program Files\SDK\include"#]);
}

#[test]
fn test_cpp_driver_restores_implicit_language_mode() {
    let dir = tempdir().unwrap();
    let source_file = dir.path().join("legacy.c");
    fs::write(&source_file, "class Legacy {};").unwrap();

    let db_path = dir.path().join("compile_commands.json");
    let json_content = serde_json::json!([{
        "directory": dir.path().to_str().unwrap(),
        "file": "legacy.c",
        "arguments": [
            r#"C:\MinGW\bin\g++.exe"#,
            "-DCPP_MODE",
            "-c",
            "legacy.c"
        ]
    }]);
    fs::write(
        &db_path,
        serde_json::to_string_pretty(&json_content).unwrap(),
    )
    .unwrap();

    let db = CompilationDb::load(&db_path).unwrap();
    let context = &db.contexts_for_source(&source_file)[0];
    let args = context
        .clang_args
        .iter()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect::<Vec<_>>();

    assert_eq!(args, vec!["-x", "c++", "-DCPP_MODE"]);
}

#[test]
fn test_cpp_driver_preserves_explicit_language_mode() {
    let dir = tempdir().unwrap();
    let source_file = dir.path().join("forced.c");
    fs::write(&source_file, "int forced(void) { return 0; }").unwrap();

    let db_path = dir.path().join("compile_commands.json");
    let json_content = serde_json::json!([{
        "directory": dir.path().to_str().unwrap(),
        "file": "forced.c",
        "arguments": ["clang++", "-x", "c", "-c", "forced.c"]
    }]);
    fs::write(
        &db_path,
        serde_json::to_string_pretty(&json_content).unwrap(),
    )
    .unwrap();

    let db = CompilationDb::load(&db_path).unwrap();
    let context = &db.contexts_for_source(&source_file)[0];
    let args = context
        .clang_args
        .iter()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect::<Vec<_>>();

    assert_eq!(args, vec!["-x", "c"]);
}

#[test]
fn test_unusable_entry_diagnostics() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("compile_commands.json");
    let json_content = serde_json::json!([
        {
            // directory 누락
            "file": "src/main.cpp",
            "command": "clang++ src/main.cpp"
        },
        {
            // file 누락
            "directory": dir.path().to_str().unwrap(),
            "command": "clang++ src/main.cpp"
        },
        {
            // command와 arguments 모두 누락
            "directory": dir.path().to_str().unwrap(),
            "file": "src/main.cpp"
        }
    ]);

    fs::write(
        &db_path,
        serde_json::to_string_pretty(&json_content).unwrap(),
    )
    .unwrap();

    let db = CompilationDb::load(&db_path).unwrap();
    assert_eq!(
        db.diagnostics.len(),
        3,
        "사용 불가 엔트리 3건에 대해 진단이 수집되어야 함"
    );
}

#[test]
fn test_project_context_load_and_source_enumeration() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let main_cpp = src_dir.join("main.cpp");
    let header_h = src_dir.join("header.h");
    let non_source = src_dir.join("notes.txt");

    fs::write(&main_cpp, "int main() {}").unwrap();
    fs::write(&header_h, "#pragma once").unwrap();
    fs::write(&non_source, "just notes").unwrap();

    let db_path = root.join("compile_commands.json");
    let json_content = serde_json::json!([
        {
            "directory": root.to_str().unwrap(),
            "file": "src/main.cpp",
            "command": "clang++ -c src/main.cpp"
        }
    ]);
    fs::write(&db_path, serde_json::to_string(&json_content).unwrap()).unwrap();

    let project = ProjectContext::load(ProjectInput {
        source_root: root.to_path_buf(),
        compile_commands_path: db_path,
    })
    .unwrap();

    let source_files = project.source_files();
    let file_names: Vec<String> = source_files
        .iter()
        .map(|p| p.file_name().unwrap().to_str().unwrap().to_string())
        .collect();

    assert!(file_names.contains(&"main.cpp".to_string()));
    assert!(file_names.contains(&"header.h".to_string()));
    assert!(!file_names.contains(&"notes.txt".to_string()));
}

#[cfg(unix)]
#[test]
fn test_source_enumeration_does_not_follow_directory_symlinks() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let root = dir.path().join("project");
    let outside = dir.path().join("outside");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&outside).unwrap();

    let main_cpp = root.join("main.cpp");
    let outside_cpp = outside.join("outside.cpp");
    fs::write(&main_cpp, "int main() {}").unwrap();
    fs::write(&outside_cpp, "void outside() {}").unwrap();

    symlink(&root, root.join("cycle")).unwrap();
    symlink(&outside, root.join("external")).unwrap();

    let db_path = root.join("compile_commands.json");
    fs::write(
        &db_path,
        serde_json::json!([{
            "directory": root.to_str().unwrap(),
            "file": "main.cpp",
            "command": "clang++ -c main.cpp"
        }])
        .to_string(),
    )
    .unwrap();

    let project = ProjectContext::load(ProjectInput {
        source_root: root,
        compile_commands_path: db_path,
    })
    .unwrap();

    let source_files = project.source_files();
    assert_eq!(source_files, vec![fs::canonicalize(main_cpp).unwrap()]);
    assert!(!source_files.contains(&fs::canonicalize(outside_cpp).unwrap()));
}
