use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use crate::diagnostic::Diagnostic;

/// 분석 대상 언어 (Programming Language)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum Language {
    /// C 언어
    C,
    /// C++ 언어
    Cpp,
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::C => write!(f, "C"),
            Self::Cpp => write!(f, "C++"),
        }
    }
}

/// 1부터 시작하는 소스 행/열 위치 (1-based Line/Column coordinates)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct LineColumn {
    /// 행 번호 (1-based)
    pub line: u32,
    /// 열 번호 (1-based)
    pub column: u32,
}

impl fmt::Display for LineColumn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

/// 소스 위치 (Source Location)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct SourceLocation {
    /// 파일 경로 (정규화된 경로)
    pub file: PathBuf,
    /// 행 및 열 (좌표가 없는 경우 None)
    pub point: Option<LineColumn>,
}

impl SourceLocation {
    /// 파일과 행/열로 새 소스 위치 생성
    pub fn new(file: impl Into<PathBuf>, line: u32, column: u32) -> Self {
        Self {
            file: file.into(),
            point: Some(LineColumn { line, column }),
        }
    }

    /// 파일 경로만 있는 소스 위치 생성 (위치 좌표 부재)
    pub fn file_only(file: impl Into<PathBuf>) -> Self {
        Self {
            file: file.into(),
            point: None,
        }
    }
}

impl fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.file.display())?;
        if let Some(pt) = &self.point {
            write!(f, ":{}", pt)?;
        }
        Ok(())
    }
}

/// 소스 범위 (Source Range)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct SourceRange {
    /// 시작 위치 (Start Location)
    pub start: SourceLocation,
    /// 종료 위치 (End Location, 선택적)
    pub end: Option<SourceLocation>,
}

impl SourceRange {
    /// 단일 위치로부터 범위 생성
    pub fn single(loc: SourceLocation) -> Self {
        Self {
            start: loc,
            end: None,
        }
    }

    /// 시작과 끝 위치로 범위 생성
    pub fn spanned(start: SourceLocation, end: SourceLocation) -> Self {
        Self {
            start,
            end: Some(end),
        }
    }
}

impl fmt::Display for SourceRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.start)?;
        if let Some(end) = &self.end {
            if let Some(pt) = &end.point {
                write!(f, "-{}", pt)?;
            }
        }
        Ok(())
    }
}

/// 구문적 후보 심볼 식별자 (Candidate Symbol ID)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct CandidateSymbolId(pub u32);

/// 후보 심볼 종류 (Syntactic Kind)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum CandidateSymbolKind {
    /// 일반 함수 (Free Function)
    Function,
    /// 클래스/구조체 메서드 (Method)
    Method,
    /// 생성자/소멸자
    ConstructorOrDestructor,
}

/// Tree-sitter 구문 분석으로 발견된 후보 심볼 (Candidate Symbol)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CandidateSymbol {
    /// 후보 심볼 고유 식별자
    pub id: CandidateSymbolId,
    /// 언어 종류
    pub language: Language,
    /// 구문상 심볼 종류 (함수, 메서드 등)
    pub syntactic_kind: CandidateSymbolKind,
    /// 심볼 단말 이름 (Terminal Name)
    pub name: String,
    /// 네임스페이스 등 한정자 힌트 (Qualifier Hint)
    pub qualifier_hint: Option<String>,
    /// 시그니처 힌트 (Signature Hint)
    pub signature_hint: Option<String>,
    /// 소유 클래스/구조체 힌트 (Owner Hint)
    pub owner_hint: Option<String>,
    /// 선언부 소스 범위 (Declaration Range)
    pub declaration: SourceRange,
    /// 정의부 소스 범위 (Definition Body Range)
    pub definition_body: Option<SourceRange>,
    /// 구문 완성 여부 (Syntax Complete)
    pub syntax_complete: bool,
}

