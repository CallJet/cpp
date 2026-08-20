//! Clang 기반 시맨틱 공급자 구현 모듈
//! Clang semantic provider implementation module

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{CStr, CString};
use std::path::Path;
use std::sync::Once;

use clang_sys::*;

use crate::diagnostic::{AnalysisCause, AnalysisIssue, Severity};
use crate::model::{
    BackendSymbolId, CallEdge, CallEdgeId, CallKind, CandidateSymbolId, CompilationContext,
    CompilationKey, Confidence, Language, LineColumn, SourceLocation, Symbol, SymbolId,
    VerificationEvidence, VerificationReason,
};
use crate::project::ProjectContext;
use crate::semantic::{ResolutionBatch, SemanticProvider, VerificationBatch, VerificationResult};

static INIT_CLANG: Once = Once::new();

/// libclang 초기화 헬퍼
pub fn ensure_libclang_loaded() -> bool {
    let mut success = true;
    INIT_CLANG.call_once(|| {
        if !clang_sys::is_loaded() && clang_sys::load().is_err() {
            // 일반적인 LLVM 기본 경로 시도
            let possible_paths = [
                r"C:\Program Files\LLVM\bin",
                r"C:\Program Files (x86)\LLVM\bin",
                r"C:\LLVM\bin",
            ];
            let mut loaded = false;
            for p in possible_paths {
                let path = Path::new(p);
                if path.exists() {
                    std::env::set_var("LIBCLANG_PATH", path);
                    if clang_sys::load().is_ok() {
                        loaded = true;
                        break;
                    }
                }
            }
            if !loaded {
                success = false;
            }
        }
    });
    clang_sys::is_loaded()
}

/// Translation Unit 캐시 엔트리
enum TuCacheEntry {
    Parsed(CXTranslationUnit),
    Failed(String),
}

/// Clang 시맨틱 분석 공급자 (Clang Semantic Provider)
pub struct ClangProvider {
    index: CXIndex,
    tu_cache: BTreeMap<CompilationKey, TuCacheEntry>,
    symbol_cache: BTreeMap<(CandidateSymbolId, CompilationKey), Option<Symbol>>,
    next_edge_id: u32,
    pub tu_parse_count: usize,
    pub tu_cache_hits: usize,
}

impl Default for ClangProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ClangProvider {
    pub fn new() -> Self {
        ensure_libclang_loaded();
        let index = unsafe {
            if clang_sys::is_loaded() {
                clang_createIndex(0, 0)
            } else {
                std::ptr::null_mut()
            }
        };

        Self {
            index,
            tu_cache: BTreeMap::new(),
            symbol_cache: BTreeMap::new(),
            next_edge_id: 1,
            tu_parse_count: 0,
            tu_cache_hits: 0,
        }
    }

    /// Translation Unit 획득 또는 파싱 (1 TU parse per CompilationKey 캐싱 불변식)
    fn get_or_parse_tu(
        &mut self,
        context: &CompilationContext,
    ) -> Result<CXTranslationUnit, String> {
        if self.index.is_null() {
            return Err("libclang 라이브러리가 로드되지 않았습니다.".to_string());
        }

        if let Some(entry) = self.tu_cache.get(&context.key) {
            self.tu_cache_hits += 1;
            return match entry {
                TuCacheEntry::Parsed(tu) => Ok(*tu),
                TuCacheEntry::Failed(err) => Err(err.clone()),
            };
        }

        self.tu_parse_count += 1;

        let source_file_str = match context.source_file.to_str() {
            Some(s) => s,
            None => {
                let err = format!("소스 파일 경로 UTF-8 변환 실패: {:?}", context.source_file);
                self.tu_cache
                    .insert(context.key.clone(), TuCacheEntry::Failed(err.clone()));
                return Err(err);
            }
        };

        let c_source_file = CString::new(source_file_str).unwrap_or_default();

        // 컴파일 인자 CString 변환
        let mut c_args: Vec<CString> = Vec::new();
        for arg in &context.clang_args {
            if let Some(s) = arg.to_str() {
                if let Ok(cs) = CString::new(s) {
                    c_args.push(cs);
                }
            }
        }

        let mut c_arg_ptrs: Vec<*const std::os::raw::c_char> =
            c_args.iter().map(|cs| cs.as_ptr()).collect();

        let mut tu: CXTranslationUnit = std::ptr::null_mut();

        let options = CXTranslationUnit_DetailedPreprocessingRecord | CXTranslationUnit_KeepGoing;

        let error_code = unsafe {
            clang_parseTranslationUnit2(
                self.index,
                c_source_file.as_ptr(),
                c_arg_ptrs.as_mut_ptr(),
                c_arg_ptrs.len() as i32,
                std::ptr::null_mut(),
                0,
                options,
                &mut tu,
            )
        };

        if error_code != CXError_Success || tu.is_null() {
            let err = format!(
                "Clang TU 파싱 실패 (에러 코드: {error_code:?}) for context: {}",
                context.key.0
            );
            self.tu_cache
                .insert(context.key.clone(), TuCacheEntry::Failed(err.clone()));
            Err(err)
        } else {
            self.tu_cache
                .insert(context.key.clone(), TuCacheEntry::Parsed(tu));
            Ok(tu)
        }
    }
}

