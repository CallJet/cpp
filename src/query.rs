//! 온디맨드 쿼리 및 순회 엔진 모듈
//! On-demand query and traversal engine module

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::PathBuf;
use std::time::Instant;

use crate::diagnostic::{
    AnalysisCause, AnalysisIssue, Diagnostic, FatalError, QueryError, Severity,
};
use crate::discovery::DiscoveryIndex;
use crate::model::{
    BackendSymbolId, CallEdge, CallEdgeId, CallKind, CallPath, CandidateCallId, CandidateCallKind,
    CandidateSymbol, CandidateSymbolId, CandidateSymbolKind, CompilationKey, Completion,
    Confidence, QueryMetrics, QueryRequest, QueryResult, ResultCounts, Symbol, SymbolId,
    SymbolQuery, VerificationEvidence, VerificationReason, VerifiedEdgeKey,
};
use crate::project::ProjectContext;
use crate::semantic::{SemanticProvider, VerificationBatch, VerificationResult};

macro_rules! progress_log {
    ($enabled:expr, $($arg:tt)*) => {
        if $enabled {
            eprintln!($($arg)*);
        }
    };
}

/// 순회 프론티어 아이템 (Traversal Frontier Item, SDS §9.1)
#[derive(Debug, Clone)]
pub struct FrontierItem {
    pub symbol: SymbolId,
    pub depth: usize,
    pub predecessor: Option<(SymbolId, CallEdgeId)>,
}

/// 순회 공유 상태 (Traversal Shared State, SDS §9.1)
#[derive(Debug, Default)]
pub struct TraversalState {
    pub frontier: VecDeque<FrontierItem>,
    pub best_depth: HashMap<SymbolId, usize>,
    pub predecessors: HashMap<SymbolId, (SymbolId, CallEdgeId)>,
    pub edges: BTreeMap<VerifiedEdgeKey, CallEdge>,
}

/// 한 쿼리 안에서 동일한 시맨틱 검증 요청을 재사용하기 위한 안정적 키.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct VerificationBatchCacheKey {
    context: CompilationKey,
    symbols: Vec<CandidateSymbolId>,
    calls: Vec<CandidateCallId>,
    discovery_symbol_count: Option<usize>,
}

/// 온디맨드 쿼리 엔진 (Query Engine)
pub struct QueryEngine<'a, S: SemanticProvider> {
    pub project: &'a ProjectContext,
    pub discovery: DiscoveryIndex,
    pub provider: S,
    progress: bool,
    diagnostics: Vec<Diagnostic>,
    missing_context_files: BTreeSet<PathBuf>,
    next_fallback_edge_id: u32,
}

impl<'a, S: SemanticProvider> QueryEngine<'a, S> {
    /// 새 쿼리 엔진 인스턴스 생성
    pub fn new(project: &'a ProjectContext, provider: S) -> Self {
        Self {
            project,
            discovery: DiscoveryIndex::default(),
            provider,
            progress: false,
            diagnostics: Vec::new(),
            missing_context_files: BTreeSet::new(),
            next_fallback_edge_id: u32::MAX,
        }
    }

    /// 쿼리, 순회 및 discovery 진행 로그 상세도를 설정한다.
    pub fn set_progress_verbosity(&mut self, verbosity: u8) {
        self.progress = verbosity > 0;
        self.discovery.set_verbosity(verbosity);
    }

    /// 기존 호출자를 위한 집계 진행 로그 토글.
    pub fn set_progress(&mut self, enabled: bool) {
        self.set_progress_verbosity(u8::from(enabled));
    }

    /// 동일 컨텍스트·대상·호출 후보 묶음은 공급자를 다시 호출하지 않고 재사용한다.
    fn verify_calls_cached(
        &mut self,
        cache: &mut BTreeMap<VerificationBatchCacheKey, VerificationResult>,
        mut batch: VerificationBatch,
        progress_scope: &str,
        verified_batches: &mut usize,
        attempted_tu_keys: &mut BTreeSet<CompilationKey>,
    ) -> VerificationResult {
        batch.symbols.sort();
        batch.symbols.dedup();
        batch.calls.sort();
        batch.calls.dedup();

        let key = VerificationBatchCacheKey {
            context: batch.context.clone(),
            symbols: batch.symbols.clone(),
            calls: batch.calls.clone(),
            discovery_symbol_count: batch
                .symbols
                .is_empty()
                .then_some(self.discovery.symbols.len()),
        };
        if let Some(cached) = cache.get(&key) {
            return cached.clone();
        }

        *verified_batches += 1;
        attempted_tu_keys.insert(batch.context.clone());
        if *verified_batches == 1 || *verified_batches % 25 == 0 {
            progress_log!(
                self.progress,
                "[CallJet] traversal/{progress_scope}: semantic batch {}, unique TU {}, {} call candidate(s)",
                *verified_batches,
                attempted_tu_keys.len(),
                batch.calls.len()
            );
        }

        let result = self
            .provider
            .verify_calls(self.project, &self.discovery, batch);
        cache.insert(key, result.clone());
        result
    }

    /// 쿼리 요청 실행
    pub fn execute(&mut self, request: QueryRequest) -> Result<QueryResult, FatalError> {
        self.diagnostics = self.project.compilation_db.diagnostics.clone();
        self.missing_context_files.clear();
        self.next_fallback_edge_id = u32::MAX;
        let total_start = Instant::now();
        let inspected_before = self.discovery.source_files_inspected;
        let parsed_before = self.discovery.source_files.len();
        let discovery_time_before = self.discovery.discovery_time;

        let mut metrics = QueryMetrics {
            available_translation_units: self.project.compilation_db.all_source_files().len(),
            ..Default::default()
        };

        let result = match request {
            QueryRequest::Trace {
                target,
                max_depth,
                verified_only,
            } => self.execute_callers(target, max_depth, verified_only, true, &mut metrics)?,
            QueryRequest::Callers {
                target,
                max_depth,
                verified_only,
            } => self.execute_callers(target, max_depth, verified_only, false, &mut metrics)?,
            QueryRequest::Callees {
                source,
                max_depth,
                verified_only,
            } => self.execute_callees(source, max_depth, verified_only, &mut metrics)?,
            QueryRequest::Path {
                source,
                target,
                max_depth,
                verified_only,
            } => self.execute_path(source, target, max_depth, verified_only, &mut metrics)?,
            QueryRequest::Explain { caller, callee } => {
                self.execute_explain(caller, callee, &mut metrics)?
            }
        };

        metrics.source_files_inspected = self
            .discovery
            .source_files_inspected
            .saturating_sub(inspected_before);
        metrics.source_files_parsed_by_treesitter = self
            .discovery
            .source_files
            .len()
            .saturating_sub(parsed_before);
        metrics.candidate_call_sites = self.discovery.calls.len();
        metrics.discovery_time = self
            .discovery
            .discovery_time
            .saturating_sub(discovery_time_before);
        metrics.total_query_time = total_start.elapsed();

        // 검증된 파일 및 스킵된 파일 목록 계산
        let all_sources = &self.discovery.source_files;
        let mut verified_files = BTreeSet::new();
        for edge in &result.edges {
            if !edge.contexts.is_empty() {
                verified_files.insert(edge.callsite.start.file.clone());
            }
        }

        let mut skipped_files = Vec::new();
        for src in all_sources {
            if !verified_files.contains(src) {
                skipped_files.push(src.clone());
            }
        }

        metrics.verified_source_files = verified_files.into_iter().collect();
        metrics.skipped_source_files = skipped_files;

        let mut final_res = result;
        final_res.metrics = metrics;
        final_res
            .diagnostics
            .extend(std::mem::take(&mut self.diagnostics));

        Ok(final_res)
    }

