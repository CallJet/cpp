//! Tree-sitter 구문 분석 기반 후보 탐색 및 인메모리 인덱스 모듈
//! Tree-sitter discovery and in-memory index module

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tree_sitter::{Node, Parser};

use crate::model::{
    CandidateCallId, CandidateCallKey, CandidateCallKind, CandidateCallSite, CandidateSymbol,
    CandidateSymbolId, CandidateSymbolKey, CandidateSymbolKind, CompilationKey, Language,
    LineColumn, SourceLocation, SourceRange, Symbol, SymbolQuery,
};
use crate::project::ProjectContext;

/// 단말 이름 및 한정자 기반 검색 키 (Normalized Lookup Key)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NameKey {
    /// 단말 이름 (Terminal Name)
    pub terminal_name: String,
    /// 한정자 접두어 (Qualifier, 선택적)
    pub qualifier: Option<String>,
}

/// 인메모리 후보 탐색 인덱스 (Discovery Index)
#[derive(Debug, Clone, Default)]
pub struct DiscoveryIndex {
    /// Tree-sitter로 실제 파싱한 소스 파일 목록
    pub source_files: Vec<PathBuf>,
    /// 이름 기반 텍스트 prefilter에서 검사한 파일 수 (중복 검색 포함)
    pub source_files_inspected: usize,
    /// 텍스트 prefilter와 Tree-sitter 후보 인덱스 구축에 걸린 누적 시간
    pub discovery_time: Duration,
    /// 심볼 이름별 후보 심볼 ID 맵
    pub symbols_by_name: BTreeMap<NameKey, Vec<CandidateSymbolId>>,
    /// 후보 심볼 ID별 상세 데이터
    pub symbols: BTreeMap<CandidateSymbolId, CandidateSymbol>,
    /// 피호출자 표기 문자열별 후보 호출 ID 맵 (역방향 탐색용)
    pub calls_by_spelling: BTreeMap<NameKey, Vec<CandidateCallId>>,
    /// 호출자 심볼별 포함된 호출 목록 (순방향 탐색용)
    pub calls_by_caller: BTreeMap<CandidateSymbolId, Vec<CandidateCallId>>,
    /// 후보 호출 ID별 상세 데이터
    pub calls: BTreeMap<CandidateCallId, CandidateCallSite>,
    /// 파일별 포함하는 헤더 목록 (Source File -> Included Headers)
    pub file_includes: BTreeMap<PathBuf, BTreeSet<PathBuf>>,
    /// 헤더 포함 관계 (Header -> Parent Source Files)
    pub include_parents: BTreeMap<PathBuf, BTreeSet<PathBuf>>,
    /// 이름 검색에서 함께 발견된 직접 컴파일 가능한 컨텍스트
    search_contexts_by_name: BTreeMap<String, Vec<CompilationKey>>,
    /// 텍스트 검색에서 같은 후보군으로 묶인 헤더/소스별 컴파일 컨텍스트
    search_contexts_by_file: BTreeMap<PathBuf, Vec<CompilationKey>>,
    /// 아직 AST를 만들지 않은 프로젝트 내 C/C++ 파일 카탈로그
    candidate_files: Option<Vec<PathBuf>>,
    /// 이미 텍스트 prefilter를 끝낸 단말 이름
    searched_names: BTreeSet<String>,
}

impl DiscoveryIndex {
    /// 프로젝트 전체 인덱스를 명시적으로 구축한다.
    ///
    /// 단위 테스트와 전체 인덱스가 필요한 호출자를 위한 호환 API이며,
    /// 일반 쿼리 경로는 `discover_query`로 필요한 파일만 지연 파싱한다.
    pub fn build(project: &ProjectContext) -> Self {
        let started = Instant::now();
        eprintln!("[CallJet] discovery: scanning source tree...");
        let scan_started = Instant::now();
        let source_files = project.source_files();
        eprintln!(
            "[CallJet] discovery: found {} C/C++ file(s) in {:?}",
            source_files.len(),
            scan_started.elapsed()
        );

        let mut index = Self {
            candidate_files: Some(source_files.clone()),
            source_files_inspected: source_files.len(),
            ..Self::default()
        };
        index.index_files(project, &source_files);
        index.discovery_time = started.elapsed();
        eprintln!(
            "[CallJet] discovery: complete — {} symbol candidate(s), {} call candidate(s) in {:?}",
            index.symbols.len(),
            index.calls.len(),
            index.discovery_time
        );
        index
    }