/// 후보 심볼 중복 제거용 키 (Candidate Symbol Deduplication Key)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct CandidateSymbolKey {
    pub file: PathBuf,
    pub declaration_range: SourceRange,
    pub syntactic_kind: CandidateSymbolKind,
}

/// 구문적 후보 호출 식별자 (Candidate Call ID)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct CandidateCallId(pub u32);

/// 후보 호출 구문 힌트 종류 (Candidate Call Kind Hint)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum CandidateCallKind {
    /// 일반 함수 호출
    Direct,
    /// 멤버 함수 호출 (obj.func() or ptr->func())
    Member,
    /// 한정된 호출 (Namespace::func())
    Qualified,
    /// 기타/알 수 없음
    Other,
}

/// 구문 분석으로 발견된 후보 호출 위치 (Candidate Call Site)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CandidateCallSite {
    /// 후보 호출 식별자
    pub id: CandidateCallId,
    /// 이 호출을 포함하고 있는 후보 심볼 ID (Caller)
    pub caller: CandidateSymbolId,
    /// 피호출자 표기 문자열 (Callee Spelling)
    pub callee_spelling: String,
    /// 피호출자 단말 이름의 정확한 소스 위치
    pub callee_location: Option<SourceLocation>,
    /// 한정자 힌트 (Qualifier Hint)
    pub qualifier_hint: Option<String>,
    /// 호출 표현식 소스 범위 (Expression Range)
    pub expression: SourceRange,
    /// 호출 표현식 원문 텍스트 (선택적)
    pub expression_text: Option<String>,
    /// 구문 힌트 종류
    pub syntax_hint: CandidateCallKind,
    /// 구문 완성 여부
    pub syntax_complete: bool,
}

/// 후보 호출 중복 제거용 키 (Candidate Call Deduplication Key)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct CandidateCallKey {
    pub file: PathBuf,
    pub expression_range: SourceRange,
    pub enclosing_symbol: CandidateSymbolId,
    pub callee_spelling: String,
}

/// 백엔드 정규 심볼 식별자 (Backend Canonical Symbol ID)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum BackendSymbolId {
    /// Clang USR (Unified Symbol Resolution) 문자열
    ClangUsr(String),
    /// USR 부재 시 위치 기반 폴백 식별자
    ClangLocationFallback {
        canonical_declaration: SourceLocation,
        cursor_kind: String,
        qualified_name: String,
        signature: Option<String>,
    },
    /// Clang 검증을 사용할 수 없을 때의 Tree-sitter 위치 기반 식별자
    TreeSitterLocationFallback {
        declaration: SourceLocation,
        syntactic_kind: String,
        qualified_name: String,
    },
}

/// 언어 중립적 정규 심볼 식별자 (Canonical Symbol ID)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct SymbolId {
    /// 언어 종류
    pub language: Language,
    /// 백엔드 식별자
    pub backend_id: BackendSymbolId,
}

impl SymbolId {
    /// Clang USR 기반 심볼 ID 생성
    pub fn clang_usr(language: Language, usr: impl Into<String>) -> Self {
        Self {
            language,
            backend_id: BackendSymbolId::ClangUsr(usr.into()),
        }
    }

    /// Tree-sitter 후보의 위치와 한정 이름으로 안정적인 fallback ID 생성
    pub fn tree_sitter_fallback(
        language: Language,
        declaration: SourceLocation,
        syntactic_kind: impl Into<String>,
        qualified_name: impl Into<String>,
    ) -> Self {
        Self {
            language,
            backend_id: BackendSymbolId::TreeSitterLocationFallback {
                declaration,
                syntactic_kind: syntactic_kind.into(),
                qualified_name: qualified_name.into(),
            },
        }
    }
}