    /// 단일 심볼 쿼리를 canonical Symbol로 해석
    fn resolve_endpoint(&mut self, query: &SymbolQuery) -> Result<Symbol, FatalError> {
        self.discovery.discover_query(self.project, query);
        let cand_ids = self.discovery.matching_symbols(query).to_vec();
        if cand_ids.is_empty() {
            return Err(FatalError::Query(QueryError::SymbolNotFound {
                query: query.raw.clone(),
            }));
        }

        let resolution =
            self.provider
                .resolve_symbols(self.project, &self.discovery, &cand_ids);
        self.record_analysis_issues(resolution.issues.clone());

        if resolution.symbols.is_empty() {
            // 관련 TU를 정상 파싱했지만 callable 커서가 하나도 없으면 해당
            // 빌드 구성에서 #if 등으로 비활성화된 구문 후보로 간주한다.
            if resolution.checked_contexts > 0 {
                return Err(FatalError::Query(QueryError::SymbolNotFound {
                    query: query.raw.clone(),
                }));
            }

            // Clang을 사용할 수 없거나 모든 관련 TU 파싱이 실패한 경우에도
            // Tree-sitter의 완전한 구문 후보를 버리지 않는다.
            let complete_candidates = cand_ids
                .iter()
                .filter_map(|id| self.discovery.symbols.get(id))
                .cloned()
                .filter(|candidate| candidate.syntax_complete)
                .collect::<Vec<_>>();
            let definitions = complete_candidates
                .iter()
                .filter(|candidate| candidate.definition_body.is_some())
                .cloned()
                .collect::<Vec<_>>();
            let viable_candidates = if definitions.is_empty() {
                &complete_candidates
            } else {
                &definitions
            };
            if viable_candidates.len() > 1 {
                let candidates = viable_candidates
                    .iter()
                    .map(candidate_display_name_with_location)
                    .collect::<Vec<_>>();
                return Err(FatalError::Query(QueryError::AmbiguousSymbol {
                    query: query.raw.clone(),
                    candidates,
                }));
            }
            let first_cand = viable_candidates.first().cloned().ok_or_else(|| {
                FatalError::Query(QueryError::SymbolNotFound {
                    query: query.raw.clone(),
                })
            })?;
            if resolution.failed_contexts == 0 {
                self.record_missing_symbol_context(&first_cand);
            }
            return Ok(candidate_to_symbol(&first_cand));
        }

        if resolution.symbols.len() > 1 {
            let names: Vec<String> = resolution
                .symbols
                .iter()
                .map(|s| s.display_name().to_string())
                .collect();
            return Err(FatalError::Query(QueryError::AmbiguousSymbol {
                query: query.raw.clone(),
                candidates: names,
            }));
        }

        Ok(resolution.symbols[0].clone())
    }

    fn record_analysis_issues(&mut self, issues: Vec<AnalysisIssue>) {
        for issue in issues {
            let diagnostic = Diagnostic::analysis(issue);
            if !self.diagnostics.contains(&diagnostic) {
                self.diagnostics.push(diagnostic);
            }
        }
    }

    fn record_missing_context(&mut self, call_id: CandidateCallId) {
        let Some(location) = self
            .discovery
            .calls
            .get(&call_id)
            .map(|call| call.expression.start.clone())
        else {
            return;
        };
        if !self.missing_context_files.insert(location.file.clone()) {
            return;
        }
        self.record_analysis_issues(vec![AnalysisIssue {
            severity: Severity::Recoverable,
            context: None,
            location: Some(location),
            message: "연결된 컴파일 컨텍스트가 없어 Tree-sitter 호출 후보를 유지합니다."
                .to_string(),
            cause: AnalysisCause::MissingCompilationContext,
        }]);
    }

    fn record_missing_symbol_context(&mut self, candidate: &CandidateSymbol) {
        if !self
            .missing_context_files
            .insert(candidate.declaration.start.file.clone())
        {
            return;
        }
        self.record_analysis_issues(vec![AnalysisIssue {
            severity: Severity::Recoverable,
            context: None,
            location: Some(candidate.declaration.start.clone()),
            message: format!(
                "심볼 '{}'에 연결된 컴파일 컨텍스트가 없어 Tree-sitter 후보를 유지합니다.",
                candidate.name
            ),
            cause: AnalysisCause::MissingCompilationContext,
        }]);
    }

    fn fallback_edges_for_call(
        &mut self,
        call_id: CandidateCallId,
        forced_caller: Option<&Symbol>,
        forced_callee: Option<&Symbol>,
    ) -> Vec<(CallEdge, Symbol, Option<Symbol>)> {
        let Some(call) = self.discovery.calls.get(&call_id).cloned() else {
            return Vec::new();
        };
        if !call.syntax_complete {
            return Vec::new();
        }

        let caller = forced_caller.cloned().or_else(|| {
            self.discovery
                .symbols
                .get(&call.caller)
                .filter(|candidate| candidate.syntax_complete)
                .map(candidate_to_symbol)
        });
        let Some(caller) = caller else {
            return Vec::new();
        };

        let mut callees = if let Some(callee) = forced_callee {
            vec![callee.clone()]
        } else {
            self.discovery
                .discover_spelling(self.project, &call.callee_spelling);
            let raw_query = call
                .qualifier_hint
                .as_deref()
                .map(|qualifier| format!("{qualifier}{}", call.callee_spelling))
                .unwrap_or_else(|| call.callee_spelling.clone());
            let query = SymbolQuery::parse(&raw_query);
            self.discovery
                .matching_symbols(&query)
                .iter()
                .filter_map(|id| self.discovery.symbols.get(id))
                .filter(|candidate| candidate.syntax_complete)
                .map(candidate_to_symbol)
                .collect::<Vec<_>>()
        };
        callees.sort_by(|left, right| left.id.cmp(&right.id));
        callees.dedup_by(|left, right| left.id == right.id);

        let syntactic_kind = match call.syntax_hint {
            CandidateCallKind::Direct | CandidateCallKind::Qualified => CallKind::Direct,
            CandidateCallKind::Member | CandidateCallKind::Other => CallKind::Unresolved,
        };
        let callee_options = if callees.is_empty() {
            vec![None]
        } else {
            callees.into_iter().map(Some).collect()
        };

        callee_options
            .into_iter()
            .map(|callee| {
                let confidence = if callee.is_some() {
                    Confidence::Possible
                } else {
                    Confidence::Unresolved
                };
                let evidence_key = CompilationKey("tree-sitter-fallback".to_string());
                let mut evidence_by_context = BTreeMap::new();
                evidence_by_context.insert(
                    evidence_key,
                    VerificationEvidence {
                        expression_text: call.expression_text.clone(),
                        static_target: None,
                        candidate_targets: callee.iter().cloned().collect(),
                        clang_diagnostics: Vec::new(),
                        reason: VerificationReason::SyntacticCandidate,
                        spelling_location: call
                            .callee_location
                            .clone()
                            .or_else(|| Some(call.expression.start.clone())),
                        expansion_location: Some(call.expression.start.clone()),
                        is_virtual: false,
                        is_template_related: false,
                        is_macro_expanded: false,
                    },
                );

                let edge = CallEdge {
                    id: self.allocate_fallback_edge_id(),
                    caller: caller.id.clone(),
                    callee: callee.as_ref().map(|symbol| symbol.id.clone()),
                    callsite: call.expression.clone(),
                    kind: syntactic_kind,
                    confidence,
                    contexts: BTreeSet::new(),
                    evidence_by_context,
                };
                (edge, caller.clone(), callee)
            })
            .collect()
    }