    /// 심볼 쿼리에 필요한 파일만 텍스트로 좁힌 뒤 Tree-sitter로 지연 파싱한다.
    pub fn discover_query(&mut self, project: &ProjectContext, query: &SymbolQuery) {
        self.discover_spelling(project, &query.terminal_name);
    }

    /// 단말 이름이 등장하는 파일만 후보 인덱스에 추가한다.
    pub fn discover_spelling(&mut self, project: &ProjectContext, spelling: &str) {
        let spelling = spelling.trim();
        if spelling.is_empty() || !self.searched_names.insert(spelling.to_string()) {
            return;
        }

        let started = Instant::now();
        if self.candidate_files.is_none() {
            eprintln!("[CallJet] discovery: collecting C/C++ file paths...");
            let scan_started = Instant::now();
            let files = project.source_files();
            eprintln!(
                "[CallJet] discovery: {} candidate file path(s) in {:?}",
                files.len(),
                scan_started.elapsed()
            );
            self.candidate_files = Some(files);
        }

        let files = self.candidate_files.clone().unwrap_or_default();
        let total_files = files.len();
        let progress_step = (total_files / 10).max(1);
        let mut matches = Vec::new();

        eprintln!(
            "[CallJet] discovery: text prefilter '{}' across {} file(s)...",
            spelling, total_files
        );
        for (index, file) in files.iter().enumerate() {
            let processed = index + 1;
            if processed == total_files || processed % progress_step == 0 {
                let percent = processed.saturating_mul(100) / total_files.max(1);
                eprintln!(
                    "[CallJet] discovery: text prefilter {processed}/{total_files} ({percent}%)"
                );
            }

            if file_contains_spelling(file, spelling) {
                matches.push(file.clone());
            }
        }
        self.source_files_inspected = self.source_files_inspected.saturating_add(total_files);

        let mut context_keys = Vec::new();
        for file in &matches {
            for context in project.compilation_db.contexts_for_source(file) {
                if !context_keys.contains(&context.key) {
                    context_keys.push(context.key.clone());
                }
            }
        }
        self.search_contexts_by_name
            .insert(spelling.to_string(), context_keys.clone());
        for file in &matches {
            let file_contexts = self
                .search_contexts_by_file
                .entry(file.clone())
                .or_default();
            for key in &context_keys {
                if !file_contexts.contains(key) {
                    file_contexts.push(key.clone());
                }
            }
        }

        let matched_count = matches.len();
        let parsed = self.source_files.iter().cloned().collect::<BTreeSet<_>>();
        let new_matches = matches
            .into_iter()
            .filter(|file| !parsed.contains(file))
            .collect::<Vec<_>>();

        eprintln!(
            "[CallJet] discovery: '{}' matched {} file(s); Tree-sitter parsing {} new file(s)",
            spelling,
            matched_count,
            new_matches.len()
        );
        self.index_files(project, &new_matches);
        self.discovery_time += started.elapsed();
    }