impl Drop for ClangProvider {
    fn drop(&mut self) {
        unsafe {
            for entry in self.tu_cache.values() {
                if let TuCacheEntry::Parsed(tu) = entry {
                    clang_disposeTranslationUnit(*tu);
                }
            }
            if !self.index.is_null() {
                clang_disposeIndex(self.index);
            }
        }
    }
}

impl SemanticProvider for ClangProvider {
    fn resolve_symbols(
        &mut self,
        project: &ProjectContext,
        candidates: &[CandidateSymbolId],
    ) -> ResolutionBatch {
        let mut result = ResolutionBatch::default();
        let discovery = &project.compilation_db;

        for &cand_id in candidates {
            // 후보 심볼의 소스 파일 컨텍스트 매핑
            let all_contexts = project.compilation_db.all_source_files();
            let mut resolved_symbol = None;

            for file in &all_contexts {
                for ctx in discovery.contexts_for_source(file) {
                    if let Some(cached) = self.symbol_cache.get(&(cand_id, ctx.key.clone())) {
                        if let Some(sym) = cached {
                            resolved_symbol = Some(sym.clone());
                            break;
                        }
                        continue;
                    }

                    match self.get_or_parse_tu(ctx) {
                        Ok(tu) => {
                            if let Some(sym) = self.resolve_symbol_in_tu(tu, cand_id, project) {
                                self.symbol_cache
                                    .insert((cand_id, ctx.key.clone()), Some(sym.clone()));
                                resolved_symbol = Some(sym);
                                break;
                            } else {
                                self.symbol_cache.insert((cand_id, ctx.key.clone()), None);
                            }
                        }
                        Err(err) => {
                            result.issues.push(AnalysisIssue {
                                severity: Severity::Recoverable,
                                context: Some(ctx.key.clone()),
                                location: None,
                                message: err,
                                cause: AnalysisCause::TranslationUnitParseFailed,
                            });
                        }
                    }
                }
                if resolved_symbol.is_some() {
                    break;
                }
            }

            if let Some(sym) = resolved_symbol {
                if !result.symbols.iter().any(|s| s.id == sym.id) {
                    result.symbols.push(sym);
                }
            }
        }

        result
    }

    fn verify_calls(
        &mut self,
        project: &ProjectContext,
        batch: VerificationBatch,
    ) -> VerificationResult {
        let mut result = VerificationResult::default();

        // 1. 해당 CompilationKey에 해당하는 CompilationContext 찾기
        let mut target_context = None;
        for file in project.compilation_db.all_source_files() {
            for ctx in project.compilation_db.contexts_for_source(&file) {
                if ctx.key == batch.context {
                    target_context = Some(ctx.clone());
                    break;
                }
            }
            if target_context.is_some() {
                break;
            }
        }

        let context = match target_context {
            Some(c) => c,
            None => {
                result.issues.push(AnalysisIssue {
                    severity: Severity::Recoverable,
                    context: Some(batch.context.clone()),
                    location: None,
                    message: "컴파일 컨텍스트를 찾을 수 없습니다.".to_string(),
                    cause: AnalysisCause::MissingCompilationContext,
                });
                return result;
            }
        };

        // 2. TU 획득
        let tu = match self.get_or_parse_tu(&context) {
            Ok(tu) => tu,
            Err(err) => {
                result.issues.push(AnalysisIssue {
                    severity: Severity::Recoverable,
                    context: Some(context.key.clone()),
                    location: None,
                    message: err,
                    cause: AnalysisCause::TranslationUnitParseFailed,
                });
                return result;
            }
        };

        // 3. 배치 내 각 후보 호출 검증
        let discovery = crate::discovery::DiscoveryIndex::build(project);

        for call_id in batch.calls {
            if let Some(call_site) = discovery.calls.get(&call_id) {
                if let Some(edge) = self.verify_single_call(tu, call_site, &context, &discovery) {
                    result.edges.push(edge);
                }
            }
        }

        result
    }
}