    fn allocate_fallback_edge_id(&mut self) -> CallEdgeId {
        let id = CallEdgeId(self.next_fallback_edge_id);
        self.next_fallback_edge_id = self.next_fallback_edge_id.saturating_sub(1);
        id
    }

    /// callers 쿼리 실행 (온디맨드 역방향 탐색)
    fn execute_callers(
        &mut self,
        target_query: SymbolQuery,
        max_depth: Option<usize>,
        verified_only: bool,
        include_paths: bool,
        metrics: &mut QueryMetrics,
    ) -> Result<QueryResult, FatalError> {
        let query_name = if include_paths { "trace" } else { "callers" };
        progress_log!(
            self.progress,
            "[CallJet] query/{query_name}: resolving target '{}'...",
            target_query.raw
        );
        let target_sym = self.resolve_endpoint(&target_query)?;
        progress_log!(
            self.progress,
            "[CallJet] traversal/{query_name}: reverse search from {}",
            target_sym.display_name()
        );

        let mut symbols_map: BTreeMap<SymbolId, Symbol> = BTreeMap::new();
        symbols_map.insert(target_sym.id.clone(), target_sym.clone());

        let mut state = TraversalState::default();
        let mut truncated = false;

        state.frontier.push_back(FrontierItem {
            symbol: target_sym.id.clone(),
            depth: 0,
            predecessor: None,
        });
        state.best_depth.insert(target_sym.id.clone(), 0);

        let mut verified_tu_keys = BTreeSet::new();
        let mut attempted_tu_keys = BTreeSet::new();
        let mut verification_cache = BTreeMap::new();
        let mut reported_depth = None;
        let mut processed_nodes = 0usize;
        let mut verified_batches = 0usize;

        while let Some(item) = state.frontier.pop_front() {
            processed_nodes += 1;
            if reported_depth != Some(item.depth) {
                reported_depth = Some(item.depth);
                progress_log!(
                    self.progress,
                    "[CallJet] traversal/{query_name}: depth {}, processed {}, queued {}, verified edges {}",
                    item.depth,
                    processed_nodes,
                    state.frontier.len(),
                    state.edges.len()
                );
            }

            if let Some(limit) = max_depth {
                if item.depth >= limit {
                    truncated = true;
                    continue;
                }
            }

            let cur_sym = match symbols_map.get(&item.symbol) {
                Some(s) => s.clone(),
                None => continue,
            };

            // 1. 후보 호출 발견 (역방향)
            self.discovery
                .discover_spelling(self.project, &cur_sym.name);
            let cur_query = SymbolQuery::parse(
                cur_sym
                    .qualified_name
                    .as_deref()
                    .unwrap_or(cur_sym.name.as_str()),
            );
            let target_candidates = self.discovery.matching_symbols(&cur_query).to_vec();
            let candidate_calls = self.discovery.candidate_callers(&cur_sym);
            metrics.semantic_candidates_verified += candidate_calls.len();

            // 2. 컴파일 컨텍스트별 그룹화
            let mut batches: BTreeMap<CompilationKey, Vec<CandidateCallId>> = BTreeMap::new();
            let mut fallback_calls = candidate_calls.iter().copied().collect::<BTreeSet<_>>();
            for &call_id in &candidate_calls {
                let contexts = self.discovery.contexts_for(call_id, self.project);
                if contexts.is_empty() {
                    self.record_missing_context(call_id);
                }
                for ctx_key in contexts {
                    batches.entry(ctx_key).or_default().push(call_id);
                }
            }

            // 3. Clang 온디맨드 시맨틱 검증 수행
            let verify_start = Instant::now();
            for (ctx_key, calls) in batches {
                let batch = VerificationBatch {
                    context: ctx_key.clone(),
                    symbols: target_candidates.clone(),
                    calls: calls.clone(),
                };
                let ver_res = self.verify_calls_cached(
                    &mut verification_cache,
                    batch,
                    query_name,
                    &mut verified_batches,
                    &mut attempted_tu_keys,
                );
                let context_checked = ver_res.context_checked;
                self.record_analysis_issues(ver_res.issues);
                if context_checked {
                    verified_tu_keys.insert(ctx_key);
                    for call_id in &calls {
                        fallback_calls.remove(call_id);
                    }
                }

                record_verified_symbols(&mut symbols_map, ver_res.symbols);

                for mut edge in ver_res.edges {
                    // verified_only 필터링
                    if verified_only && edge.confidence != Confidence::Confirmed {
                        continue;
                    }

                    // 피호출자가 현재 심볼과 매칭되는지 확인
                    let targets_current = edge_targets_symbol(&edge, &cur_sym, &symbols_map);
                    let matches_callee = targets_current
                        || (edge.callee.is_none()
                            && edge.confidence == Confidence::Unresolved);

                    if matches_callee {
                        // 정적 참조가 base virtual 메서드여도 요청 대상이 같은 override
                        // family라면 쿼리 결과 엣지는 실제 요청 대상으로 연결한다.
                        if targets_current
                            && (edge.kind == CallKind::Virtual
                                || is_tree_sitter_symbol(&cur_sym))
                            && edge.callee.as_ref() != Some(&item.symbol)
                        {
                            edge.callee = Some(item.symbol.clone());
                        }

                        // caller 심볼 기록
                        let caller_id = edge.caller.clone();
                        let edge_key = VerifiedEdgeKey {
                            caller: edge.caller.clone(),
                            callee: edge.callee.clone(),
                            callsite: edge.callsite.clone(),
                            kind: edge.kind,
                            confidence: edge.confidence,
                        };

                        record_symbols_from_edge(&mut symbols_map, &edge);
                        if let Some(existing) = state.edges.get_mut(&edge_key) {
                            existing.contexts.extend(edge.contexts);
                            existing
                                .evidence_by_context
                                .extend(edge.evidence_by_context);
                        } else {
                            state.edges.insert(edge_key, edge);
                        }

                        // 다음 프론티어 enqueue (사이클 및 최적 깊이 관리)
                        let next_depth = item.depth + 1;
                        let should_enqueue = match state.best_depth.get(&caller_id) {
                            Some(&d) => next_depth < d,
                            None => true,
                        };

                        if should_enqueue {
                            state.best_depth.insert(caller_id.clone(), next_depth);
                            state.frontier.push_back(FrontierItem {
                                symbol: caller_id,
                                depth: next_depth,
                                predecessor: Some((item.symbol.clone(), CallEdgeId(0))),
                            });
                        }
                    }
                }
            }

            if !verified_only {
                for call_id in fallback_calls {
                    for (edge, caller, callee) in
                        self.fallback_edges_for_call(call_id, None, Some(&cur_sym))
                    {
                        let caller_id = caller.id.clone();
                        symbols_map.entry(caller.id.clone()).or_insert(caller);
                        if let Some(callee) = callee {
                            symbols_map.entry(callee.id.clone()).or_insert(callee);
                        }

                        let edge_key = VerifiedEdgeKey {
                            caller: edge.caller.clone(),
                            callee: edge.callee.clone(),
                            callsite: edge.callsite.clone(),
                            kind: edge.kind,
                            confidence: edge.confidence,
                        };
                        state.edges.entry(edge_key).or_insert(edge);

                        let next_depth = item.depth + 1;
                        let should_enqueue = match state.best_depth.get(&caller_id) {
                            Some(&depth) => next_depth < depth,
                            None => true,
                        };
                        if should_enqueue {
                            state.best_depth.insert(caller_id.clone(), next_depth);
                            state.frontier.push_back(FrontierItem {
                                symbol: caller_id,
                                depth: next_depth,
                                predecessor: Some((item.symbol.clone(), CallEdgeId(0))),
                            });
                        }
                    }
                }
            }
            metrics.verification_time += verify_start.elapsed();
        }

        metrics.verified_translation_units = verified_tu_keys.len();

        let edges_vec: Vec<CallEdge> = state.edges.into_values().collect();
        let paths = if include_paths {
            build_caller_paths(&target_sym.id, &edges_vec)
        } else {
            Vec::new()
        };
        let counts = calculate_counts(&symbols_map, &edges_vec, &paths);

        let completion = if edges_vec.is_empty() {
            Completion::NoResult
        } else if truncated {
            Completion::Truncated {
                max_depth: max_depth.unwrap_or(0),
            }
        } else {
            Completion::Complete
        };

        progress_log!(
            self.progress,
            "[CallJet] traversal/{query_name}: complete — processed {processed_nodes} node(s), {verified_batches} semantic batch(es), {} unique TU(s), {} edge(s), {} path(s)",
            attempted_tu_keys.len(),
            edges_vec.len(),
            paths.len()
        );

        Ok(QueryResult {
            completion,
            symbols: symbols_map,
            edges: edges_vec,
            paths,
            counts,
            diagnostics: Vec::new(),
            metrics: metrics.clone(),
        })
    }

