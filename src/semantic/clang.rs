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
            // 운영체제별 표준 LLVM / libclang 기본 경로 목록
            let possible_paths = [
                // Windows
                r"C:\Program Files\LLVM\bin",
                r"C:\Program Files (x86)\LLVM\bin",
                r"C:\LLVM\bin",
                // macOS (Homebrew Apple Silicon & Intel, Xcode CLT)
                "/opt/homebrew/opt/llvm/lib",
                "/usr/local/opt/llvm/lib",
                "/Library/Developer/CommandLineTools/usr/lib",
                // Linux (Debian/Ubuntu, Fedora/RHEL, Arch)
                "/usr/lib/llvm-19/lib",
                "/usr/lib/llvm-18/lib",
                "/usr/lib/llvm-17/lib",
                "/usr/lib/llvm-16/lib",
                "/usr/lib/x86_64-linux-gnu",
                "/usr/lib/aarch64-linux-gnu",
                "/usr/lib64",
                "/usr/lib",
                "/usr/local/lib",
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
        discovery: &crate::discovery::DiscoveryIndex,
        candidates: &[CandidateSymbolId],
    ) -> ResolutionBatch {
        let mut result = ResolutionBatch::default();

        for &cand_id in candidates {
            let Some(candidate) = discovery.symbols.get(&cand_id) else {
                continue;
            };

            // 후보가 발견된 소스/헤더와 직접 연관된 컨텍스트만 우선 시도한다.
            let context_keys = discovery.contexts_for_symbol(cand_id, project);
            let contexts = context_keys
                .iter()
                .filter_map(|key| project.compilation_db.context_by_key(key))
                .collect::<Vec<_>>();

            // 관련 컨텍스트를 찾지 못했다고 전체 compile_commands를 순회하지 않는다.
            // QueryEngine은 이 경우 Tree-sitter 후보를 임시 심볼로 보존한다.
            if contexts.is_empty() {
                continue;
            }

            let mut resolved_symbol = None;

            for ctx in contexts {
                if let Some(cached) = self.symbol_cache.get(&(cand_id, ctx.key.clone())) {
                    if let Some(sym) = cached {
                        resolved_symbol = Some(sym.clone());
                        break;
                    }
                    continue;
                }

                match self.get_or_parse_tu(ctx) {
                    Ok(tu) => {
                        if let Some(sym) = self.resolve_symbol_with_cand(tu, candidate) {
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
        discovery: &crate::discovery::DiscoveryIndex,
        batch: VerificationBatch,
    ) -> VerificationResult {
        let mut result = VerificationResult::default();

        // 1. 해당 CompilationKey에 해당하는 CompilationContext 찾기
        let context = match project.compilation_db.context_by_key(&batch.context) {
            Some(context) => context.clone(),
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

        // 3. 배치 내 각 후보 호출 검증. 최초 구축한 discovery 인덱스를 재사용한다.
        for call_id in batch.calls {
            if let Some(call_site) = discovery.calls.get(&call_id) {
                if let Some((edge, caller, callee)) =
                    self.verify_single_call(tu, call_site, &context, discovery)
                {
                    for symbol in std::iter::once(caller).chain(callee) {
                        if !result.symbols.iter().any(|item| item.id == symbol.id) {
                            result.symbols.push(symbol);
                        }
                    }
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
        let (namespace, class_name) = self.extract_scope_parts(cursor);
        let signature = self.extract_signature(cursor);
        let declaration = self.get_cursor_location(cursor);
        let definition_cursor = clang_getCursorDefinition(cursor);
        let definition = if clang_Cursor_isNull(definition_cursor) == 0 {
            self.get_cursor_location(definition_cursor)
        } else {
            None
        };

        Some(Symbol {
            id: symbol_id,
            name,
            qualified_name: Some(qualified_name),
            namespace,
            class_name,
            signature,
            declaration,
            definition,
        })
    }

    /// 현재 커서 또는 호출 표현식의 멤버 이름 위치에서 실제 callable 선언을 찾는다.
    unsafe fn find_callee_reference(
        &self,
        tu: CXTranslationUnit,
        cx_file: CXFile,
        cursor: CXCursor,
        call_site: &crate::model::CandidateCallSite,
    ) -> Option<CXCursor> {
        if let Some(referenced) = self.callable_reference(cursor, &call_site.callee_spelling) {
            return Some(referenced);
        }

        // `obj.method()`의 Tree-sitter 범위는 `obj`에서 시작하므로 위 cursor가
        // 객체 변수 참조일 수 있다. 실제 `method` 토큰 위치의 MemberRefExpr에
        // clang_getCursorReferenced를 다시 적용한다.
        let member_point = call_site
            .callee_location
            .as_ref()
            .and_then(|location| location.point)
            .or_else(|| callee_spelling_point(call_site))?;
        let member_location = clang_getLocation(
            tu,
            cx_file,
            member_point.line,
            member_point.column,
        );
        let member_cursor = clang_getCursor(tu, member_location);
        if clang_Cursor_isNull(member_cursor) != 0 {
            return None;
        }

        self.callable_reference(member_cursor, &call_site.callee_spelling)
    }

    /// 참조 커서가 기대한 함수/메서드 선언을 가리키는 경우 canonical cursor를 반환한다.
    unsafe fn callable_reference(&self, cursor: CXCursor, expected_name: &str) -> Option<CXCursor> {
        if is_callable_cursor_kind(cursor.kind) {
            let spelling = get_cx_string(clang_getCursorSpelling(cursor));
            if spelling == expected_name {
                let canonical = clang_getCanonicalCursor(cursor);
                return Some(if clang_Cursor_isNull(canonical) == 0 {
                    canonical
                } else {
                    cursor
                });
            }
        }

        let referenced = clang_getCursorReferenced(cursor);
        if clang_Cursor_isNull(referenced) != 0 {
            return None;
        }

        let canonical = clang_getCanonicalCursor(referenced);
        let callable = if clang_Cursor_isNull(canonical) == 0 {
            canonical
        } else {
            referenced
        };
        if !is_callable_cursor_kind(callable.kind) {
            return None;
        }

        let spelling = get_cx_string(clang_getCursorSpelling(callable));
        (spelling == expected_name).then_some(callable)
    }

    /// 단일 후보 호출에 대한 Clang 시맨틱 검증 수행
    fn verify_single_call(
        &mut self,
        tu: CXTranslationUnit,
        call_site: &crate::model::CandidateCallSite,
        context: &CompilationContext,
        discovery: &crate::discovery::DiscoveryIndex,
    ) -> Option<(CallEdge, Symbol, Option<Symbol>)> {
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

            // 피호출자 참조 커서 탐색. 멤버 호출은 실제 멤버 토큰 위치에서 재시도한다.
            let canonical_ref = self
                .find_callee_reference(tu, cx_file, cursor, call_site)
                .unwrap_or_else(|| clang_getNullCursor());

            let is_virtual = canonical_ref.kind == CXCursor_CXXMethod
                && clang_CXXMethod_isVirtual(canonical_ref) != 0;

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
                candidate_targets: if let Some(s) = &callee_sym {
                    vec![s.clone()]
                } else {
                    Vec::new()
                },
                clang_diagnostics: Vec::new(),
                reason,
                spelling_location: call_site
                    .callee_location
                    .clone()
                    .or_else(|| Some(call_site.expression.start.clone())),
                expansion_location: Some(call_site.expression.start.clone()),
                is_virtual,
                is_template_related: false,
                is_macro_expanded: false,
            };
            evidence_map.insert(context.key.clone(), evidence);

            let edge_id = CallEdgeId(self.next_edge_id);
            self.next_edge_id += 1;

            let edge = CallEdge {
                id: edge_id,
                caller: caller_sym.id.clone(),
                callee: callee_id,
                callsite: call_site.expression.clone(),
                kind,
                confidence,
                contexts: contexts_set,
                evidence_by_context: evidence_map,
            };

            Some((edge, caller_sym, callee_sym))
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

    /// Clang semantic parent 종류를 이용해 네임스페이스와 클래스 소유자를 분리한다.
    unsafe fn extract_scope_parts(&self, cursor: CXCursor) -> (Option<String>, Option<String>) {
        let mut namespaces = Vec::new();
        let mut classes = Vec::new();
        let mut parent = clang_getCursorSemanticParent(cursor);

        while clang_Cursor_isNull(parent) == 0 && parent.kind != CXCursor_TranslationUnit {
            let spelling = get_cx_string(clang_getCursorSpelling(parent));
            if !spelling.is_empty() {
                match parent.kind {
                    CXCursor_Namespace => namespaces.push(spelling),
                    CXCursor_ClassDecl
                    | CXCursor_StructDecl
                    | CXCursor_UnionDecl
                    | CXCursor_ClassTemplate
                    | CXCursor_ClassTemplatePartialSpecialization => classes.push(spelling),
                    _ => {}
                }
            }
            parent = clang_getCursorSemanticParent(parent);
        }

        namespaces.reverse();
        classes.reverse();
        let namespace = (!namespaces.is_empty()).then(|| namespaces.join("::"));
        let class_name = (!classes.is_empty()).then(|| classes.join("::"));
        (namespace, class_name)
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

/// Tree-sitter 호출식 원문에서 피호출자 단말 이름의 1-based 위치를 계산한다.
fn callee_spelling_point(
    call_site: &crate::model::CandidateCallSite,
) -> Option<LineColumn> {
    let start = call_site.expression.start.point?;
    let expression = call_site.expression_text.as_deref()?;
    let callable_part = expression
        .split_once('(')
        .map(|(before_args, _)| before_args)
        .unwrap_or(expression);
    let byte_offset = callable_part.rfind(&call_site.callee_spelling)?;
    let prefix = &expression[..byte_offset];
    let line_delta = prefix.bytes().filter(|byte| *byte == b'\n').count() as u32;

    if line_delta == 0 {
        Some(LineColumn {
            line: start.line,
            column: start.column.saturating_add(byte_offset as u32),
        })
    } else {
        let column = prefix
            .rsplit_once('\n')
            .map(|(_, tail)| tail.len() as u32 + 1)
            .unwrap_or(1);
        Some(LineColumn {
            line: start.line.saturating_add(line_delta),
            column,
        })
    }
}

fn is_callable_cursor_kind(kind: CXCursorKind) -> bool {
    matches!(
        kind,
        CXCursor_FunctionDecl
            | CXCursor_CXXMethod
            | CXCursor_Constructor
            | CXCursor_Destructor
            | CXCursor_ConversionFunction
            | CXCursor_FunctionTemplate
    )
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
