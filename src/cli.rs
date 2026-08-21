use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

use crate::diagnostic::InputError;
use crate::model::{QueryRequest, SymbolQuery};

/// 출력 형식 (Output Format)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum OutputFormat {
    /// 기본 사람이 읽기 쉬운 텍스트 형식
    #[default]
    Text,
    /// JSON 형식 (자동화 및 도구 연동용)
    Json,
    /// Mermaid 다이어그램 형식 (마크다운 임베드용)
    Mermaid,
    /// Graphviz DOT 다이어그램 형식
    Dot,
}

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
    #[arg(short = 'r', long = "root", value_name = "PATH", global = true)]
    pub root: Option<PathBuf>,

    /// compile_commands.json 파일 경로 (기본값: <root>/compile_commands.json)
    #[arg(
        short = 'c',
        long = "compile-commands",
        value_name = "PATH",
        global = true
    )]
    pub compile_commands: Option<PathBuf>,

    /// 출력 결과 포맷 (text, json, mermaid, dot)
    #[arg(
        short = 'f',
        long = "format",
        value_enum,
        default_value_t = OutputFormat::Text,
        global = true
    )]
    pub format: OutputFormat,

    /// 결과를 파일로 직접 저장할 경로 (선택적)
    #[arg(short = 'o', long = "output", value_name = "FILE", global = true)]
    pub output: Option<PathBuf>,

    /// 타이밍 및 메모리 등 성능 메트릭 상세 출력
    #[arg(long = "metrics", default_value_t = false, global = true)]
    pub metrics: bool,

    /// 상세 분석 메트릭 및 번역 단위(TU) 파일 목록 출력
    #[arg(long = "verbose", default_value_t = false, global = true)]
    pub verbose: bool,
}

/// 탐색 제어 옵션 (Traversal Options)
#[derive(Debug, Args, Clone)]
pub struct TraversalOptions {
    /// 최대 탐색 깊이 제한 (지정하지 않을 경우 무제한 탐색)
    #[arg(short = 'd', long = "max-depth", value_name = "N")]
    pub max_depth: Option<usize>,

    /// 시맨틱 검증으로 완전히 확정(CONFIRMED)된 엣지만 결과에 포함
    #[arg(long = "verified-only", default_value_t = false)]
    pub verified_only: bool,

    /// 미해결(UNRESOLVED) 엣지를 결과에서 제외
    #[arg(long = "no-unresolved", default_value_t = false)]
    pub no_unresolved: bool,

    /// 외부 라이브러리(Foreign) 호출 엣지를 결과에서 제외
    #[arg(long = "no-foreign", default_value_t = false)]
    pub no_foreign: bool,
}

/// 서브커맨드 정의 (Commands)
#[derive(Debug, Subcommand, Clone)]
pub enum Commands {
    /// 메서드 하나를 받아 최상위 호출자에서 해당 메서드까지의 경로를 자동 탐색
    Trace {
        /// 도달 경로를 찾을 메서드 또는 함수 이름
        #[arg(value_name = "METHOD")]
        target: String,

        #[command(flatten)]
        common: CommonOptions,

        #[command(flatten)]
        traversal: TraversalOptions,
    },

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
    /// 파싱된 CLI로부터 ProjectInput, QueryRequest, RenderOptions 추출
    pub fn into_execution_plan(
        self,
    ) -> Result<(ProjectInput, QueryRequest, crate::render::RenderOptions), InputError> {
        let (common, request, no_unresolved, no_foreign) = match self.command {
            Commands::Trace {
                target,
                common,
                traversal,
            } => {
                let target_query = SymbolQuery::parse(&target);
                (
                    common,
                    QueryRequest::Trace {
                        target: target_query,
                        max_depth: traversal.max_depth,
                        verified_only: traversal.verified_only,
                    },
                    traversal.no_unresolved,
                    traversal.no_foreign,
                )
            }
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
                    traversal.no_unresolved,
                    traversal.no_foreign,
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
                    traversal.no_unresolved,
                    traversal.no_foreign,
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
                    traversal.no_unresolved,
                    traversal.no_foreign,
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
                    false,
                    false,
                )
            }
        };

        let render_options = crate::render::RenderOptions {
            format: common.format,
            output_file: common.output,
            verbose: common.verbose,
            show_metrics: common.metrics,
            no_unresolved,
            no_foreign,
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
            render_options,
        ))
    }

    /// 하위 호환성용 기존 메서드
    pub fn into_request(self) -> Result<(ProjectInput, QueryRequest), InputError> {
        let (input, req, _) = self.into_execution_plan()?;
        Ok((input, req))
    }
}