    /// 아직 파싱하지 않은 파일만 기존 인덱스에 병합한다.
    fn index_files(&mut self, project: &ProjectContext, files: &[PathBuf]) {
        if files.is_empty() {
            return;
        }

        let parsed = self.source_files.iter().cloned().collect::<BTreeSet<_>>();
        let mut new_files = files
            .iter()
            .filter(|file| !parsed.contains(*file))
            .cloned()
            .collect::<Vec<_>>();
        new_files.sort();
        new_files.dedup();
        if new_files.is_empty() {
            return;
        }

        let total_files = new_files.len();
        let progress_step = (total_files / 10).max(1);
        let previous = std::mem::take(self);
        let mut indexer = IndexBuilder::with_index(project, previous);

        for (index, file) in new_files.iter().enumerate() {
            let processed = index + 1;
            if processed == 1 || processed == total_files || processed % progress_step == 0 {
                let percent = processed.saturating_mul(100) / total_files.max(1);
                eprintln!(
                    "[CallJet] discovery: Tree-sitter {processed}/{total_files} ({percent}%) — {}",
                    project.display_path(file).display()
                );
            }
            indexer.index_file(file);
        }

        let mut index = indexer.finish();
        index.source_files.extend(new_files);
        index.source_files.sort();
        index.source_files.dedup();
        *self = index;
    }

    /// 쿼리와 매칭되는 후보 심볼 ID 목록 조회
    pub fn matching_symbols(&self, query: &SymbolQuery) -> &[CandidateSymbolId] {
        let key = NameKey {
            terminal_name: query.terminal_name.clone(),
            qualifier: query.qualifier_hint.clone(),
        };
        if let Some(syms) = self.symbols_by_name.get(&key) {
            return syms.as_slice();
        }

        // 한정자가 일치하지 않더라도 단말 이름이 일치하는 후보 검색 (SDS DD-002: Broad narrowing)
        let relaxed_key = NameKey {
            terminal_name: query.terminal_name.clone(),
            qualifier: None,
        };
        self.symbols_by_name
            .get(&relaxed_key)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// 특정 심볼을 호출할 가능성이 있는 후보 호출 ID 목록 조회 (역방향 탐색용)
    pub fn candidate_callers(&self, target: &Symbol) -> Vec<CandidateCallId> {
        let mut result = Vec::new();

        // 1. 단말 이름 기반 호출 검색
        let key = NameKey {
            terminal_name: target.name.clone(),
            qualifier: None,
        };
        if let Some(calls) = self.calls_by_spelling.get(&key) {
            result.extend(calls.iter().copied());
        }

        // 2. 한정된 이름이 있는 경우 해당 이름으로도 추가 검색
        if let Some(qn) = &target.qualified_name {
            if let Some(pos) = qn.rfind("::") {
                let qual_key = NameKey {
                    terminal_name: target.name.clone(),
                    qualifier: Some(qn[..pos + 2].to_string()),
                };
                if let Some(calls) = self.calls_by_spelling.get(&qual_key) {
                    for &c in calls {
                        if !result.contains(&c) {
                            result.push(c);
                        }
                    }
                }
            }
        }

        result
    }

    /// 특정 후보 심볼 내부에서 발생하는 호출 ID 목록 조회 (순방향 탐색용)
    pub fn candidate_callees(&self, source: CandidateSymbolId) -> &[CandidateCallId] {
        self.calls_by_caller
            .get(&source)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// 특정 호출이 속한 컴파일 컨텍스트 키 목록 조회
    pub fn contexts_for(
        &self,
        call_id: CandidateCallId,
        project: &ProjectContext,
    ) -> Vec<CompilationKey> {
        if let Some(call) = self.calls.get(&call_id) {
            let contexts = self.contexts_for_file(&call.expression.start.file, project);
            if !contexts.is_empty() {
                return contexts;
            }

            if let Some(caller) = self.symbols.get(&call.caller) {
                return self
                    .search_contexts_by_name
                    .get(&caller.name)
                    .cloned()
                    .unwrap_or_default();
            }
        }

        Vec::new()
    }

    /// 특정 후보 심볼을 해석할 수 있는 컴파일 컨텍스트 키 목록 조회
    pub fn contexts_for_symbol(
        &self,
        symbol_id: CandidateSymbolId,
        project: &ProjectContext,
    ) -> Vec<CompilationKey> {
        let Some(symbol) = self.symbols.get(&symbol_id) else {
            return Vec::new();
        };

        let contexts = self.contexts_for_file(&symbol.declaration.start.file, project);
        if !contexts.is_empty() {
            return contexts;
        }

        self.search_contexts_by_name
            .get(&symbol.name)
            .cloned()
            .unwrap_or_default()
    }

    /// 소스 또는 헤더 파일을 사용할 수 있는 컴파일 컨텍스트 키 목록 조회
    fn contexts_for_file(&self, file: &Path, project: &ProjectContext) -> Vec<CompilationKey> {
        let mut contexts = Vec::new();

        for ctx in project.compilation_db.contexts_for_source(file) {
            if !contexts.contains(&ctx.key) {
                contexts.push(ctx.key.clone());
            }
        }

        if contexts.is_empty() {
            if let Some(parents) = self.include_parents.get(file) {
                for parent in parents {
                    for ctx in project.compilation_db.contexts_for_source(parent) {
                        if !contexts.contains(&ctx.key) {
                            contexts.push(ctx.key.clone());
                        }
                    }
                }
            }
        }

        if contexts.is_empty() {
            if let Some(search_contexts) = self.search_contexts_by_file.get(file) {
                contexts.extend(search_contexts.iter().cloned());
            }
        }

        contexts
    }
}

/// 인덱스 구축을 위한 빌더 구조체
struct IndexBuilder<'a> {
    project: &'a ProjectContext,
    index: DiscoveryIndex,
    symbol_keys: BTreeMap<CandidateSymbolKey, CandidateSymbolId>,
    call_keys: BTreeMap<CandidateCallKey, CandidateCallId>,
    next_symbol_id: u32,
    next_call_id: u32,
}

impl<'a> IndexBuilder<'a> {
    fn with_index(project: &'a ProjectContext, index: DiscoveryIndex) -> Self {
        let next_symbol_id = index
            .symbols
            .keys()
            .map(|id| id.0)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let next_call_id = index
            .calls
            .keys()
            .map(|id| id.0)
            .max()
            .unwrap_or(0)
            .saturating_add(1);

        Self {
            project,
            index,
            symbol_keys: BTreeMap::new(),
            call_keys: BTreeMap::new(),
            next_symbol_id,
            next_call_id,
        }
    }