    /// callees 쿼리 실행 (온디맨드 순방향 탐색)
    fn execute_callees(
        &mut self,
        source_query: SymbolQuery,
        max_depth: Option<usize>,
        verified_only: bool,
        metrics: &mut QueryMetrics,
    ) -> Result<QueryResult, FatalError> {
        progress_log!(
            self.progress,
            "[CallJet] query/callees: resolving source '{}'...",
            source_query.raw
        );
        let source_sym = self.resolve_endpoint(&source_query)?;
        progress_log!(
            self.progress,
            "[CallJet] traversal/callees: forward search from {}",
            source_sym.display_name()
        );

        let mut symbols_map: BTreeMap<SymbolId, Symbol> = BTreeMap::new();
        symbols_map.insert(source_sym.id.clone(), source_sym.clone());

        let mut state = TraversalState::default();
        let mut truncated = false;

        state.frontier.push_back(FrontierItem {
            symbol: source_sym.id.clone(),
            depth: 0,
            predecessor: None,
        });
        state.best_depth.insert(source_sym.id.clone(), 0);

        let mut verified_tu_keys = BTreeSet::new();
        let mut attempted_tu_keys = BTreeSet::new();
        let mut verification_cache = BTreeMap::new();
        let mut reported_depth = None;
        let mut processed_nodes = 0usize;
        let mut verified_batches = 0usize;

        while let Some(item) = state.frontier.pop_front() {
            processed_nodes += 1;
            if reported_depth != Some(item.depth) {
                reported_depth = Some(item.depth);
                progress_log!(
                    self.progress,
                    "[CallJet] traversal/callees: depth {}, processed {}, queued {}, verified edges {}",
                    item.depth,
                    processed_nodes,
                    state.frontier.len(),
                    state.edges.len()
                );
            }

            if let Some(limit) = max_depth {
                if item.depth >= limit {
                    truncated = true;
                    continue;
                }
            }

            // 1. 현재 심볼에 해당하는 candidate symbol 탐색
            let cur_sym = match symbols_map.get(&item.symbol) {
                Some(symbol) => symbol.clone(),
                None => continue,
            };
            let cur_query = SymbolQuery::parse(
                cur_sym
                    .qualified_name
                    .as_deref()
                    .unwrap_or(cur_sym.name.as_str()),
            );
            self.discovery.discover_query(self.project, &cur_query);
            let cand_syms = self
                .discovery
                .matching_symbols(&cur_query);
            let mut calls_to_verify = Vec::new();
            for &cand_id in cand_syms {
                let calls = self.discovery.candidate_callees(cand_id);
                calls_to_verify.extend_from_slice(calls);
            }

            metrics.semantic_candidates_verified += calls_to_verify.len();

            // 2. 컴파일 컨텍스트 그룹화
            let mut batches: BTreeMap<CompilationKey, Vec<CandidateCallId>> = BTreeMap::new();
            let mut fallback_calls = calls_to_verify.iter().copied().collect::<BTreeSet<_>>();
            for &call_id in &calls_to_verify {
                let contexts = self.discovery.contexts_for(call_id, self.project);
                if contexts.is_empty() {
                    self.record_missing_context(call_id);
                }
                for ctx_key in contexts {
                    batches.entry(ctx_key).or_default().push(call_id);
                }
            }

            // 3. Clang 검증
            let verify_start = Instant::now();
            for (ctx_key, calls) in batches {
                let batch = VerificationBatch {
                    context: ctx_key.clone(),
                    symbols: Vec::new(),
                    calls: calls.clone(),
                };
                let ver_res = self.verify_calls_cached(
                    &mut verification_cache,
                    batch,
                    "callees",
                    &mut verified_batches,
                    &mut attempted_tu_keys,
                );
                let context_checked = ver_res.context_checked;
                self.record_analysis_issues(ver_res.issues);
                if context_checked {
                    verified_tu_keys.insert(ctx_key);
                    for call_id in &calls {
                        fallback_calls.remove(call_id);
                    }
                }

                record_verified_symbols(&mut symbols_map, ver_res.symbols);

                for mut edge in ver_res.edges {
                    if verified_only && edge.confidence != Confidence::Confirmed {
                        continue;
                    }

                    if !edge_caller_matches_symbol(&edge, &cur_sym, &symbols_map) {
                        continue;
                    }
                    if edge.caller != item.symbol {
                        edge.caller = item.symbol.clone();
                    }

                    let edge_key = VerifiedEdgeKey {
                        caller: edge.caller.clone(),
                        callee: edge.callee.clone(),
                        callsite: edge.callsite.clone(),
                        kind: edge.kind,
                        confidence: edge.confidence,
                    };

                    let callee_id_opt = edge.callee.clone();

                    record_symbols_from_edge(&mut symbols_map, &edge);
                    if let Some(existing) = state.edges.get_mut(&edge_key) {
                        existing.contexts.extend(edge.contexts);
                        existing
                            .evidence_by_context
                            .extend(edge.evidence_by_context);
                    } else {
                        state.edges.insert(edge_key, edge);
                    }

                    // 대상 식별자가 있는 경우에만 다음 프론티어로 enqueue (UNRESOLVED는 enqueue 안 함)
                    if let Some(callee_id) = callee_id_opt {
                        let next_depth = item.depth + 1;
                        let should_enqueue = match state.best_depth.get(&callee_id) {
                            Some(&d) => next_depth < d,
                            None => true,
                        };

                        if should_enqueue {
                            state.best_depth.insert(callee_id.clone(), next_depth);
                            state.frontier.push_back(FrontierItem {
                                symbol: callee_id,
                                depth: next_depth,
                                predecessor: Some((item.symbol.clone(), CallEdgeId(0))),
                            });
                        }
                    }
                }
            }

            if !verified_only {
                for call_id in fallback_calls {
                    for (edge, caller, callee) in
                        self.fallback_edges_for_call(call_id, Some(&cur_sym), None)
                    {
                        let edge_key = VerifiedEdgeKey {
                            caller: edge.caller.clone(),
                            callee: edge.callee.clone(),
                            callsite: edge.callsite.clone(),
                            kind: edge.kind,
                            confidence: edge.confidence,
                        };
                        let edge_id = edge.id;
                        let callee_id = callee.as_ref().map(|symbol| symbol.id.clone());
                        symbols_map.entry(caller.id.clone()).or_insert(caller);
                        if let Some(callee) = callee {
                            symbols_map.entry(callee.id.clone()).or_insert(callee);
                        }
                        state.edges.entry(edge_key).or_insert(edge);

                        if let Some(callee_id) = callee_id {
                            let next_depth = item.depth + 1;
                            let should_enqueue = match state.best_depth.get(&callee_id) {
                                Some(&depth) => next_depth < depth,
                                None => true,
                            };
                            if should_enqueue {
                                state.best_depth.insert(callee_id.clone(), next_depth);
                                state.frontier.push_back(FrontierItem {
                                    symbol: callee_id,
                                    depth: next_depth,
                                    predecessor: Some((item.symbol.clone(), edge_id)),
                                });
                            }
                        }
                    }
                }
            }
            metrics.verification_time += verify_start.elapsed();
        }

        metrics.verified_translation_units = verified_tu_keys.len();

        let edges_vec: Vec<CallEdge> = state.edges.into_values().collect();
        let counts = calculate_counts(&symbols_map, &edges_vec, &[]);

        let completion = if edges_vec.is_empty() {
            Completion::NoResult
        } else if truncated {
            Completion::Truncated {
                max_depth: max_depth.unwrap_or(0),
            }
        } else {
            Completion::Complete
        };

        progress_log!(
            self.progress,
            "[CallJet] traversal/callees: complete — processed {processed_nodes} node(s), {verified_batches} semantic batch(es), {} unique TU(s), {} edge(s)",
            attempted_tu_keys.len(),
            edges_vec.len()
        );

        Ok(QueryResult {
            completion,
            symbols: symbols_map,
            edges: edges_vec,
            paths: Vec::new(),
            counts,
            diagnostics: Vec::new(),
            metrics: metrics.clone(),
        })
    }

