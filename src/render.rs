//! 사용자 출력 렌더링 모듈
//! User output rendering module

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use crate::cli::OutputFormat;
use crate::model::{
    BackendSymbolId, CallEdge, CallKind, CallPath, Completion, Confidence, QueryResult, SymbolId,
};
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

/// 렌더링 옵션 (Render Options)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderOptions {
    /// 출력 포맷
    pub format: OutputFormat,
    /// 파일 저장 경로
    pub output_file: Option<PathBuf>,
    /// 상세 번역 단위(TU) 출력 여부
    pub verbose: bool,
    /// 성능 메트릭 출력 여부
    pub show_metrics: bool,
    /// 미해결(UNRESOLVED) 엣지 숨기기
    pub no_unresolved: bool,
    /// 외부 라이브러리(Foreign) 엣지 숨기기
    pub no_foreign: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            format: OutputFormat::Text,
            output_file: None,
            verbose: false,
            show_metrics: false,
            no_unresolved: false,
            no_foreign: false,
        }
    }
}

/// 결정론적 텍스트 렌더러 (Human-readable Deterministic Renderer)
#[derive(Debug, Default)]
pub struct HumanRenderer;

impl HumanRenderer {
    pub fn new() -> Self {
        Self
    }

    /// QueryResult를 받아 기본 옵션으로 렌더링
    pub fn render(&self, project: &ProjectContext, result: &QueryResult) -> RenderedOutput {
        self.render_with_options(project, result, RenderOptions::default())
    }