    fn finish(self) -> DiscoveryIndex {
        self.index
    }

    /// 개별 파일 파싱 및 심볼/호출/인클루드 추출
    fn index_file(&mut self, file_path: &Path) {
        let content = match fs::read(file_path) {
            Ok(c) => c,
            Err(_) => return,
        };

        let lang = detect_language(file_path);
        let mut parser = Parser::new();

        let ts_lang = match lang {
            Language::C => tree_sitter_c::language(),
            Language::Cpp => tree_sitter_cpp::language(),
        };

        if parser.set_language(&ts_lang).is_err() {
            return;
        }

        let tree = match parser.parse(&content, None) {
            Some(t) => t,
            None => return,
        };

        let canonical_path =
            fs::canonicalize(file_path).unwrap_or_else(|_| file_path.to_path_buf());

        let mut extractor = AstExtractor {
            file_path: canonical_path,
            source: &content,
            language: lang,
            builder: self,
            scope_stack: Vec::new(),
            class_stack: Vec::new(),
            current_symbol: None,
        };

        extractor.walk_node(tree.root_node());
    }
}

/// AST 순회 및 데이터 추출기
struct AstExtractor<'s, 'b, 'p> {
    file_path: PathBuf,
    source: &'s [u8],
    language: Language,
    builder: &'b mut IndexBuilder<'p>,
    scope_stack: Vec<String>,
    class_stack: Vec<String>,
    current_symbol: Option<CandidateSymbolId>,
}

