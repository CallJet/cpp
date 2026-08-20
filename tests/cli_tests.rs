//! CLI 인자 파싱 및 명령 구성 단위 테스트
//! Unit tests for CLI argument parsing and command construction

use calljet::cli::{Cli, Commands};
use calljet::model::QueryRequest;
use clap::Parser;

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
