//! 사용자 출력 렌더링 모듈
//! User output rendering module

use crate::model::{CallEdge, CallPath, Completion, Confidence, QueryResult};
use crate::project::ProjectContext;

/// 렌더링된 출력 결과 (Rendered Output)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedOutput {
    /// 표준 출력 문자열 (stdout)
    pub stdout: String,
    /// 표준 오류 문자열 (stderr)
    pub stderr: String,
    /// 권장 프로세스 종료 코드 (Exit code)
    pub exit_code: i32,
}

/// 결정론적 텍스트 렌더러 (Human-readable Deterministic Renderer)
#[derive(Debug, Default)]
pub struct HumanRenderer;

impl HumanRenderer {
    pub fn new() -> Self {
        Self
    }

    /// QueryResult를 받아 사람이 읽기 쉬운 텍스트로 렌더링
    pub fn render(&self, project: &ProjectContext, result: &QueryResult) -> RenderedOutput {
        let mut stdout = String::new();
        let mut stderr = String::new();

        // 1. 진단 메시지 출력 (stderr)
        for diag in &result.diagnostics {
            stderr.push_str(&format!("{diag}\n"));
        }

        // 2. 부분 분석 실패 안내 (FR-080, FR-081)
        if result.completion == Completion::Partial {
            stderr.push_str(
                "주의: 일부 분석 작업이 실패하여 부분 결과(Partial Result)만 표시됩니다.\n",
            );
        }

        // 3. 경로 결과가 있는 경우 경로 렌더링 (path 커맨드)
        if !result.paths.is_empty() {
            stdout.push_str("=== 호출 경로 (Call Path) ===\n");
            for (idx, path) in result.paths.iter().enumerate() {
                stdout.push_str(&format!("경로 #{}:\n", idx + 1));
                self.render_path(&mut stdout, project, path, result);
            }
        } else if !result.edges.is_empty() {
            // 4. 엣지 목록 렌더링 (callers, callees, explain)
            stdout.push_str("=== 호출 관계 (Call Edges) ===\n");
            for edge in &result.edges {
                self.render_edge(&mut stdout, project, edge, result);
            }
        }

        // 5. 완료 상태 및 요약 통계 출력
        stdout.push_str("\n--- 분석 결과 요약 ---\n");
        match &result.completion {
            Completion::Complete => {
                stdout.push_str("상태: 분석 완료 (Complete)\n");
            }
            Completion::NoResult => {
                stdout.push_str("상태: 결과 없음 (No Result)\n");
            }
            Completion::Truncated { max_depth } => {
                stdout.push_str(&format!(
                    "상태: 최대 깊이 도달로 잘림 (Truncated at depth {max_depth})\n"
                ));
            }
            Completion::Partial => {
                stdout.push_str("상태: 부분 완료 (Partial)\n");
            }
        }

        stdout.push_str(&format!(
            "통계: 총 심볼 {}개, 확정 엣지(CONFIRMED) {}개, 가능 엣지(POSSIBLE) {}개, 미해결 엣지(UNRESOLVED) {}개\n",
            result.counts.total_symbols,
            result.counts.confirmed_edges,
            result.counts.possible_edges,
            result.counts.unresolved_edges
        ));

        // 6. 프로세스 종료 코드 매핑 (FR-070, FR-079, FR-081)
        // Complete, NoResult, Truncated, Unresolved(에러 없는 경우) -> 0
        // Partial -> 1
        let exit_code = match result.completion {
            Completion::Complete | Completion::NoResult | Completion::Truncated { .. } => 0,
            Completion::Partial => 1,
        };

        RenderedOutput {
            stdout,
            stderr,
            exit_code,
        }
    }

    /// 단일 경로 렌더링
    fn render_path(
        &self,
        out: &mut String,
        project: &ProjectContext,
        path: &CallPath,
        result: &QueryResult,
    ) {
        for (i, node_id) in path.nodes.iter().enumerate() {
            let sym_name = if let Some(sym) = result.symbols.get(node_id) {
                sym.display_name().to_string()
            } else {
                format!("{:?}", node_id)
            };

            let loc_str = if let Some(sym) = result.symbols.get(node_id) {
                if let Some(decl) = &sym.declaration {
                    format!(" ({})", self.format_location(project, decl))
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            out.push_str(&format!("  [{i}] {sym_name}{loc_str}\n"));

            if i < path.edges.len() {
                let edge_id = path.edges[i];
                if let Some(edge) = result.edges.iter().find(|e| e.id == edge_id) {
                    let conf_str = format!("[{}]", edge.confidence);
                    let kind_str = format!("({})", edge.kind);
                    let loc_str = self.format_range(project, &edge.callsite);
                    out.push_str(&format!("      ↓ {conf_str} {kind_str} at {loc_str}\n"));
                } else {
                    out.push_str("      ↓ [call]\n");
                }
            }
        }
    }

    /// 단일 호출 엣지 및 증거 렌더링
    fn render_edge(
        &self,
        out: &mut String,
        project: &ProjectContext,
        edge: &CallEdge,
        result: &QueryResult,
    ) {
        let caller_name = result
            .symbols
            .get(&edge.caller)
            .map(|s| s.display_name().to_string())
            .unwrap_or_else(|| format!("{:?}", edge.caller));

        let callee_name = match &edge.callee {
            Some(cid) => result
                .symbols
                .get(cid)
                .map(|s| s.display_name().to_string())
                .unwrap_or_else(|| format!("{:?}", cid)),
            None => "<unresolved>".to_string(),
        };

        let conf_tag = match edge.confidence {
            Confidence::Confirmed => "[CONFIRMED]",
            Confidence::Possible => "[POSSIBLE]",
            Confidence::Unresolved => "[UNRESOLVED]",
        };

        let loc_str = self.format_range(project, &edge.callsite);

        out.push_str(&format!(
            "• {conf_tag} {caller_name} -> {callee_name} ({}, at {loc_str})\n",
            edge.kind
        ));

        // 검증 증거(Evidence) 세부 정보 (explain 및 상세 보기 지원)
        for (ctx_key, evidence) in &edge.evidence_by_context {
            if let Some(expr) = &evidence.expression_text {
                out.push_str(&format!("    표현식: `{expr}`\n"));
            }
            out.push_str(&format!(
                "    사유: {:?}, 컨텍스트: {}\n",
                evidence.reason, ctx_key.0
            ));
            if evidence.is_virtual {
                out.push_str("    가상 함수 디스패치(Virtual Dispatch)\n");
            }
        }
    }

    /// 소스 위치 포맷팅 (프로젝트 루트 기준 상대 경로 변환)
    fn format_location(
        &self,
        project: &ProjectContext,
        loc: &crate::model::SourceLocation,
    ) -> String {
        let display_file = project.display_path(&loc.file);
        if let Some(pt) = &loc.point {
            format!("{}:{}", display_file.display(), pt)
        } else {
            format!("{}", display_file.display())
        }
    }

    /// 소스 범위 포맷팅
    fn format_range(&self, project: &ProjectContext, range: &crate::model::SourceRange) -> String {
        let start_str = self.format_location(project, &range.start);
        if let Some(end) = &range.end {
            if let Some(pt) = &end.point {
                format!("{start_str}-{pt}")
            } else {
                start_str
            }
        } else {
            start_str
        }
    }
}
