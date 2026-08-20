//! 시맨틱 검증 계약 및 추상화 모듈
//! Semantic verification provider contract and abstraction module

pub mod clang;

use crate::diagnostic::AnalysisIssue;
use crate::model::{CallEdge, CandidateCallId, CandidateSymbolId, CompilationKey, Symbol};
use crate::project::ProjectContext;

/// 심볼 해석 배치 결과 (Resolution Batch)
#[derive(Debug, Clone, Default)]
pub struct ResolutionBatch {
    /// 해석된 정규 심볼 목록
    pub symbols: Vec<Symbol>,
    /// 발생한 분석 이슈 목록
    pub issues: Vec<AnalysisIssue>,
}

/// 검증 요청 배치 (Verification Batch)
#[derive(Debug, Clone)]
pub struct VerificationBatch {
    /// 적용할 컴파일 컨텍스트 키
    pub context: CompilationKey,
    /// 대상 후보 심볼 목록
    pub symbols: Vec<CandidateSymbolId>,
    /// 검증할 후보 호출 목록
    pub calls: Vec<CandidateCallId>,
}

/// 엣지 검증 결과 (Verification Result)
#[derive(Debug, Clone, Default)]
pub struct VerificationResult {
    /// 검증된 엣지 목록
    pub edges: Vec<CallEdge>,
    /// 발생한 분석 이슈 목록
    pub issues: Vec<AnalysisIssue>,
}

/// 시맨틱 공급자 내부 계약 (Semantic Provider Trait)
pub trait SemanticProvider {
    /// 후보 심볼들을 정규 심볼로 해석
    fn resolve_symbols(
        &mut self,
        project: &ProjectContext,
        candidates: &[CandidateSymbolId],
    ) -> ResolutionBatch;

    /// 후보 호출 배치에 대해 Clang 시맨틱 검증 수행
    fn verify_calls(
        &mut self,
        project: &ProjectContext,
        batch: VerificationBatch,
    ) -> VerificationResult;
}