/// 정규화된 심볼 정보 (Canonical Symbol)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Symbol {
    /// 정규 심볼 ID
    pub id: SymbolId,
    /// 단말 이름 (Name)
    pub name: String,
    /// 네임스페이스 및 소유자를 포함한 정규화된 이름 (Qualified Name)
    pub qualified_name: Option<String>,
    /// 소속 네임스페이스 (`outer::inner` 형식)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// 소유 클래스/구조체 (`Outer::Inner` 형식)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class_name: Option<String>,
    /// 함수 시그니처 (Signature)
    pub signature: Option<String>,
    /// 선언 위치 (Declaration Location)
    pub declaration: Option<SourceLocation>,
    /// 정의 위치 (Definition Location)
    pub definition: Option<SourceLocation>,
}

impl Symbol {
    /// 표시용 이름 반환 (한정 이름 우선, 없으면 기본 이름)
    pub fn display_name(&self) -> &str {
        if let Some(qn) = &self.qualified_name {
            qn
        } else {
            &self.name
        }
    }
}

/// 호출 신뢰도 상태 (Call Confidence State)
/// 주의: initial CallJet에서는 PROBABLE 상태가 없으며(FR-076), 오직 3가지 상태만 존재합니다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum Confidence {
    /// 시맨틱 증거로 고유한 피호출자가 완전히 확정됨 (Confirmed)
    Confirmed,
    /// 구문상 가능한 후보이거나 런타임 대상이 유일하게 확정되지 않음 (Possible)
    Possible,
    /// 사용 가능한 구문/시맨틱 근거로 대상 식별자를 찾을 수 없음 (Unresolved)
    Unresolved,
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Confirmed => write!(f, "CONFIRMED"),
            Self::Possible => write!(f, "POSSIBLE"),
            Self::Unresolved => write!(f, "UNRESOLVED"),
        }
    }
}

/// 호출 형태 분류 (Call Kind)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum CallKind {
    /// 직접 호출 (Direct Call)
    Direct,
    /// 가상 함수 디스패치 호출 (Virtual Call)
    Virtual,
    /// 함수 포인터를 통한 간접 호출 (Function Pointer Call)
    FunctionPointer,
    /// 템플릿 인스턴스화 관련 호출 (Template Call)
    Template,
    /// 매크로 확장에 의해 정의된 호출 (Macro Expanded Call)
    MacroExpanded,
    /// 언어/프로젝트 경계를 넘는 외부 호출 (Foreign Call)
    Foreign,
    /// 호출 형태를 판별할 수 없음 (Unresolved Call)
    Unresolved,
}

impl fmt::Display for CallKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Direct => write!(f, "direct"),
            Self::Virtual => write!(f, "virtual"),
            Self::FunctionPointer => write!(f, "function_pointer"),
            Self::Template => write!(f, "template"),
            Self::MacroExpanded => write!(f, "macro_expanded"),
            Self::Foreign => write!(f, "foreign"),
            Self::Unresolved => write!(f, "unresolved"),
        }
    }
}

/// 검증 근거 사유 (Verification Reason)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum VerificationReason {
    /// 정확한 정규 참조 일치
    ExactReference,
    /// 다중 런타임 대상 가능 (가상 함수 등)
    MultipleRuntimeTargets,
    /// 간접 호출 대상 불명
    IndirectTargetUnknown,
    /// 커서를 찾을 수 없음
    CursorNotFound,
    /// 모호한 참조
    AmbiguousReference,
    /// 외부 경계
    ForeignBoundary,
    /// Clang 검증 없이 유지된 Tree-sitter 구문 후보
    SyntacticCandidate,
}

/// 시맨틱 진단 정보
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SemanticDiagnostic {
    pub message: String,
    pub location: Option<SourceLocation>,
}

/// 검증 근거 및 출처 정보 (Verification Evidence)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VerificationEvidence {
    /// 호출 표현식 원문 (선택적)
    pub expression_text: Option<String>,
    /// 정적 대상 심볼 (선택적)
    pub static_target: Option<Symbol>,
    /// 가능한 후보 대상 심볼 목록 (가상 함수 등)
    pub candidate_targets: Vec<Symbol>,
    /// Clang 진단 메시지들
    pub clang_diagnostics: Vec<SemanticDiagnostic>,
    /// 검증 사유
    pub reason: VerificationReason,
    /// 철자 위치 (Spelling Location)
    pub spelling_location: Option<SourceLocation>,
    /// 매크로 확장 위치 (Expansion Location)
    pub expansion_location: Option<SourceLocation>,
    /// 가상 호출 여부
    pub is_virtual: bool,
    /// 템플릿 관련 여부
    pub is_template_related: bool,
    /// 매크로 확장 여부
    pub is_macro_expanded: bool,
}