impl<'s, 'b, 'p> AstExtractor<'s, 'b, 'p> {
    fn walk_node(&mut self, node: Node) {
        let kind = node.kind();

        // 1. #include 전처리기 디렉티브 감지
        if kind == "preproc_include" {
            self.extract_include(node);
            return;
        }

        // 2. 네임스페이스 스코프 진입
        if kind == "namespace_definition" {
            let ns_name = node
                .child_by_field_name("name")
                .map(|n| self.node_text(n))
                .unwrap_or_default();

            if !ns_name.is_empty() {
                self.scope_stack.push(ns_name.clone());
            }

            if let Some(body) = node.child_by_field_name("body") {
                let mut cursor = body.walk();
                for child in body.children(&mut cursor) {
                    self.walk_node(child);
                }
            }

            if !ns_name.is_empty() {
                self.scope_stack.pop();
            }
            return;
        }

        // 3. 클래스 또는 구조체 스코프 진입
        if kind == "class_specifier" || kind == "struct_specifier" {
            let class_name = node
                .child_by_field_name("name")
                .map(|n| self.node_text(n))
                .unwrap_or_default();

            if !class_name.is_empty() {
                self.scope_stack.push(class_name.clone());
                self.class_stack.push(class_name.clone());
            }

            if let Some(body) = node.child_by_field_name("body") {
                let mut cursor = body.walk();
                for child in body.children(&mut cursor) {
                    self.walk_node(child);
                }
            }

            if !class_name.is_empty() {
                self.class_stack.pop();
                self.scope_stack.pop();
            }
            return;
        }

        // 4. 함수 또는 메서드 정의/선언
        if kind == "function_definition" {
            self.extract_function_definition(node);
            return;
        } else if kind == "declaration"
            || kind == "field_declaration"
            || kind == "template_declaration"
        {
            if let Some(func_node) = find_function_declarator(node) {
                self.extract_function_declaration(node, func_node);
                return;
            }
        }

        // 5. 호출식 발견 (함수 내부인 경우)
        if kind == "call_expression" {
            if let Some(enclosing) = self.current_symbol {
                self.extract_call_expression(node, enclosing);
            }
        }

        // 자식 노드 순회
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_node(child);
        }
    }

    /// include 지시문 추출
    fn extract_include(&mut self, node: Node) {
        if let Some(path_node) = node.child_by_field_name("path") {
            let raw_include = self.node_text(path_node);
            let cleaned = raw_include.trim_matches(|c| c == '<' || c == '>' || c == '"');
            let header_path = PathBuf::from(cleaned);

            self.builder
                .index
                .file_includes
                .entry(self.file_path.clone())
                .or_default()
                .insert(header_path.clone());

            // 헤더가 소스 루트 내에 물리적으로 존재하는 경우 canonicalize하여 include_parents에 등록
            let local_candidate = self
                .file_path
                .parent()
                .unwrap_or(Path::new(""))
                .join(cleaned);
            if let Ok(canonical_hdr) = fs::canonicalize(&local_candidate) {
                self.builder
                    .index
                    .include_parents
                    .entry(canonical_hdr)
                    .or_default()
                    .insert(self.file_path.clone());
            } else if let Ok(canonical_hdr) =
                fs::canonicalize(self.builder.project.source_root.join(cleaned))
            {
                self.builder
                    .index
                    .include_parents
                    .entry(canonical_hdr)
                    .or_default()
                    .insert(self.file_path.clone());
            }
        }
    }

    /// 함수 정의(Definition) 추출
    fn extract_function_definition(&mut self, node: Node) {
        let decl_node = match node.child_by_field_name("declarator") {
            Some(d) => d,
            None => return,
        };

        let (name, qual_hint) = self.extract_declarator_name(decl_node);
        if name.is_empty() {
            return;
        }

        let decl_range = self.node_source_range(node);
        let body_range = node
            .child_by_field_name("body")
            .map(|b| self.node_source_range(b));

        let current_scope_qualifier = if !self.scope_stack.is_empty() {
            Some(format!("{}::", self.scope_stack.join("::")))
        } else {
            None
        };

        let final_qualifier = match (current_scope_qualifier, qual_hint) {
            (Some(scope), Some(q)) => Some(format!("{scope}{q}")),
            (Some(scope), None) => Some(scope),
            (None, Some(q)) => Some(q),
            (None, None) => None,
        };

        let syntactic_kind = if !self.class_stack.is_empty() {
            CandidateSymbolKind::Method
        } else {
            CandidateSymbolKind::Function
        };

        let has_syntax_error = node.has_error();

        let sym_key = CandidateSymbolKey {
            file: self.file_path.clone(),
            declaration_range: decl_range.clone(),
            syntactic_kind,
        };

        let sym_id = if let Some(&existing_id) = self.builder.symbol_keys.get(&sym_key) {
            existing_id
        } else {
            let new_id = CandidateSymbolId(self.builder.next_symbol_id);
            self.builder.next_symbol_id += 1;
            self.builder.symbol_keys.insert(sym_key, new_id);

            let candidate = CandidateSymbol {
                id: new_id,
                language: self.language,
                syntactic_kind,
                name: name.clone(),
                qualifier_hint: final_qualifier.clone(),
                signature_hint: None,
                owner_hint: self.class_stack.last().cloned(),
                declaration: decl_range,
                definition_body: body_range,
                syntax_complete: !has_syntax_error,
            };

            // 인덱스에 등록
            let name_key = NameKey {
                terminal_name: name.clone(),
                qualifier: final_qualifier,
            };
            self.builder
                .index
                .symbols_by_name
                .entry(name_key.clone())
                .or_default()
                .push(new_id);

            // 한정자 없는 기본 이름 키도 등록 (빠른 검색용)
            if name_key.qualifier.is_some() {
                let bare_key = NameKey {
                    terminal_name: name,
                    qualifier: None,
                };
                self.builder
                    .index
                    .symbols_by_name
                    .entry(bare_key)
                    .or_default()
                    .push(new_id);
            }

            self.builder.index.symbols.insert(new_id, candidate);
            new_id
        };

        // 함수 본문 내부의 호출식 수집을 위해 current_symbol 설정 후 본문 탐색
        let prev_symbol = self.current_symbol;
        self.current_symbol = Some(sym_id);

        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                self.walk_node(child);
            }
        }

        self.current_symbol = prev_symbol;
    }

    /// 함수 선언(Declaration) 추출
    fn extract_function_declaration(&mut self, node: Node, func_declarator: Node) {
        let (name, qual_hint) = self.extract_declarator_name(func_declarator);
        if name.is_empty() {
            return;
        }

        let decl_range = self.node_source_range(node);
        let current_scope_qualifier = if !self.scope_stack.is_empty() {
            Some(format!("{}::", self.scope_stack.join("::")))
        } else {
            None
        };

        let final_qualifier = match (current_scope_qualifier, qual_hint) {
            (Some(scope), Some(q)) => Some(format!("{scope}{q}")),
            (Some(scope), None) => Some(scope),
            (None, Some(q)) => Some(q),
            (None, None) => None,
        };

        let syntactic_kind = if !self.class_stack.is_empty() {
            CandidateSymbolKind::Method
        } else {
            CandidateSymbolKind::Function
        };

        let has_syntax_error = node.has_error();

        let sym_key = CandidateSymbolKey {
            file: self.file_path.clone(),
            declaration_range: decl_range.clone(),
            syntactic_kind,
        };

        if !self.builder.symbol_keys.contains_key(&sym_key) {
            let new_id = CandidateSymbolId(self.builder.next_symbol_id);
            self.builder.next_symbol_id += 1;
            self.builder.symbol_keys.insert(sym_key, new_id);

            let candidate = CandidateSymbol {
                id: new_id,
                language: self.language,
                syntactic_kind,
                name: name.clone(),
                qualifier_hint: final_qualifier.clone(),
                signature_hint: None,
                owner_hint: self.class_stack.last().cloned(),
                declaration: decl_range,
                definition_body: None,
                syntax_complete: !has_syntax_error,
            };

            let name_key = NameKey {
                terminal_name: name.clone(),
                qualifier: final_qualifier,
            };
            self.builder
                .index
                .symbols_by_name
                .entry(name_key.clone())
                .or_default()
                .push(new_id);

            if name_key.qualifier.is_some() {
                let bare_key = NameKey {
                    terminal_name: name,
                    qualifier: None,
                };
                self.builder
                    .index
                    .symbols_by_name
                    .entry(bare_key)
                    .or_default()
                    .push(new_id);
            }

            self.builder.index.symbols.insert(new_id, candidate);
        }
    }

    /// 호출 표현식(Call Expression) 추출
    fn extract_call_expression(&mut self, node: Node, caller_id: CandidateSymbolId) {
        let func_node = match node.child_by_field_name("function") {
            Some(f) => f,
            None => return,
        };

        let (callee_spelling, qualifier_hint, syntax_hint, callee_node) =
            self.parse_callee_expression(func_node);
        if callee_spelling.is_empty() {
            return;
        }

        let expr_range = self.node_source_range(node);
        let expr_text = self.node_text(node);
        let has_syntax_error = node.has_error();

        let call_key = CandidateCallKey {
            file: self.file_path.clone(),
            expression_range: expr_range.clone(),
            enclosing_symbol: caller_id,
            callee_spelling: callee_spelling.clone(),
        };

        let _call_id = if let Some(&existing_id) = self.builder.call_keys.get(&call_key) {
            existing_id
        } else {
            let new_id = CandidateCallId(self.builder.next_call_id);
            self.builder.next_call_id += 1;
            self.builder.call_keys.insert(call_key, new_id);

            let candidate_call = CandidateCallSite {
                id: new_id,
                caller: caller_id,
                callee_spelling: callee_spelling.clone(),
                callee_location: callee_node
                    .map(|node| self.node_source_range(node).start),
                qualifier_hint: qualifier_hint.clone(),
                expression: expr_range,
                expression_text: Some(expr_text),
                syntax_hint,
                syntax_complete: !has_syntax_error,
            };

            // 역방향 인덱스 (피호출자 표기 문자열 기준)
            let spelling_key = NameKey {
                terminal_name: callee_spelling,
                qualifier: qualifier_hint,
            };
            self.builder
                .index
                .calls_by_spelling
                .entry(spelling_key)
                .or_default()
                .push(new_id);

            // 순방향 인덱스 (호출자 심볼 기준)
            self.builder
                .index
                .calls_by_caller
                .entry(caller_id)
                .or_default()
                .push(new_id);

            self.builder.index.calls.insert(new_id, candidate_call);
            new_id
        };

        // 호출 인자 내부의 중첩 호출식(예: foo(bar()))을 위해 인자 노드 순회
        if let Some(args_node) = node.child_by_field_name("arguments") {
            let mut cursor = args_node.walk();
            for child in args_node.children(&mut cursor) {
                self.walk_node(child);
            }
        }
    }

    /// callee 표현식 파싱
    fn parse_callee_expression<'tree>(
        &self,
        func_node: Node<'tree>,
    ) -> (
        String,
        Option<String>,
        CandidateCallKind,
        Option<Node<'tree>>,
    ) {
        let kind = func_node.kind();

        match kind {
            "identifier" => (
                self.node_text(func_node),
                None,
                CandidateCallKind::Direct,
                Some(func_node),
            ),
            "scoped_identifier" | "qualified_identifier" => {
                let full = self.node_text(func_node);
                let name_node = func_node.child_by_field_name("name").unwrap_or(func_node);
                if let Some(pos) = full.rfind("::") {
                    let qualifier = &full[..pos + 2];
                    let name = &full[pos + 2..];
                    (
                        name.to_string(),
                        Some(qualifier.to_string()),
                        CandidateCallKind::Qualified,
                        Some(name_node),
                    )
                } else {
                    (full, None, CandidateCallKind::Qualified, Some(name_node))
                }
            }
            "field_expression" => {
                let field_node = func_node.child_by_field_name("field");
                let field_name = field_node.map(|n| self.node_text(n)).unwrap_or_default();
                (field_name, None, CandidateCallKind::Member, field_node)
            }
            "template_function" => {
                if let Some(name_node) = func_node.child_by_field_name("name") {
                    self.parse_callee_expression(name_node)
                } else {
                    (
                        self.node_text(func_node),
                        None,
                        CandidateCallKind::Other,
                        Some(func_node),
                    )
                }
            }
            _ => (
                self.node_text(func_node),
                None,
                CandidateCallKind::Other,
                Some(func_node),
            ),
        }
    }

    /// 선언자(Declarator) 노드에서 이름과 한정자 추출
    fn extract_declarator_name(&self, declarator: Node) -> (String, Option<String>) {
        let mut cur = declarator;
        loop {
            match cur.kind() {
                "function_declarator" | "pointer_declarator" | "reference_declarator" => {
                    if let Some(inner) = cur.child_by_field_name("declarator") {
                        cur = inner;
                    } else {
                        break;
                    }
                }
                "identifier" => return (self.node_text(cur), None),
                "scoped_identifier" | "qualified_identifier" => {
                    let full = self.node_text(cur);
                    if let Some(pos) = full.rfind("::") {
                        let qualifier = &full[..pos + 2];
                        let name = &full[pos + 2..];
                        return (name.to_string(), Some(qualifier.to_string()));
                    } else {
                        return (full, None);
                    }
                }
                "destructor_name" => {
                    return (self.node_text(cur), None);
                }
                _ => {
                    // child 중 identifier가 있는지 탐색
                    let mut found = None;
                    let mut cursor = cur.walk();
                    for child in cur.children(&mut cursor) {
                        if child.kind() == "identifier" {
                            found = Some(self.node_text(child));
                            break;
                        }
                    }
                    if let Some(f) = found {
                        return (f, None);
                    }
                    break;
                }
            }
        }

        (self.node_text(cur), None)
    }

    /// Tree-sitter 노드의 원문 텍스트 추출
    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    /// Tree-sitter 노드의 SourceRange 변환 (1-based 변환)
    fn node_source_range(&self, node: Node) -> SourceRange {
        let start_point = node.start_position();
        let end_point = node.end_position();

        let start_loc = SourceLocation {
            file: self.file_path.clone(),
            point: Some(LineColumn {
                line: (start_point.row + 1) as u32,
                column: (start_point.column + 1) as u32,
            }),
        };

        let end_loc = SourceLocation {
            file: self.file_path.clone(),
            point: Some(LineColumn {
                line: (end_point.row + 1) as u32,
                column: (end_point.column + 1) as u32,
            }),
        };

        SourceRange::spanned(start_loc, end_loc)
    }
}