    /// path 쿼리 실행 (온디맨드 경로 탐색)
    fn execute_path(
        &mut self,
        source_query: SymbolQuery,
        target_query: SymbolQuery,
        max_depth: Option<usize>,
        verified_only: bool,
        metrics: &mut QueryMetrics,
    ) -> Result<QueryResult, FatalError> {
        progress_log!(
            self.progress,
            "[CallJet] query/path: resolving '{}' -> '{}'...",
            source_query.raw, target_query.raw
        );
        let source_sym = self.resolve_endpoint(&source_query)?;
        let target_sym = self.resolve_endpoint(&target_query)?;
        let target_candidates = self.discovery.matching_symbols(&target_query).to_vec();
        progress_log!(
            self.progress,
            "[CallJet] traversal/path: forward search {} -> {}",
            source_sym.display_name(),
            target_sym.display_name()
        );

        let mut symbols_map: BTreeMap<SymbolId, Symbol> = BTreeMap::new();
        symbols_map.insert(source_sym.id.clone(), source_sym.clone());
        symbols_map.insert(target_sym.id.clone(), target_sym.clone());

        // source == target 인 경우 0-edge path 즉시 반환
        if source_sym.id == target_sym.id {
            let path = CallPath {
                nodes: vec![source_sym.id.clone()],
                edges: vec![],
            };
            let counts = calculate_counts(&symbols_map, &[], std::slice::from_ref(&path));
            progress_log!(
                self.progress,
                "[CallJet] traversal/path: source and target are identical"
            );
            return Ok(QueryResult {
                completion: Completion::Complete,
                symbols: symbols_map,
                edges: Vec::new(),
                paths: vec![path],
                counts,
                diagnostics: Vec::new(),
                metrics: metrics.clone(),
            });
        }

        let mut state = TraversalState::default();
        let mut found_target = false;
        let mut truncated = false;

        state.frontier.push_back(FrontierItem {
            symbol: source_sym.id.clone(),
            depth: 0,
            predecessor: None,
        });
        state.best_depth.insert(source_sym.id.clone(), 0);

        let mut verified_tu_keys = BTreeSet::new();
        let mut attempted_tu_keys = BTreeSet::new();
        let mut verification_cache = BTreeMap::new();
        let mut reported_depth = None;
        let mut processed_nodes = 0usize;
        let mut verified_batches = 0usize;

        while let Some(item) = state.frontier.pop_front() {
            processed_nodes += 1;
            if reported_depth != Some(item.depth) {
                reported_depth = Some(item.depth);
                progress_log!(
                    self.progress,
                    "[CallJet] traversal/path: depth {}, processed {}, queued {}, verified edges {}",
                    item.depth,
                    processed_nodes,
                    state.frontier.len(),
                    state.edges.len()
                );
            }

            if item.symbol == target_sym.id {
                found_target = true;
                break;
            }

            if let Some(limit) = max_depth {
                if item.depth >= limit {
                    truncated = true;
                    continue;
                }
            }

            let cur_sym = match symbols_map.get(&item.symbol) {
                Some(symbol) => symbol.clone(),
                None => continue,
            };
            let cur_query = SymbolQuery::parse(
                cur_sym
                    .qualified_name
                    .as_deref()
                    .unwrap_or(cur_sym.name.as_str()),
            );
            self.discovery.discover_query(self.project, &cur_query);
            let cand_syms = self
                .discovery
                .matching_symbols(&cur_query);
            let mut calls_to_verify = Vec::new();
            for &cand_id in cand_syms {
                let calls = self.discovery.candidate_callees(cand_id);
                calls_to_verify.extend_from_slice(calls);
            }

            let mut batches: BTreeMap<CompilationKey, Vec<CandidateCallId>> = BTreeMap::new();
            let mut fallback_calls = calls_to_verify.iter().copied().collect::<BTreeSet<_>>();
            for &call_id in &calls_to_verify {
                let contexts = self.discovery.contexts_for(call_id, self.project);
                if contexts.is_empty() {
                    self.record_missing_context(call_id);
                }
                for ctx_key in contexts {
                    batches.entry(ctx_key).or_default().push(call_id);
                }
            }

            for (ctx_key, calls) in batches {
                let batch = VerificationBatch {
                    context: ctx_key.clone(),
                    symbols: target_candidates.clone(),
                    calls: calls.clone(),
                };
                let ver_res = self.verify_calls_cached(
                    &mut verification_cache,
                    batch,
                    "path",
                    &mut verified_batches,
                    &mut attempted_tu_keys,
                );
                let context_checked = ver_res.context_checked;
                self.record_analysis_issues(ver_res.issues);
                if context_checked {
                    verified_tu_keys.insert(ctx_key);
                    for call_id in &calls {
                        fallback_calls.remove(call_id);
                    }
                }

                record_verified_symbols(&mut symbols_map, ver_res.symbols);

                for mut edge in ver_res.edges {
                    if verified_only && edge.confidence != Confidence::Confirmed {
                        continue;
                    }

                    if !edge_caller_matches_symbol(&edge, &cur_sym, &symbols_map) {
                        continue;
                    }
                    if edge.caller != item.symbol {
                        edge.caller = item.symbol.clone();
                    }

                    if (edge.kind == CallKind::Virtual || is_tree_sitter_symbol(&target_sym))
                        && edge_targets_symbol(&edge, &target_sym, &symbols_map)
                        && edge.callee.as_ref() != Some(&target_sym.id)
                    {
                        edge.callee = Some(target_sym.id.clone());
                    }

                    let edge_id = edge.id;
                    let callee_id_opt = edge.callee.clone();

                    let edge_key = VerifiedEdgeKey {
                        caller: edge.caller.clone(),
                        callee: edge.callee.clone(),
                        callsite: edge.callsite.clone(),
                        kind: edge.kind,
                        confidence: edge.confidence,
                    };

                    record_symbols_from_edge(&mut symbols_map, &edge);
                    state.edges.insert(edge_key, edge);

                    if let Some(callee_id) = callee_id_opt {
                        let next_depth = item.depth + 1;
                        let should_enqueue = match state.best_depth.get(&callee_id) {
                            Some(&d) => next_depth < d,
                            None => true,
                        };

                        if should_enqueue {
                            state.best_depth.insert(callee_id.clone(), next_depth);
                            state
                                .predecessors
                                .insert(callee_id.clone(), (item.symbol.clone(), edge_id));
                            state.frontier.push_back(FrontierItem {
                                symbol: callee_id,
                                depth: next_depth,
                                predecessor: Some((item.symbol.clone(), edge_id)),
                            });
                        }
                    }
                }
            }

            if !verified_only {
                for call_id in fallback_calls {
                    let forced_target = self
                        .discovery
                        .calls
                        .get(&call_id)
                        .filter(|call| call_may_target_symbol(call, &target_sym))
                        .map(|_| &target_sym);
                    for (edge, caller, callee) in self.fallback_edges_for_call(
                        call_id,
                        Some(&cur_sym),
                        forced_target,
                    ) {
                        let edge_key = VerifiedEdgeKey {
                            caller: edge.caller.clone(),
                            callee: edge.callee.clone(),
                            callsite: edge.callsite.clone(),
                            kind: edge.kind,
                            confidence: edge.confidence,
                        };
                        let edge_id = edge.id;
                        let callee_id = callee.as_ref().map(|symbol| symbol.id.clone());
                        symbols_map.entry(caller.id.clone()).or_insert(caller);
                        if let Some(callee) = callee {
                            symbols_map.entry(callee.id.clone()).or_insert(callee);
                        }
                        state.edges.entry(edge_key).or_insert(edge);

                        if let Some(callee_id) = callee_id {
                            let next_depth = item.depth + 1;
                            let should_enqueue = match state.best_depth.get(&callee_id) {
                                Some(&depth) => next_depth < depth,
                                None => true,
                            };
                            if should_enqueue {
                                state.best_depth.insert(callee_id.clone(), next_depth);
                                state
                                    .predecessors
                                    .insert(callee_id.clone(), (item.symbol.clone(), edge_id));
                                state.frontier.push_back(FrontierItem {
                                    symbol: callee_id,
                                    depth: next_depth,
                                    predecessor: Some((item.symbol.clone(), edge_id)),
                                });
                            }
                        }
                    }
                }
            }
        }

        metrics.verified_translation_units = verified_tu_keys.len();

        let edges_vec: Vec<CallEdge> = state.edges.into_values().collect();

        if found_target {
            // 경로 역추적
            let mut nodes = Vec::new();
            let mut path_edges = Vec::new();
            let mut cur = target_sym.id.clone();
            nodes.push(cur.clone());

            while let Some((pred_sym, edge_id)) = state.predecessors.get(&cur) {
                path_edges.push(*edge_id);
                nodes.push(pred_sym.clone());
                cur = pred_sym.clone();
                if cur == source_sym.id {
                    break;
                }
            }

            nodes.reverse();
            path_edges.reverse();

            let path = CallPath {
                nodes,
                edges: path_edges,
            };
            let counts = calculate_counts(&symbols_map, &edges_vec, std::slice::from_ref(&path));
            progress_log!(
                self.progress,
                "[CallJet] traversal/path: found — processed {processed_nodes} node(s), {verified_batches} semantic batch(es), {} unique TU(s), {} hop(s)",
                attempted_tu_keys.len(),
                path.edges.len()
            );

            Ok(QueryResult {
                completion: Completion::Complete,
                symbols: symbols_map,
                edges: edges_vec,
                paths: vec![path],
                counts,
                diagnostics: Vec::new(),
                metrics: metrics.clone(),
            })
        } else if truncated {
            let counts = calculate_counts(&symbols_map, &edges_vec, &[]);
            progress_log!(
                self.progress,
                "[CallJet] traversal/path: truncated — processed {processed_nodes} node(s), {verified_batches} semantic batch(es), {} unique TU(s), {} edge(s)",
                attempted_tu_keys.len(),
                edges_vec.len()
            );
            Ok(QueryResult {
                completion: Completion::Truncated {
                    max_depth: max_depth.unwrap_or(0),
                },
                symbols: symbols_map,
                edges: edges_vec,
                paths: Vec::new(),
                counts,
                diagnostics: Vec::new(),
                metrics: metrics.clone(),
            })
        } else {
            let counts = calculate_counts(&symbols_map, &edges_vec, &[]);
            progress_log!(
                self.progress,
                "[CallJet] traversal/path: no path — processed {processed_nodes} node(s), {verified_batches} semantic batch(es), {} unique TU(s), {} edge(s)",
                attempted_tu_keys.len(),
                edges_vec.len()
            );
            Ok(QueryResult {
                completion: Completion::NoResult,
                symbols: symbols_map,
                edges: edges_vec,
                paths: Vec::new(),
                counts,
                diagnostics: Vec::new(),
                metrics: metrics.clone(),
            })
        }
    }

