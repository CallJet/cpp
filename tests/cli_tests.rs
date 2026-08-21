//! CLI 인자 파싱 및 명령 구성 단위 테스트
//! Unit tests for CLI argument parsing and command construction

use std::process::Command;

use calljet::cli::{Cli, Commands};
use calljet::console::{missing_compilation_database_help, CALLJET_ASCII_ART};
use calljet::model::QueryRequest;
use clap::Parser;
use tempfile::tempdir;

#[test]
fn test_cli_trace_parsing() {
    let cli = Cli::try_parse_from([
        "calljet",
        "trace",
        "ns::Class::method",
        "--max-depth",
        "8",
    ])
    .unwrap();

    let (_, req) = cli.into_request().unwrap();
    if let QueryRequest::Trace {
        target,
        max_depth,
        verified_only,
    } = req
    {
        assert_eq!(target.terminal_name, "method");
        assert_eq!(target.qualifier_hint, Some("ns::Class::".to_string()));
        assert_eq!(max_depth, Some(8));
        assert!(!verified_only);
    } else {
        panic!("예상치 못한 커맨드 형태");
    }
}

#[test]
fn test_cli_callers_parsing() {
    let cli = Cli::try_parse_from([
        "calljet",
        "callers",
        "target_func",
        "--max-depth",
        "5",
        "--verified-only",
    ])
    .unwrap();
    if let Commands::Callers {
        target, traversal, ..
    } = &cli.command
    {
        assert_eq!(target, "target_func");
        assert_eq!(traversal.max_depth, Some(5));
        assert!(traversal.verified_only);
    } else {
        panic!("예상치 못한 커맨드 형태");
    }

    let (_, req) = cli.into_request().unwrap();
    if let QueryRequest::Callers {
        target,
        max_depth,
        verified_only,
    } = req
    {
        assert_eq!(target.terminal_name, "target_func");
        assert_eq!(max_depth, Some(5));
        assert!(verified_only);
    } else {
        panic!("예상치 못한 요청 형태");
    }
}

#[test]
fn test_cli_callees_parsing() {
    let cli = Cli::try_parse_from(["calljet", "callees", "ns::Class::method"]).unwrap();
    let (_, req) = cli.into_request().unwrap();
    if let QueryRequest::Callees {
        source,
        max_depth,
        verified_only,
    } = req
    {
        assert_eq!(source.terminal_name, "method");
        assert_eq!(source.qualifier_hint, Some("ns::Class::".to_string()));
        assert_eq!(max_depth, None);
        assert!(!verified_only);
    } else {
        panic!("예상치 못한 요청 형태");
    }
}

#[test]
fn test_cli_path_parsing() {
    let cli = Cli::try_parse_from(["calljet", "path", "start_fn", "end_fn", "--max-depth", "10"])
        .unwrap();
    let (_, req) = cli.into_request().unwrap();
    if let QueryRequest::Path {
        source,
        target,
        max_depth,
        verified_only,
    } = req
    {
        assert_eq!(source.terminal_name, "start_fn");
        assert_eq!(target.terminal_name, "end_fn");
        assert_eq!(max_depth, Some(10));
        assert!(!verified_only);
    } else {
        panic!("예상치 못한 요청 형태");
    }
}

#[test]
fn test_cli_explain_parsing() {
    let cli = Cli::try_parse_from(["calljet", "explain", "caller_fn", "callee_fn"]).unwrap();
    let (_, req) = cli.into_request().unwrap();
    if let QueryRequest::Explain { caller, callee } = req {
        assert_eq!(caller.terminal_name, "caller_fn");
        assert_eq!(callee.terminal_name, "callee_fn");
    } else {
        panic!("예상치 못한 요청 형태");
    }
}

#[test]
fn test_cli_invalid_command() {
    let result = Cli::try_parse_from(["calljet", "unknown_cmd"]);
    assert!(result.is_err());
}

