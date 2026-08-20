//! 온디맨드 쿼리 및 순회 엔진 모듈
//! On-demand query and traversal engine module

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::time::Instant;

use crate::diagnostic::{FatalError, QueryError};
use crate::discovery::DiscoveryIndex;
use crate::model::{
    CallEdge, CallEdgeId, CallPath, CandidateCallId, CompilationKey, Completion, Confidence,
    QueryMetrics, QueryRequest, QueryResult, ResultCounts, Symbol, SymbolId, SymbolQuery,
    VerifiedEdgeKey,
};
use crate::project::ProjectContext;
use crate::semantic::{SemanticProvider, VerificationBatch};

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

/// 온디맨드 쿼리 엔진 (Query Engine)
pub struct QueryEngine<'a, S: SemanticProvider> {
    pub project: &'a ProjectContext,
    pub discovery: DiscoveryIndex,
    pub provider: S,
}

impl<'a, S: SemanticProvider> QueryEngine<'a, S> {
    /// 새 쿼리 엔진 인스턴스 생성
    pub fn new(project: &'a ProjectContext, provider: S) -> Self {
        let discovery = DiscoveryIndex::build(project);
        Self {
            project,
            discovery,
            provider,
        }
    }

    /// 쿼리 요청 실행
    pub fn execute(&mut self, request: QueryRequest) -> Result<QueryResult, FatalError> {
        let total_start = Instant::now();

        let mut metrics = QueryMetrics {
            source_files_inspected: self.project.source_files().len(),
            source_files_parsed_by_treesitter: self.project.source_files().len(),
            candidate_call_sites: self.discovery.calls.len(),
            available_translation_units: self.project.compilation_db.all_source_files().len(),
            ..Default::default()
        };

        let result = match request {
            QueryRequest::Callers {
                target,
                max_depth,
                verified_only,
            } => self.execute_callers(target, max_depth, verified_only, &mut metrics)?,
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

        metrics.total_query_time = total_start.elapsed();
        let mut final_res = result;
        final_res.metrics = metrics;

        Ok(final_res)
    }

    /// 단일 심볼 쿼리를 canonical Symbol로 해석
    fn resolve_endpoint(&mut self, query: &SymbolQuery) -> Result<Symbol, FatalError> {
        let cand_ids = self.discovery.matching_symbols(query);
        if cand_ids.is_empty() {
            return Err(FatalError::Query(QueryError::SymbolNotFound {
                query: query.raw.clone(),
            }));
        }

        let resolution = self.provider.resolve_symbols(self.project, cand_ids);

        if resolution.symbols.is_empty() {
            // Clang에서 해석하지 못한 경우, 힌트 기반으로 임시 fallback 심볼 생성
            let first_cand = self.discovery.symbols.get(&cand_ids[0]).unwrap();
            let sym = Symbol {
                id: SymbolId::clang_usr(first_cand.language, format!("usr:@{}", first_cand.name)),
                name: first_cand.name.clone(),
                qualified_name: first_cand
                    .qualifier_hint
                    .clone()
                    .map(|q| format!("{q}{}", first_cand.name)),
                signature: None,
                declaration: Some(first_cand.declaration.start.clone()),
                definition: None,
            };
            return Ok(sym);
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

    /// callers 쿼리 실행 (온디맨드 역방향 탐색)
    fn execute_callers(
        &mut self,
        target_query: SymbolQuery,
        max_depth: Option<usize>,
        verified_only: bool,
        metrics: &mut QueryMetrics,
    ) -> Result<QueryResult, FatalError> {
        let target_sym = self.resolve_endpoint(&target_query)?;

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

        while let Some(item) = state.frontier.pop_front() {
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
            let candidate_calls = self.discovery.candidate_callers(&cur_sym);
            metrics.semantic_candidates_verified += candidate_calls.len();

            // 2. 컴파일 컨텍스트별 그룹화
            let mut batches: BTreeMap<CompilationKey, Vec<CandidateCallId>> = BTreeMap::new();
            for &call_id in &candidate_calls {
                let contexts = self.discovery.contexts_for(call_id, self.project);
                if contexts.is_empty() {
                    // 컨텍스트가 없으면 기본 첫 번째 컨텍스트 시도
                    if let Some(first_file) = self.project.compilation_db.all_source_files().first()
                    {
                        if let Some(ctx) = self
                            .project
                            .compilation_db
                            .contexts_for_source(first_file)
                            .first()
                        {
                            batches.entry(ctx.key.clone()).or_default().push(call_id);
                        }
                    }
                } else {
                    for ctx_key in contexts {
                        batches.entry(ctx_key).or_default().push(call_id);
                    }
                }
            }

            // 3. Clang 온디맨드 시맨틱 검증 수행
            let verify_start = Instant::now();
            for (ctx_key, calls) in batches {
                verified_tu_keys.insert(ctx_key.clone());
                let batch = VerificationBatch {
                    context: ctx_key,
                    symbols: Vec::new(),
                    calls,
                };
                let ver_res = self.provider.verify_calls(self.project, batch);

                for edge in ver_res.edges {
                    // verified_only 필터링
                    if verified_only && edge.confidence != Confidence::Confirmed {
                        continue;
                    }

                    // 피호출자가 현재 심볼과 매칭되는지 확인
                    let matches_callee = match &edge.callee {
                        Some(callee_id) => callee_id == &item.symbol,
                        None => edge.confidence == Confidence::Unresolved,
                    };

                    if matches_callee {
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

    /// callees 쿼리 실행 (온디맨드 순방향 탐색)
    fn execute_callees(
        &mut self,
        source_query: SymbolQuery,
        max_depth: Option<usize>,
        verified_only: bool,
        metrics: &mut QueryMetrics,
    ) -> Result<QueryResult, FatalError> {
        let source_sym = self.resolve_endpoint(&source_query)?;

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

        while let Some(item) = state.frontier.pop_front() {
            if let Some(limit) = max_depth {
                if item.depth >= limit {
                    truncated = true;
                    continue;
                }
            }

            // 1. 현재 심볼에 해당하는 candidate symbol 탐색
            let cand_syms = self
                .discovery
                .matching_symbols(&SymbolQuery::parse(&source_sym.name));
            let mut calls_to_verify = Vec::new();
            for &cand_id in cand_syms {
                let calls = self.discovery.candidate_callees(cand_id);
                calls_to_verify.extend_from_slice(calls);
            }

            metrics.semantic_candidates_verified += calls_to_verify.len();

            // 2. 컴파일 컨텍스트 그룹화
            let mut batches: BTreeMap<CompilationKey, Vec<CandidateCallId>> = BTreeMap::new();
            for &call_id in &calls_to_verify {
                let contexts = self.discovery.contexts_for(call_id, self.project);
                if contexts.is_empty() {
                    if let Some(first_file) = self.project.compilation_db.all_source_files().first()
                    {
                        if let Some(ctx) = self
                            .project
                            .compilation_db
                            .contexts_for_source(first_file)
                            .first()
                        {
                            batches.entry(ctx.key.clone()).or_default().push(call_id);
                        }
                    }
                } else {
                    for ctx_key in contexts {
                        batches.entry(ctx_key).or_default().push(call_id);
                    }
                }
            }

            // 3. Clang 검증
            let verify_start = Instant::now();
            for (ctx_key, calls) in batches {
                verified_tu_keys.insert(ctx_key.clone());
                let batch = VerificationBatch {
                    context: ctx_key,
                    symbols: Vec::new(),
                    calls,
                };
                let ver_res = self.provider.verify_calls(self.project, batch);

                for edge in ver_res.edges {
                    if verified_only && edge.confidence != Confidence::Confirmed {
                        continue;
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
        let source_sym = self.resolve_endpoint(&source_query)?;
        let target_sym = self.resolve_endpoint(&target_query)?;

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

        while let Some(item) = state.frontier.pop_front() {
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

            let cand_syms = self
                .discovery
                .matching_symbols(&SymbolQuery::parse(&source_sym.name));
            let mut calls_to_verify = Vec::new();
            for &cand_id in cand_syms {
                let calls = self.discovery.candidate_callees(cand_id);
                calls_to_verify.extend_from_slice(calls);
            }

            let mut batches: BTreeMap<CompilationKey, Vec<CandidateCallId>> = BTreeMap::new();
            for &call_id in &calls_to_verify {
                let contexts = self.discovery.contexts_for(call_id, self.project);
                if contexts.is_empty() {
                    if let Some(first_file) = self.project.compilation_db.all_source_files().first()
                    {
                        if let Some(ctx) = self
                            .project
                            .compilation_db
                            .contexts_for_source(first_file)
                            .first()
                        {
                            batches.entry(ctx.key.clone()).or_default().push(call_id);
                        }
                    }
                } else {
                    for ctx_key in contexts {
                        batches.entry(ctx_key).or_default().push(call_id);
                    }
                }
            }

            for (ctx_key, calls) in batches {
                verified_tu_keys.insert(ctx_key.clone());
                let batch = VerificationBatch {
                    context: ctx_key,
                    symbols: Vec::new(),
                    calls,
                };
                let ver_res = self.provider.verify_calls(self.project, batch);

                for edge in ver_res.edges {
                    if verified_only && edge.confidence != Confidence::Confirmed {
                        continue;
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
        for &call_id in &calls_to_verify {
            let contexts = self.discovery.contexts_for(call_id, self.project);
            if contexts.is_empty() {
                if let Some(first_file) = self.project.compilation_db.all_source_files().first() {
                    if let Some(ctx) = self
                        .project
                        .compilation_db
                        .contexts_for_source(first_file)
                        .first()
                    {
                        batches.entry(ctx.key.clone()).or_default().push(call_id);
                    }
                }
            } else {
                for ctx_key in contexts {
                    batches.entry(ctx_key).or_default().push(call_id);
                }
            }
        }

        for (ctx_key, calls) in batches {
            let batch = VerificationBatch {
                context: ctx_key,
                symbols: Vec::new(),
                calls,
            };
            let ver_res = self.provider.verify_calls(self.project, batch);
            for edge in ver_res.edges {
                if edge.callee.as_ref() == Some(&callee_sym.id) {
                    record_symbols_from_edge(&mut symbols_map, &edge);
                    verified_edges.push(edge);
                }
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