    /// explain 쿼리 실행
    fn execute_explain(
        &mut self,
        caller_query: SymbolQuery,
        callee_query: SymbolQuery,
        metrics: &mut QueryMetrics,
    ) -> Result<QueryResult, FatalError> {
        let caller_sym = self.resolve_endpoint(&caller_query)?;
        let callee_sym = self.resolve_endpoint(&callee_query)?;
        let target_candidates = self.discovery.matching_symbols(&callee_query).to_vec();

        let mut symbols_map: BTreeMap<SymbolId, Symbol> = BTreeMap::new();
        symbols_map.insert(caller_sym.id.clone(), caller_sym.clone());
        symbols_map.insert(callee_sym.id.clone(), callee_sym.clone());

        // caller 내부의 callee 호출 후보 검색
        let cand_syms = self
            .discovery
            .matching_symbols(&SymbolQuery::parse(&caller_sym.name));
        let mut calls_to_verify = Vec::new();
        for &cand_id in cand_syms {
            let calls = self.discovery.candidate_callees(cand_id);
            for &c in calls {
                if let Some(call_site) = self.discovery.calls.get(&c) {
                    if call_site.callee_spelling == callee_sym.name {
                        calls_to_verify.push(c);
                    }
                }
            }
        }

        let mut verified_edges = Vec::new();
        let mut batches: BTreeMap<CompilationKey, Vec<CandidateCallId>> = BTreeMap::new();
        let mut fallback_calls = calls_to_verify.iter().copied().collect::<BTreeSet<_>>();
        for &call_id in &calls_to_verify {
            let contexts = self.discovery.contexts_for(call_id, self.project);
            if contexts.is_empty() {
                self.record_missing_context(call_id);
            }
            for ctx_key in contexts {
                batches.entry(ctx_key).or_default().push(call_id);
            }
        }

        for (ctx_key, calls) in batches {
            let batch = VerificationBatch {
                context: ctx_key,
                symbols: target_candidates.clone(),
                calls: calls.clone(),
            };
            let ver_res =
                self.provider
                    .verify_calls(self.project, &self.discovery, batch);
            let context_checked = ver_res.context_checked;
            self.record_analysis_issues(ver_res.issues);
            if context_checked {
                for call_id in &calls {
                    fallback_calls.remove(call_id);
                }
            }
            record_verified_symbols(&mut symbols_map, ver_res.symbols);
            for mut edge in ver_res.edges {
                if !edge_caller_matches_symbol(&edge, &caller_sym, &symbols_map) {
                    continue;
                }
                if edge.caller != caller_sym.id {
                    edge.caller = caller_sym.id.clone();
                }
                if edge_targets_symbol(&edge, &callee_sym, &symbols_map) {
                    if (edge.kind == CallKind::Virtual || is_tree_sitter_symbol(&callee_sym))
                        && edge.callee.as_ref() != Some(&callee_sym.id)
                    {
                        edge.callee = Some(callee_sym.id.clone());
                    }
                    record_symbols_from_edge(&mut symbols_map, &edge);
                    verified_edges.push(edge);
                }
            }
        }

        for call_id in fallback_calls {
            for (edge, caller, callee) in self.fallback_edges_for_call(
                call_id,
                Some(&caller_sym),
                Some(&callee_sym),
            ) {
                symbols_map.entry(caller.id.clone()).or_insert(caller);
                if let Some(callee) = callee {
                    symbols_map.entry(callee.id.clone()).or_insert(callee);
                }
                verified_edges.push(edge);
            }
        }

        let counts = calculate_counts(&symbols_map, &verified_edges, &[]);

        let completion = if verified_edges.is_empty() {
            Completion::NoResult
        } else {
            Completion::Complete
        };

        Ok(QueryResult {
            completion,
            symbols: symbols_map,
            edges: verified_edges,
            paths: Vec::new(),
            counts,
            diagnostics: Vec::new(),
            metrics: metrics.clone(),
        })
    }
}