/// Node 트리 내에서 function_declarator 노드를 재귀적으로 검색
fn find_function_declarator(node: Node) -> Option<Node> {
    if node.kind() == "function_declarator" {
        return Some(node);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = find_function_declarator(child) {
            return Some(found);
        }
    }

    None
}

/// 파일 경로를 기반으로 C 또는 C++ 언어 감지
fn detect_language(path: &Path) -> Language {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        match ext.to_ascii_lowercase().as_str() {
            "c" | "h" => Language::C,
            _ => Language::Cpp,
        }
    } else {
        Language::Cpp
    }
}

/// 파일 원문에 단말 이름이 식별자 경계로 등장하는지 빠르게 확인한다.
/// Tree-sitter보다 먼저 실행되는 저비용 prefilter이므로 주석/문자열의 오탐은 허용한다.
fn file_contains_spelling(path: &Path, spelling: &str) -> bool {
    let Ok(content) = fs::read(path) else {
        return false;
    };
    let needle = spelling.as_bytes();
    if needle.is_empty() || content.len() < needle.len() {
        return false;
    }

    let identifier = needle.iter().all(|byte| is_identifier_byte(*byte));
    content.windows(needle.len()).enumerate().any(|(offset, window)| {
        if window != needle {
            return false;
        }
        if !identifier {
            return true;
        }

        let left_ok = offset == 0 || !is_identifier_byte(content[offset - 1]);
        let end = offset + needle.len();
        let right_ok = end == content.len() || !is_identifier_byte(content[end]);
        left_ok && right_ok
    })
}

fn is_identifier_byte(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}