/// 컴파일 컨텍스트 고유 키 (Compilation Key)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct CompilationKey(pub String);

/// 컴파일 컨텍스트 (Compilation Context)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompilationContext {
    /// 고유 키
    pub key: CompilationKey,
    /// 작업 디렉토리
    pub directory: PathBuf,
    /// 소스 파일 경로
    pub source_file: PathBuf,
    /// 정규화된 Clang 인자 목록
    pub clang_args: Vec<std::ffi::OsString>,
}

/// 검증된 호출 엣지 식별자 (Call Edge ID)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct CallEdgeId(pub u32);

/// 검증된 호출 엣지 (Call Edge)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CallEdge {
    /// 엣지 고유 번호
    pub id: CallEdgeId,
    /// 호출자 심볼 ID (Caller)
    pub caller: SymbolId,
    /// 피호출자 심볼 ID (Callee, 대상 불명 시 None)
    pub callee: Option<SymbolId>,
    /// 호출 위치 범위 (Callsite Range)
    pub callsite: SourceRange,
    /// 호출 종류 (Call Kind)
    pub kind: CallKind,
    /// 신뢰도 (Confidence)
    pub confidence: Confidence,
    /// 기여한 컴파일 컨텍스트 키 집합 (Context Provenance)
    pub contexts: BTreeSet<CompilationKey>,
    /// 컨텍스트별 검증 근거 맵
    pub evidence_by_context: BTreeMap<CompilationKey, VerificationEvidence>,
}

/// 검증된 엣지 중복 판정 키 (Verified Edge Key)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct VerifiedEdgeKey {
    pub caller: SymbolId,
    pub callee: Option<SymbolId>,
    pub callsite: SourceRange,
    pub kind: CallKind,
    pub confidence: Confidence,
}

/// 사용자 심볼 쿼리 (Symbol Query from user)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SymbolQuery {
    /// 원본 쿼리 문자열
    pub raw: String,
    /// 한정자 접두어 힌트 (예: "ns::Class::")
    pub qualifier_hint: Option<String>,
    /// 단말 이름 (예: "targetFunc")
    pub terminal_name: String,
}

impl SymbolQuery {
    /// 쿼리 문자열로부터 SymbolQuery 파싱
    pub fn parse(input: &str) -> Self {
        let trimmed = input.trim();
        if let Some(pos) = trimmed.rfind("::") {
            let qualifier = &trimmed[..pos + 2];
            let name = &trimmed[pos + 2..];
            Self {
                raw: trimmed.to_string(),
                qualifier_hint: Some(qualifier.to_string()),
                terminal_name: name.to_string(),
            }
        } else {
            Self {
                raw: trimmed.to_string(),
                qualifier_hint: None,
                terminal_name: trimmed.to_string(),
            }
        }
    }
}

/// CLI 쿼리 요청 종류 (Query Request)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum QueryRequest {
    /// 단일 대상까지 도달하는 호출 경로 자동 탐색 (Trace)
    Trace {
        target: SymbolQuery,
        max_depth: Option<usize>,
        verified_only: bool,
    },
    /// 호출자 탐색 (Callers)
    Callers {
        target: SymbolQuery,
        max_depth: Option<usize>,
        verified_only: bool,
    },
    /// 피호출자 탐색 (Callees)
    Callees {
        source: SymbolQuery,
        max_depth: Option<usize>,
        verified_only: bool,
    },
    /// 경로 탐색 (Path)
    Path {
        source: SymbolQuery,
        target: SymbolQuery,
        max_depth: Option<usize>,
        verified_only: bool,
    },
    /// 엣지 설명 (Explain)
    Explain {
        caller: SymbolQuery,
        callee: SymbolQuery,
    },
}

