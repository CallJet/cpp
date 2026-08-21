use serde::Serialize;
use std::fmt;
use std::path::PathBuf;

use crate::model::{CompilationKey, SourceLocation};

/// 진단 심각도 (Diagnostic Severity)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum Severity {
    /// 치명적 오류 (Fatal Error)
    Fatal,
    /// 복구 가능한 문제 (Recoverable Issue)
    Recoverable,
}

/// 시스템 내부 오류 (Internal Error)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum InternalError {
    /// 불변식 위반 (Invariant Violation)
    InvariantViolation(String),
    /// 알 수 없는 내부 오류 (Unknown Internal Error)
    Other(String),
}

impl fmt::Display for InternalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvariantViolation(msg) => {
                write!(f, "내부 불변식 위반(Invariant Violation): {msg}")
            }
            Self::Other(msg) => write!(f, "내부 오류(Internal Error): {msg}"),
        }
    }
}

impl std::error::Error for InternalError {}

/// 입력 유효성 검증 오류 (Input Validation Error)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum InputError {
    /// 소스 루트 경로가 존재하지 않거나 디렉토리가 아님
    InvalidSourceRoot { path: PathBuf, reason: String },
    /// 컴파일 데이터베이스 파일이 없거나 읽을 수 없음
    InvalidCompilationDatabase { path: PathBuf, reason: String },
    /// 잘못된 CLI 인자 또는 옵션
    InvalidCliArgument { argument: String, reason: String },
    /// 입출력 오류 (I/O Error)
    IoError { path: PathBuf, reason: String },
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSourceRoot { path, reason } => {
                write!(
                    f,
                    "유효하지 않은 소스 루트(Invalid Source Root) '{}': {}",
                    path.display(),
                    reason
                )
            }
            Self::InvalidCompilationDatabase { path, reason } => {
                write!(
                    f,
                    "유효하지 않은 컴파일 데이터베이스(Invalid Compilation DB) '{}': {}",
                    path.display(),
                    reason
                )
            }
            Self::InvalidCliArgument { argument, reason } => {
                write!(
                    f,
                    "잘못된 CLI 인자(Invalid CLI Argument) '{}': {}",
                    argument, reason
                )
            }
            Self::IoError { path, reason } => {
                write!(f, "I/O 오류 발생 경로 '{}': {}", path.display(), reason)
            }
        }
    }
}

impl std::error::Error for InputError {}

/// 쿼리 대상 식별 오류 (Query Target Error)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum QueryError {
    /// 일치하는 심볼을 찾을 수 없음 (Symbol Not Found)
    SymbolNotFound { query: String },
    /// 일치하는 심볼이 모호함 (Ambiguous Symbol)
    AmbiguousSymbol {
        query: String,
        candidates: Vec<String>,
    },
    /// 일치하는 호출 엣지가 없음 (No Matching Edge - for explain)
    EdgeNotFound { caller: String, callee: String },
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SymbolNotFound { query } => {
                write!(f, "심볼을 찾을 수 없습니다(Symbol Not Found): '{query}'")
            }
            Self::AmbiguousSymbol { query, candidates } => {
                write!(
                    f,
                    "심볼 쿼리가 모호합니다(Ambiguous Symbol): '{query}'. 가능한 후보: {}",
                    candidates.join(", ")
                )
            }
            Self::EdgeNotFound { caller, callee } => {
                write!(f, "호출자 '{caller}'와 피호출자 '{callee}' 사이의 엣지를 찾을 수 없습니다(Edge Not Found)")
            }
        }
    }
}

impl std::error::Error for QueryError {}

/// 분석 원인 구분 (Analysis Cause)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum AnalysisCause {
    /// 번역 단위(TU) 파싱 실패
    TranslationUnitParseFailed,
    /// 시맨틱 검증용 컴파일 컨텍스트 누락
    MissingCompilationContext,
    /// 소스 파일 읽기 실패
    SourceReadFailed,
    /// 기타 분석 문제
    Other(String),
}

/// 분석 진행 중 발생한 문제 (Analysis Issue)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnalysisIssue {
    /// 심각도
    pub severity: Severity,
    /// 연관된 컴파일 키 (선택적)
    pub context: Option<CompilationKey>,
    /// 발생 소스 위치 (선택적)
    pub location: Option<SourceLocation>,
    /// 진단 메시지
    pub message: String,
    /// 원인 분류
    pub cause: AnalysisCause,
}

impl fmt::Display for AnalysisIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{:?}] {}", self.severity, self.message)?;
        if let Some(loc) = &self.location {
            write!(f, " at {loc}")?;
        }
        if let Some(ctx) = &self.context {
            write!(f, " (context: {})", ctx.0)?;
        }
        Ok(())
    }
}

/// 통합 진단 열거형 (Diagnostic)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Diagnostic {
    /// 사용자 입력 오류
    Input(InputError),
    /// 쿼리 대상 식별 오류
    Query(QueryError),
    /// 분석 과정 중 문제 (메모리 절약을 위해 Box 사용)
    Analysis(Box<AnalysisIssue>),
    /// 내부 시스템 오류
    Internal(InternalError),
}

impl Diagnostic {
    /// AnalysisIssue로부터 Diagnostic 생성 헬퍼
    pub fn analysis(issue: AnalysisIssue) -> Self {
        Self::Analysis(Box::new(issue))
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input(e) => write!(f, "입력 오류(Input Error): {e}"),
            Self::Query(e) => write!(f, "쿼리 오류(Query Error): {e}"),
            Self::Analysis(e) => write!(f, "분석 문제(Analysis Issue): {e}"),
            Self::Internal(e) => write!(f, "내부 오류(Internal Error): {e}"),
        }
    }
}

impl std::error::Error for Diagnostic {}

/// 치명적 쿼리 실패 에러 (Fatal Error)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum FatalError {
    /// 입력 검증 실패
    Input(InputError),
    /// 대상 심볼 식별 실패
    Query(QueryError),
    /// 내부 치명적 불변식 위반
    Internal(InternalError),
    /// 분석 완전 실패
    AnalysisFailed(String),
}

impl fmt::Display for FatalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input(e) => write!(f, "{e}"),
            Self::Query(e) => write!(f, "{e}"),
            Self::Internal(e) => write!(f, "{e}"),
            Self::AnalysisFailed(msg) => write!(f, "분석 실패(Analysis Failed): {msg}"),
        }
    }
}

impl std::error::Error for FatalError {}

impl From<InputError> for FatalError {
    fn from(e: InputError) -> Self {
        Self::Input(e)
    }
}

impl From<QueryError> for FatalError {
    fn from(e: QueryError) -> Self {
        Self::Query(e)
    }
}

impl From<InternalError> for FatalError {
    fn from(e: InternalError) -> Self {
        Self::Internal(e)
    }
}