    /// QueryResult와 RenderOptions를 받아 렌더링
    pub fn render_with_options(
        &self,
        project: &ProjectContext,
        result: &QueryResult,
        options: RenderOptions,
    ) -> RenderedOutput {
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

        // 3. 포맷별 stdout 생성
        let stdout = match options.format {
            OutputFormat::Json => self.render_json(result),
            OutputFormat::Mermaid => self.render_mermaid(project, result, &options),
            OutputFormat::Dot => self.render_dot(result, &options),
            OutputFormat::Text => self.render_text(project, result, &options),
        };

        // 4. 프로세스 종료 코드 매핑 (FR-070, FR-079, FR-081)
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

    /// JSON 형식으로 렌더링
    fn render_json(&self, result: &QueryResult) -> String {
        serde_json::to_string_pretty(result)
            .unwrap_or_else(|e| format!(r#"{{"error": "JSON 직렬화 실패: {}"}}"#, e))
    }

    /// Mermaid 다이어그램 형식으로 렌더링 (Markdown flowchart)
    fn render_mermaid(
        &self,
        project: &ProjectContext,
        result: &QueryResult,
        options: &RenderOptions,
    ) -> String {
        let mut out = String::from("```mermaid\nflowchart TD\n");

        for (sym_id, sym) in &result.symbols {
            let safe_id = self.symbol_safe_id(sym_id);
            let name = sym.display_name();
            let loc_str = if let Some(decl) = &sym.declaration {
                format!("\\n{}", self.format_location(project, decl))
            } else {
                String::new()
            };
            out.push_str(&format!("    {safe_id}[\"{name}{loc_str}\"]\n"));
        }

        for edge in &result.edges {
            if options.no_unresolved && edge.confidence == Confidence::Unresolved {
                continue;
            }
            if options.no_foreign && edge.kind == CallKind::Foreign {
                continue;
            }

            let caller_safe = self.symbol_safe_id(&edge.caller);
            if let Some(callee_id) = &edge.callee {
                let callee_safe = self.symbol_safe_id(callee_id);
                let label = format!("{} ({})", edge.confidence, edge.kind);
                out.push_str(&format!("    {caller_safe} -->|{label}| {callee_safe}\n"));
            }
        }

        out.push_str("```\n");
        out
    }

    /// Graphviz DOT 형식으로 렌더링
    fn render_dot(&self, result: &QueryResult, options: &RenderOptions) -> String {
        let mut out = String::from(
            "digraph CallGraph {\n    node [shape=box, fontname=\"Helvetica\"];\n    edge [fontname=\"Helvetica\", fontsize=10];\n\n",
        );

        for (sym_id, sym) in &result.symbols {
            let safe_id = self.symbol_safe_id(sym_id);
            let label = sym.display_name().replace('"', "\\\"");
            out.push_str(&format!("    {safe_id} [label=\"{label}\"];\n"));
        }

        for edge in &result.edges {
            if options.no_unresolved && edge.confidence == Confidence::Unresolved {
                continue;
            }
            if options.no_foreign && edge.kind == CallKind::Foreign {
                continue;
            }

            let caller_safe = self.symbol_safe_id(&edge.caller);
            if let Some(callee_id) = &edge.callee {
                let callee_safe = self.symbol_safe_id(callee_id);
                let label = format!("{} [{}]", edge.kind, edge.confidence);
                out.push_str(&format!(
                    "    {caller_safe} -> {callee_safe} [label=\"{label}\"];\n"
                ));
            }
        }

        out.push_str("}\n");
        out
    }

    /// 사람이 읽기 쉬운 텍스트 형식으로 렌더링
    fn render_text(
        &self,
        project: &ProjectContext,
        result: &QueryResult,
        options: &RenderOptions,
    ) -> String {
        let mut stdout = String::new();

        // 1. 경로 결과가 있는 경우 경로 렌더링 (path 커맨드)
        if !result.paths.is_empty() {
            stdout.push_str("=== 호출 경로 (Call Path) ===\n");
            for (idx, path) in result.paths.iter().enumerate() {
                stdout.push_str(&format!("경로 #{}:\n", idx + 1));
                self.render_path(&mut stdout, project, path, result);
            }
        } else if !result.edges.is_empty() {
            // 2. 엣지 목록 렌더링 (callers, callees, explain)
            stdout.push_str("=== 호출 관계 (Call Edges) ===\n");
            for edge in &result.edges {
                if options.no_unresolved && edge.confidence == Confidence::Unresolved {
                    continue;
                }
                if options.no_foreign && edge.kind == CallKind::Foreign {
                    continue;
                }
                self.render_edge(&mut stdout, project, edge, result);
            }
        }

        // 3. 완료 상태 및 요약 통계 출력
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

        // verbose 모드일 때 상세 번역 단위(TU) 파일 목록 출력
        if options.verbose {
            let verified_count = result.metrics.verified_source_files.len();
            let skipped_count = result.metrics.skipped_source_files.len();
            let total_count = verified_count + skipped_count;

            stdout.push_str(&format!(
                "\n[상세 번역 단위(TU) 리포트]\n• 번역 단위: 총 {total_count}개 중 {verified_count}개 시맨틱 검증, {skipped_count}개 파싱 생략(Skipped)\n"
            ));

            if !result.metrics.verified_source_files.is_empty() {
                stdout.push_str("• 시맨틱 검증된 소스 파일:\n");
                for f in &result.metrics.verified_source_files {
                    stdout.push_str(&format!("    - {}\n", project.display_path(f).display()));
                }
            }

            if !result.metrics.skipped_source_files.is_empty() {
                stdout.push_str("• 파싱 생략된 소스 파일:\n");
                for f in &result.metrics.skipped_source_files {
                    stdout.push_str(&format!("    - {}\n", project.display_path(f).display()));
                }
            }
        }

        // --metrics 플래그 지정 시 성능 지표 출력
        if options.show_metrics {
            stdout.push_str("\n[성능 및 비용 지표 (Performance Metrics)]\n");
            stdout.push_str(&format!(
                "• 총 소요 시간: {:?}\n",
                result.metrics.total_query_time
            ));
            stdout.push_str(&format!(
                "• Tree-sitter 탐색 시간: {:?}\n",
                result.metrics.discovery_time
            ));
            stdout.push_str(&format!(
                "• Clang 시맨틱 검증 시간: {:?}\n",
                result.metrics.verification_time
            ));
            stdout.push_str(&format!(
                "• Clang TU 파싱 횟수: {}회 (캐시 히트: {}회)\n",
                result.metrics.clang_tu_parses, result.metrics.tu_cache_hits
            ));
        }

        stdout
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

    /// 다이어그램용 안전한 노드 식별자 생성
    fn symbol_safe_id(&self, sym_id: &SymbolId) -> String {
        let mut hasher = DefaultHasher::new();
        match &sym_id.backend_id {
            BackendSymbolId::ClangUsr(usr) => usr.hash(&mut hasher),
            BackendSymbolId::ClangLocationFallback {
                canonical_declaration,
                qualified_name,
                ..
            } => {
                canonical_declaration.file.hash(&mut hasher);
                qualified_name.hash(&mut hasher);
            }
        }
        format!("node_{:016x}", hasher.finish())
    }
}