impl ClangProvider {
    /// TU 내에서 특정 후보 심볼을 Clang 커서로 찾아 canonical Symbol 생성
    fn resolve_symbol_with_cand(
        &self,
        tu: CXTranslationUnit,
        cand: &crate::model::CandidateSymbol,
    ) -> Option<Symbol> {
        let file_path = &cand.declaration.start.file;
        let c_file = CString::new(file_path.to_str()?).ok()?;

        let point = cand
            .declaration
            .start
            .point
            .unwrap_or(LineColumn { line: 1, column: 1 });

        unsafe {
            let cx_file = clang_getFile(tu, c_file.as_ptr());
            if cx_file.is_null() {
                return None;
            }

            let location = clang_getLocation(tu, cx_file, point.line, point.column);
            let cursor = clang_getCursor(tu, location);
            if clang_Cursor_isNull(cursor) != 0 {
                return None;
            }

            let canonical_cursor = clang_getCanonicalCursor(cursor);
            let cursor_to_use = if clang_Cursor_isNull(canonical_cursor) == 0 {
                canonical_cursor
            } else {
                cursor
            };

            self.cursor_to_symbol(cursor_to_use, cand.language)
        }
    }

    /// TU 내에서 특정 후보 심볼 ID를 Clang 커서로 찾아 canonical Symbol 생성
    fn resolve_symbol_in_tu(
        &self,
        tu: CXTranslationUnit,
        cand_id: CandidateSymbolId,
        project: &ProjectContext,
    ) -> Option<Symbol> {
        let discovery = crate::discovery::DiscoveryIndex::build(project);
        let cand = discovery.symbols.get(&cand_id)?;
        self.resolve_symbol_with_cand(tu, cand)
    }

    /// Clang 커서로부터 정규화된 Symbol 데이터 구조 생성
    unsafe fn cursor_to_symbol(&self, cursor: CXCursor, language: Language) -> Option<Symbol> {
        let usr_str = get_cx_string(clang_getCursorUSR(cursor));
        let spelling = get_cx_string(clang_getCursorSpelling(cursor));

        if spelling.is_empty() && usr_str.is_empty() {
            return None;
        }

        let name = if !spelling.is_empty() {
            spelling
        } else {
            "unnamed".to_string()
        };

        let backend_id = if !usr_str.is_empty() {
            BackendSymbolId::ClangUsr(usr_str)
        } else {
            let loc = self.get_cursor_location(cursor);
            BackendSymbolId::ClangLocationFallback {
                canonical_declaration: loc
                    .unwrap_or_else(|| SourceLocation::file_only("<unknown>")),
                cursor_kind: format!("{:?}", cursor.kind),
                qualified_name: name.clone(),
                signature: None,
            }
        };

        let symbol_id = SymbolId {
            language,
            backend_id,
        };

        let qualified_name = self.extract_qualified_name(cursor);
        let signature = self.extract_signature(cursor);
        let declaration = self.get_cursor_location(cursor);

        Some(Symbol {
            id: symbol_id,
            name,
            qualified_name: Some(qualified_name),
            signature,
            declaration,
            definition: None,
        })
    }

