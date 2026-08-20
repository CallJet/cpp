//! CLI 인자 파싱 및 요청 구성 모듈
//! CLI argument parsing and request construction module

use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

use crate::diagnostic::InputError;
use crate::model::{QueryRequest, SymbolQuery};

/// CallJet C++ — C/C++ 소스 온디맨드 정적 호출 경로 분석 도구
#[derive(Debug, Parser)]
#[command(
    name = "calljet",
    author,
    version,
    about = "C and C++ on-demand static call-path analysis CLI",
    long_about = "CallJet C++ answers focused questions about callers, callees, call paths, and call edges on demand."
)]
pub struct Cli {
    /// 실행할 서브커맨드 (Subcommand)
    #[command(subcommand)]
    pub command: Commands,
}

/// 공통 분석 옵션 (Common Options)
#[derive(Debug, Args, Clone)]
pub struct CommonOptions {
    /// 분석 대상 소스 루트 디렉토리 (기본값: 현재 디렉토리)
    #[arg(long, value_name = "PATH", global = true)]
    pub root: Option<PathBuf>,

    /// compile_commands.json 파일 경로 (기본값: <root>/compile_commands.json)
    #[arg(long = "compile-commands", value_name = "PATH", global = true)]
    pub compile_commands: Option<PathBuf>,
}

/// 탐색 제어 옵션 (Traversal Options)
#[derive(Debug, Args, Clone)]
pub struct TraversalOptions {
    /// 최대 탐색 깊이 제한 (지정하지 않을 경우 무제한 탐색)
    #[arg(long = "max-depth", value_name = "N")]
    pub max_depth: Option<usize>,

    /// 시맨틱 검증으로 완전히 확정(CONFIRMED)된 엣지만 결과에 포함
    #[arg(long = "verified-only", default_value_t = false)]
    pub verified_only: bool,
}

/// 서브커맨드 정의 (Commands)
#[derive(Debug, Subcommand, Clone)]
pub enum Commands {
    /// 대상 심볼을 호출하는 함수/메서드(호출자) 탐색
    Callers {
        /// 탐색 대상 심볼 이름 또는 쿼리
        #[arg(value_name = "TARGET")]
        target: String,

        #[command(flatten)]
        common: CommonOptions,

        #[command(flatten)]
        traversal: TraversalOptions,
    },

    /// 소스 심볼이 호출하는 함수/메서드(피호출자) 탐색
    Callees {
        /// 탐색 시작 심볼 이름 또는 쿼리
        #[arg(value_name = "SOURCE")]
        source: String,

        #[command(flatten)]
        common: CommonOptions,

        #[command(flatten)]
        traversal: TraversalOptions,
    },

    /// 두 심볼 사이의 정적 호출 경로 탐색
    Path {
        /// 시작 소스 심볼
        #[arg(value_name = "SOURCE")]
        source: String,

        /// 도달 대상 심볼
        #[arg(value_name = "TARGET")]
        target: String,

        #[command(flatten)]
        common: CommonOptions,

        #[command(flatten)]
        traversal: TraversalOptions,
    },

    /// 두 심볼 간의 호출 엣지와 시맨틱 근거 상세 설명
    Explain {
        /// 호출자 심볼
        #[arg(value_name = "CALLER")]
        caller: String,

        /// 피호출자 심볼
        #[arg(value_name = "CALLEE")]
        callee: String,

        #[command(flatten)]
        common: CommonOptions,
    },
}

/// 프로젝트 분석 입력 명세 (Project Input Specification)
#[derive(Debug, Clone)]
pub struct ProjectInput {
    /// 소스 루트 디렉토리
    pub source_root: PathBuf,
    /// 컴파일 데이터베이스 파일 경로
    pub compile_commands_path: PathBuf,
}

impl Cli {
    /// 파싱된 CLI로부터 ProjectInput 및 QueryRequest 추출
    pub fn into_request(self) -> Result<(ProjectInput, QueryRequest), InputError> {
        let (common, request) = match self.command {
            Commands::Callers {
                target,
                common,
                traversal,
            } => {
                let target_query = SymbolQuery::parse(&target);
                (
                    common,
                    QueryRequest::Callers {
                        target: target_query,
                        max_depth: traversal.max_depth,
                        verified_only: traversal.verified_only,
                    },
                )
            }
            Commands::Callees {
                source,
                common,
                traversal,
            } => {
                let source_query = SymbolQuery::parse(&source);
                (
                    common,
                    QueryRequest::Callees {
                        source: source_query,
                        max_depth: traversal.max_depth,
                        verified_only: traversal.verified_only,
                    },
                )
            }
            Commands::Path {
                source,
                target,
                common,
                traversal,
            } => {
                let source_query = SymbolQuery::parse(&source);
                let target_query = SymbolQuery::parse(&target);
                (
                    common,
                    QueryRequest::Path {
                        source: source_query,
                        target: target_query,
                        max_depth: traversal.max_depth,
                        verified_only: traversal.verified_only,
                    },
                )
            }
            Commands::Explain {
                caller,
                callee,
                common,
            } => {
                let caller_query = SymbolQuery::parse(&caller);
                let callee_query = SymbolQuery::parse(&callee);
                (
                    common,
                    QueryRequest::Explain {
                        caller: caller_query,
                        callee: callee_query,
                    },
                )
            }
        };

        let current_dir = std::env::current_dir().map_err(|e| InputError::IoError {
            path: PathBuf::from("."),
            reason: format!("현재 작업 디렉토리를 가져올 수 없습니다: {e}"),
        })?;

        let source_root = common.root.unwrap_or_else(|| current_dir.clone());
        let compile_commands_path = common
            .compile_commands
            .unwrap_or_else(|| source_root.join("compile_commands.json"));

        Ok((
            ProjectInput {
                source_root,
                compile_commands_path,
            },
            request,
        ))
    }
}