fn call_may_target_symbol(call: &crate::model::CandidateCallSite, target: &Symbol) -> bool {
    if call.callee_spelling != target.name {
        return false;
    }

    let Some(call_qualifier) = call.qualifier_hint.as_deref() else {
        return true;
    };
    let target_qualifier = target
        .qualified_name
        .as_deref()
        .and_then(|name| name.rfind("::").map(|index| &name[..index + 2]));
    target_qualifier == Some(call_qualifier)
}

fn candidate_to_symbol(candidate: &CandidateSymbol) -> Symbol {
    let qualified_name = candidate
        .qualifier_hint
        .as_deref()
        .map(|qualifier| format!("{qualifier}{}", candidate.name))
        .unwrap_or_else(|| candidate.name.clone());
    let (namespace, class_name) = candidate_scope_parts(candidate);
    Symbol {
        id: SymbolId::tree_sitter_fallback(
            candidate.language,
            candidate.declaration.start.clone(),
            format!("{:?}", candidate.syntactic_kind),
            qualified_name.clone(),
        ),
        name: candidate.name.clone(),
        qualified_name: Some(qualified_name),
        namespace,
        class_name,
        signature: candidate.signature_hint.clone(),
        declaration: Some(candidate.declaration.start.clone()),
        definition: candidate
            .definition_body
            .as_ref()
            .map(|range| range.start.clone()),
    }
}

fn candidate_display_name_with_location(candidate: &CandidateSymbol) -> String {
    let name = candidate
        .qualifier_hint
        .as_deref()
        .map(|qualifier| format!("{qualifier}{}", candidate.name))
        .unwrap_or_else(|| candidate.name.clone());
    format!("{name} at {}", candidate.declaration.start)
}

/// Tree-sitter 후보 한정자에서 출력용 네임스페이스/클래스 힌트를 분리한다.
fn candidate_scope_parts(candidate: &CandidateSymbol) -> (Option<String>, Option<String>) {
    let mut segments = candidate
        .qualifier_hint
        .as_deref()
        .unwrap_or_default()
        .trim_end_matches("::")
        .split("::")
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();

    match candidate.syntactic_kind {
        CandidateSymbolKind::Function => {
            let namespace = (!segments.is_empty()).then(|| segments.join("::"));
            (namespace, None)
        }
        CandidateSymbolKind::Method | CandidateSymbolKind::ConstructorOrDestructor => {
            let class_name = segments.pop().or_else(|| candidate.owner_hint.clone());
            let namespace = (!segments.is_empty()).then(|| segments.join("::"));
            (namespace, class_name)
        }
    }
}