    /// 단일 후보 호출에 대한 Clang 시맨틱 검증 수행
    fn verify_single_call(
        &mut self,
        tu: CXTranslationUnit,
        call_site: &crate::model::CandidateCallSite,
        context: &CompilationContext,
        discovery: &crate::discovery::DiscoveryIndex,
    ) -> Option<CallEdge> {
        let file_path = &call_site.expression.start.file;
        let c_file = CString::new(file_path.to_str()?).ok()?;
        let point = call_site
            .expression
            .start
            .point
            .unwrap_or(LineColumn { line: 1, column: 1 });

        unsafe {
            let cx_file = clang_getFile(tu, c_file.as_ptr());
            if cx_file.is_null() {
                return None;
            }

            let location = clang_getLocation(tu, cx_file, point.line, point.column);
            let cursor = clang_getCursor(tu, location);

            if clang_Cursor_isNull(cursor) != 0 {
                return None;
            }

            // caller 심볼 식별
            let caller_cand = discovery.symbols.get(&call_site.caller)?;
            let caller_sym = self.resolve_symbol_with_cand(tu, caller_cand)?;

            // 피호출자 참조 커서 탐색
            let ref_cursor = clang_getCursorReferenced(cursor);
            let canonical_ref = if clang_Cursor_isNull(ref_cursor) == 0 {
                clang_getCanonicalCursor(ref_cursor)
            } else {
                clang_getNullCursor()
            };

            let is_virtual =
                clang_isVirtualBase(cursor) != 0 || clang_CXXMethod_isVirtual(ref_cursor) != 0;

            let (callee_sym, kind, confidence, reason) = if clang_Cursor_isNull(canonical_ref) == 0
            {
                let callee = self.cursor_to_symbol(canonical_ref, caller_cand.language);
                if is_virtual {
                    (
                        callee,
                        CallKind::Virtual,
                        Confidence::Possible,
                        VerificationReason::MultipleRuntimeTargets,
                    )
                } else {
                    (
                        callee,
                        CallKind::Direct,
                        Confidence::Confirmed,
                        VerificationReason::ExactReference,
                    )
                }
            } else {
                // 대상을 확정할 수 없는 경우 (함수 포인터 등)
                (
                    None,
                    CallKind::Unresolved,
                    Confidence::Unresolved,
                    VerificationReason::IndirectTargetUnknown,
                )
            };

            let callee_id = callee_sym.as_ref().map(|s| s.id.clone());

            let mut contexts_set = BTreeSet::new();
            contexts_set.insert(context.key.clone());

            let mut evidence_map = BTreeMap::new();
            let evidence = VerificationEvidence {
                expression_text: call_site.expression_text.clone(),
                static_target: callee_sym.clone(),
                candidate_targets: if let Some(s) = callee_sym {
                    vec![s]
                } else {
                    Vec::new()
                },
                clang_diagnostics: Vec::new(),
                reason,
                spelling_location: Some(call_site.expression.start.clone()),
                expansion_location: Some(call_site.expression.start.clone()),
                is_virtual,
                is_template_related: false,
                is_macro_expanded: false,
            };
            evidence_map.insert(context.key.clone(), evidence);

            let edge_id = CallEdgeId(self.next_edge_id);
            self.next_edge_id += 1;

            Some(CallEdge {
                id: edge_id,
                caller: caller_sym.id,
                callee: callee_id,
                callsite: call_site.expression.clone(),
                kind,
                confidence,
                contexts: contexts_set,
                evidence_by_context: evidence_map,
            })
        }
    }

    /// 커서의 소스 위치 반환
    unsafe fn get_cursor_location(&self, cursor: CXCursor) -> Option<SourceLocation> {
        let loc = clang_getCursorLocation(cursor);
        let mut file: CXFile = std::ptr::null_mut();
        let mut line: u32 = 0;
        let mut column: u32 = 0;
        let mut offset: u32 = 0;

        clang_getSpellingLocation(loc, &mut file, &mut line, &mut column, &mut offset);

        if file.is_null() {
            return None;
        }

        let file_name = get_cx_string(clang_getFileName(file));
        if file_name.is_empty() {
            return None;
        }

        Some(SourceLocation::new(file_name, line, column))
    }

    /// 커서로부터 정규화된 한정 이름 추출
    unsafe fn extract_qualified_name(&self, cursor: CXCursor) -> String {
        let mut segments = Vec::new();
        let mut cur = cursor;

        while clang_Cursor_isNull(cur) == 0 && cur.kind != CXCursor_TranslationUnit {
            let spelling = get_cx_string(clang_getCursorSpelling(cur));
            if !spelling.is_empty() {
                segments.push(spelling);
            }
            cur = clang_getCursorSemanticParent(cur);
        }

        segments.reverse();
        if segments.is_empty() {
            "unnamed".to_string()
        } else {
            segments.join("::")
        }
    }

    /// 커서로부터 함수 시그니처 문자열 추출
    unsafe fn extract_signature(&self, cursor: CXCursor) -> Option<String> {
        let cur_type = clang_getCursorType(cursor);
        let type_spelling = get_cx_string(clang_getTypeSpelling(cur_type));
        if !type_spelling.is_empty() {
            Some(type_spelling)
        } else {
            None
        }
    }
}

/// CXString 문자열 변환 및 자동 메모리 해제 헬퍼
unsafe fn get_cx_string(s: CXString) -> String {
    if s.data.is_null() {
        return String::new();
    }
    let c_str = clang_getCString(s);
    let result = if !c_str.is_null() {
        CStr::from_ptr(c_str).to_string_lossy().to_string()
    } else {
        String::new()
    };
    clang_disposeString(s);
    result
}