#[test]
fn test_cli_rich_options_parsing() {
    let cli = Cli::try_parse_from([
        "calljet",
        "callers",
        "target_fn",
        "--format",
        "json",
        "--output",
        "output.json",
        "--metrics",
        "-vv",
        "--no-unresolved",
        "--no-foreign",
    ])
    .unwrap();

    let (_, _, render_opts) = cli.into_execution_plan().unwrap();
    assert_eq!(render_opts.format, calljet::cli::OutputFormat::Json);
    assert_eq!(
        render_opts.output_file,
        Some(std::path::PathBuf::from("output.json"))
    );
    assert!(render_opts.show_metrics);
    assert_eq!(render_opts.verbosity, 2);
    assert!(render_opts.no_unresolved);
    assert!(render_opts.no_foreign);
}

#[test]
fn test_cli_verbosity_count_and_long_option_compatibility() {
    let short = Cli::try_parse_from(["calljet", "callers", "target_fn", "-v"]).unwrap();
    let (_, _, short_options) = short.into_execution_plan().unwrap();
    assert_eq!(short_options.verbosity, 1);
    assert!(!short_options.progress);

    let repeated = Cli::try_parse_from(["calljet", "callers", "target_fn", "-vv"]).unwrap();
    let (_, _, repeated_options) = repeated.into_execution_plan().unwrap();
    assert_eq!(repeated_options.verbosity, 2);

    let long =
        Cli::try_parse_from(["calljet", "callers", "target_fn", "--verbose"]).unwrap();
    let (_, _, long_options) = long.into_execution_plan().unwrap();
    assert_eq!(long_options.verbosity, 1);
}

#[test]
fn test_cli_progress_does_not_enable_verbose_result_output() {
    let cli = Cli::try_parse_from([
        "calljet",
        "trace",
        "target_fn",
        "--progress",
    ])
    .unwrap();
    let (_, _, options) = cli.into_execution_plan().unwrap();

    assert!(options.progress);
    assert_eq!(options.verbosity, 0);
}

#[test]
fn test_calljet_ascii_art_is_a_wordmark() {
    assert!(CALLJET_ASCII_ART.contains("/ ____|"));
    assert!(CALLJET_ASCII_ART.contains("| |___|"));
    assert!(CALLJET_ASCII_ART.contains("\\_____\\__,_|_|_|"));
    assert!(CALLJET_ASCII_ART.contains("FIND THE PATH. SKIP THE WHOLE GRAPH."));
}

#[test]
fn test_missing_compilation_database_help_is_actionable() {
    let path = std::path::Path::new("project/compile_commands.json");
    let help = missing_compilation_database_help(path);

    assert!(help.contains("project/compile_commands.json"));
    assert!(help.contains("continuing with Tree-sitter candidates"));
    assert!(help.contains("Clang semantic verification is unavailable"));
    assert!(help.contains("-DCMAKE_EXPORT_COMPILE_COMMANDS=ON"));
    assert!(help.contains("--compile-commands build/compile_commands.json"));
}

#[test]
fn test_missing_compile_commands_uses_quiet_treesitter_fallback() {
    let root = tempdir().unwrap();
    std::fs::write(
        root.path().join("chain.cpp"),
        "void leaf() {}\nvoid mid() { leaf(); }\nvoid root_fn() { mid(); }\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_calljet"))
        .args(["trace", "leaf", "--root"])
        .arg(root.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "root_fn\nmid\nleaf\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn test_progress_keeps_compact_output_and_hides_project_paths() {
    let root = tempdir().unwrap();
    std::fs::write(
        root.path().join("chain.cpp"),
        "void leaf() {}\nvoid mid() { leaf(); }\nvoid root_fn() { mid(); }\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_calljet"))
        .args(["trace", "leaf", "--progress", "--root"])
        .arg(root.path())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "root_fn\nmid\nleaf\n"
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("[CallJet]"));
    assert!(!stderr.contains(&root.path().display().to_string()));
    assert!(!stderr.contains(r"\\?\"));
}