/// 역방향 탐색으로 검증된 엣지에서 최상위 호출자별 최단 경로 구성
fn build_caller_paths(target: &SymbolId, edges: &[CallEdge]) -> Vec<CallPath> {
    let mut adjacency: BTreeMap<SymbolId, Vec<(SymbolId, CallEdgeId)>> = BTreeMap::new();
    let mut callers = BTreeSet::new();
    let mut callees = BTreeSet::new();

    for edge in edges {
        let Some(callee) = &edge.callee else {
            continue;
        };

        adjacency
            .entry(edge.caller.clone())
            .or_default()
            .push((callee.clone(), edge.id));
        callers.insert(edge.caller.clone());
        callees.insert(callee.clone());
    }

    for next in adjacency.values_mut() {
        next.sort();
        next.dedup();
    }

    let mut roots = callers
        .difference(&callees)
        .cloned()
        .collect::<Vec<_>>();

    // 순환 그래프에는 진입 차수가 0인 루트가 없을 수 있다. 이때는 대상이 아닌
    // 첫 발견 심볼 하나를 시작점으로 사용해 최소한의 순환 경로를 보여준다.
    if roots.is_empty() {
        if let Some(fallback) = callers.iter().find(|symbol| *symbol != target) {
            roots.push(fallback.clone());
        }
    }

    roots
        .into_iter()
        .filter_map(|root| shortest_path(&root, target, &adjacency))
        .collect()
}

/// 검증된 호출 인접 목록에서 두 심볼 사이의 결정론적 최단 경로 계산
fn shortest_path(
    source: &SymbolId,
    target: &SymbolId,
    adjacency: &BTreeMap<SymbolId, Vec<(SymbolId, CallEdgeId)>>,
) -> Option<CallPath> {
    let mut frontier = VecDeque::from([source.clone()]);
    let mut visited = BTreeSet::from([source.clone()]);
    let mut predecessors: BTreeMap<SymbolId, (SymbolId, CallEdgeId)> = BTreeMap::new();

    while let Some(current) = frontier.pop_front() {
        if &current == target {
            break;
        }

        for (next, edge_id) in adjacency.get(&current).into_iter().flatten() {
            if visited.insert(next.clone()) {
                predecessors.insert(next.clone(), (current.clone(), *edge_id));
                frontier.push_back(next.clone());
            }
        }
    }

    if !visited.contains(target) {
        return None;
    }

    let mut nodes = vec![target.clone()];
    let mut path_edges = Vec::new();
    let mut current = target.clone();

    while &current != source {
        let (previous, edge_id) = predecessors.get(&current)?;
        nodes.push(previous.clone());
        path_edges.push(*edge_id);
        current = previous.clone();
    }

    nodes.reverse();
    path_edges.reverse();
    Some(CallPath {
        nodes,
        edges: path_edges,
    })
}

/// 시맨틱 검증으로 얻은 caller/callee 메타데이터를 결과 심볼 맵에 병합
fn record_verified_symbols(
    symbols_map: &mut BTreeMap<SymbolId, Symbol>,
    symbols: Vec<Symbol>,
) {
    for symbol in symbols {
        symbols_map.entry(symbol.id.clone()).or_insert(symbol);
    }
}

/// 엣지 내에 기록된 심볼 메타데이터를 symbols_map에 등록하는 헬퍼
fn record_symbols_from_edge(symbols_map: &mut BTreeMap<SymbolId, Symbol>, edge: &CallEdge) {
    for evidence in edge.evidence_by_context.values() {
        if let Some(target) = &evidence.static_target {
            symbols_map
                .entry(target.id.clone())
                .or_insert_with(|| target.clone());
        }
        for cand in &evidence.candidate_targets {
            symbols_map
                .entry(cand.id.clone())
                .or_insert_with(|| cand.clone());
        }
    }
}

/// 정적 대상 또는 virtual override 후보 집합에 요청 심볼이 포함되는지 확인한다.
///
/// Clang을 사용할 수 없었던 이전 단계에서 생성된 Tree-sitter 심볼은 이후 단계의
/// Clang USR과 ID가 다르므로, 이 경우에만 한정 이름을 보조 연결 키로 사용한다.
fn edge_targets_symbol(
    edge: &CallEdge,
    target: &Symbol,
    symbols: &BTreeMap<SymbolId, Symbol>,
) -> bool {
    if edge.callee.as_ref() == Some(&target.id) {
        return true;
    }

    if edge.evidence_by_context.values().any(|evidence| {
        evidence
            .candidate_targets
            .iter()
            .any(|candidate| candidate.id == target.id)
    }) {
        return true;
    }

    if !is_tree_sitter_symbol(target) {
        return false;
    }

    let callee_matches = edge
        .callee
        .as_ref()
        .and_then(|id| symbols.get(id))
        .is_some_and(|callee| symbols_match_syntactically(callee, target));
    callee_matches
        || edge.evidence_by_context.values().any(|evidence| {
            evidence
                .static_target
                .iter()
                .chain(evidence.candidate_targets.iter())
                .any(|candidate| symbols_match_syntactically(candidate, target))
        })
}

/// 순방향 검증 엣지가 현재 Tree-sitter/Clang 호출자와 같은 심볼인지 확인한다.
fn edge_caller_matches_symbol(
    edge: &CallEdge,
    caller: &Symbol,
    symbols: &BTreeMap<SymbolId, Symbol>,
) -> bool {
    if edge.caller == caller.id {
        return true;
    }
    if !is_tree_sitter_symbol(caller) {
        return false;
    }

    symbols
        .get(&edge.caller)
        .is_some_and(|verified| symbols_match_syntactically(verified, caller))
}

fn is_tree_sitter_symbol(symbol: &Symbol) -> bool {
    matches!(
        &symbol.id.backend_id,
        BackendSymbolId::TreeSitterLocationFallback { .. }
    )
}

fn symbols_match_syntactically(left: &Symbol, right: &Symbol) -> bool {
    if left.name != right.name {
        return false;
    }

    match (
        meaningful_qualifier(left),
        meaningful_qualifier(right),
    ) {
        (Some(left), Some(right)) => left == right,
        _ => true,
    }
}

fn meaningful_qualifier(symbol: &Symbol) -> Option<&str> {
    symbol
        .qualified_name
        .as_deref()
        .filter(|qualified| *qualified != symbol.name.as_str())
}

/// 결과 요약 통계 계산
fn calculate_counts(
    symbols: &BTreeMap<SymbolId, Symbol>,
    edges: &[CallEdge],
    paths: &[CallPath],
) -> ResultCounts {
    let mut confirmed = 0;
    let mut possible = 0;
    let mut unresolved = 0;

    for edge in edges {
        match edge.confidence {
            Confidence::Confirmed => confirmed += 1,
            Confidence::Possible => possible += 1,
            Confidence::Unresolved => unresolved += 1,
        }
    }

    ResultCounts {
        total_symbols: symbols.len(),
        confirmed_edges: confirmed,
        possible_edges: possible,
        unresolved_edges: unresolved,
        paths_count: paths.len(),
    }
}