/// 호출 경로 노드 및 엣지 시퀀스 (Call Path)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CallPath {
    /// 노드 시퀀스 (Symbols)
    pub nodes: Vec<SymbolId>,
    /// 엣지 시퀀스 (Edges): nodes.len() == edges.len() + 1
    pub edges: Vec<CallEdgeId>,
}

/// 쿼리 완료 상태 (Completion Status)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Completion {
    /// 온전하게 탐색 완료 (Complete)
    Complete,
    /// 탐색 결과 없음 (No Result - 정상 완료)
    NoResult,
    /// 최대 깊이 제한에 도달하여 잘림 (Truncated - 정상 완료)
    Truncated { max_depth: usize },
    /// 일부 분석 작업 실패가 포함된 부분 결과 (Partial Result - 비정상 종료 코드 필요)
    Partial,
}

/// 결과 건수 요약 (Result Counts)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct ResultCounts {
    /// 발견된 총 심볼 수
    pub total_symbols: usize,
    /// 검증 확정된 엣지 수 (CONFIRMED)
    pub confirmed_edges: usize,
    /// 가능 엣지 수 (POSSIBLE)
    pub possible_edges: usize,
    /// 미해결 엣지 수 (UNRESOLVED)
    pub unresolved_edges: usize,
    /// 발견된 경로 수
    pub paths_count: usize,
}

/// 쿼리 실행 메트릭 (Query Performance Metrics)
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct QueryMetrics {
    /// 검사된 소스 파일 수
    pub source_files_inspected: usize,
    /// Tree-sitter로 파싱된 소스 파일 수
    pub source_files_parsed_by_treesitter: usize,
    /// 발견된 후보 호출 위치 수
    pub candidate_call_sites: usize,
    /// 사용 가능한 총 번역 단위 수 (Available TUs)
    pub available_translation_units: usize,
    /// 후보가 포함된 번역 단위 수 (Candidate TUs)
    pub candidate_translation_units: usize,
    /// 시맨틱 검증을 수행한 번역 단위 수 (Verified TUs)
    pub verified_translation_units: usize,
    /// Clang TU 파싱 횟수
    pub clang_tu_parses: usize,
    /// TU 캐시 히트 횟수
    pub tu_cache_hits: usize,
    /// 검증된 시맨틱 후보 수
    pub semantic_candidates_verified: usize,
    /// 탐색 단계 소요 시간
    pub discovery_time: Duration,
    /// 시맨틱 검증 소요 시간
    pub verification_time: Duration,
    /// 순수 경로 순회 소요 시간
    pub traversal_time: Duration,
    /// 총 쿼리 소요 시간
    pub total_query_time: Duration,
    /// 대략적/최대 메모리 사용량 (Bytes)
    pub peak_resident_memory_bytes: u64,
    /// 시맨틱 검증이 수행된 소스 파일 목록 (Verified Source Files)
    pub verified_source_files: Vec<PathBuf>,
    /// 시맨틱 파싱을 회피(스킵)한 소스 파일 목록 (Skipped Source Files)
    pub skipped_source_files: Vec<PathBuf>,
}

/// 통합 쿼리 결과 (Query Result)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QueryResult {
    /// 완료 상태
    pub completion: Completion,
    /// 조회된 심볼 맵 (ID -> Symbol)
    pub symbols: BTreeMap<SymbolId, Symbol>,
    /// 결과 호출 엣지 목록
    pub edges: Vec<CallEdge>,
    /// 결과 경로 목록
    pub paths: Vec<CallPath>,
    /// 결과 수 통계
    pub counts: ResultCounts,
    /// 수집된 진단 목록
    pub diagnostics: Vec<Diagnostic>,
    /// 성능 메트릭
    pub metrics: QueryMetrics,
}
