//! 컴파일 데이터베이스 어댑터 모듈
//! Compilation database adapter module

use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use crate::diagnostic::{AnalysisCause, AnalysisIssue, Diagnostic, InputError, Severity};
use crate::model::{CompilationContext, CompilationKey};

/// compile_commands.json의 단일 JSON 엔트리 구조체
#[derive(Debug, Clone, Deserialize)]
pub struct RawCompileEntry {
    /// 작업 디렉토리
    pub directory: Option<String>,
    /// 소스 파일 경로
    pub file: Option<String>,
    /// 컴파일 명령 문자열 (arguments가 없을 때 사용)
    pub command: Option<String>,
    /// 컴파일 인자 목록
    pub arguments: Option<Vec<String>>,
    /// 빌드 출력 파일 (선택적)
    pub output: Option<String>,
}

/// 컴파일 데이터베이스 저장소 (Compilation Database)
#[derive(Debug, Clone, Default)]
pub struct CompilationDb {
    /// 정규화된 소스 파일별 컴파일 컨텍스트 목록 (다중 컨텍스트 보존)
    pub source_file_to_contexts: BTreeMap<PathBuf, Vec<CompilationContext>>,
    /// 데이터베이스 로드 중 수집된 진단 목록 (사용 불가 엔트리 등)
    pub diagnostics: Vec<Diagnostic>,
}

impl CompilationDb {
    /// compile_commands.json 파일 로드 및 유효성 검증
    pub fn load(path: &Path) -> Result<Self, InputError> {
        // 1. 파일 존재 여부 확인
        if !path.exists() {
            return Err(InputError::InvalidCompilationDatabase {
                path: path.to_path_buf(),
                reason: "지정된 compile_commands.json 파일이 존재하지 않습니다.".to_string(),
            });
        }

        // 2. 파일 내용 읽기
        let content =
            fs::read_to_string(path).map_err(|e| InputError::InvalidCompilationDatabase {
                path: path.to_path_buf(),
                reason: format!("파일을 읽을 수 없습니다: {e}"),
            })?;

        // 3. JSON 파싱
        let entries: Vec<RawCompileEntry> =
            serde_json::from_str(&content).map_err(|e| InputError::InvalidCompilationDatabase {
                path: path.to_path_buf(),
                reason: format!("JSON 형식이 올바르지 않습니다: {e}"),
            })?;

        let mut db = Self::default();

        if entries.is_empty() {
            // 빈 데이터베이스에 대한 경고 진단
            db.diagnostics.push(Diagnostic::analysis(AnalysisIssue {
                severity: Severity::Recoverable,
                context: None,
                location: None,
                message: "compile_commands.json이 비어 있습니다.".to_string(),
                cause: AnalysisCause::Other("EmptyCompilationDatabase".to_string()),
            }));
            return Ok(db);
        }

        let mut seen_keys = BTreeSet::new();

        for (idx, entry) in entries.into_iter().enumerate() {
            match Self::process_entry(entry, idx) {
                Ok(Some(ctx)) => {
                    if seen_keys.insert(ctx.key.clone()) {
                        db.source_file_to_contexts
                            .entry(ctx.source_file.clone())
                            .or_default()
                            .push(ctx);
                    }
                }
                Ok(None) => {
                    // 유효하지 않지만 복구 가능한 항목 (진단 기록)
                }
                Err(diag) => {
                    db.diagnostics.push(diag);
                }
            }
        }

        Ok(db)
    }

    /// 개별 엔트리를 검증하고 CompilationContext로 변환
    fn process_entry(
        entry: RawCompileEntry,
        index: usize,
    ) -> Result<Option<CompilationContext>, Diagnostic> {
        let dir_str = match entry.directory {
            Some(d) if !d.trim().is_empty() => d,
            _ => {
                return Err(Diagnostic::analysis(AnalysisIssue {
                    severity: Severity::Recoverable,
                    context: None,
                    location: None,
                    message: format!(
                        "엔트리 #{index}: 'directory' 필드가 누락되었거나 비어 있습니다."
                    ),
                    cause: AnalysisCause::Other("MissingDirectoryField".to_string()),
                }));
            }
        };

        let file_str = match entry.file {
            Some(f) if !f.trim().is_empty() => f,
            _ => {
                return Err(Diagnostic::analysis(AnalysisIssue {
                    severity: Severity::Recoverable,
                    context: None,
                    location: None,
                    message: format!("엔트리 #{index}: 'file' 필드가 누락되었거나 비어 있습니다."),
                    cause: AnalysisCause::Other("MissingFileField".to_string()),
                }));
            }
        };

        // 작업 디렉토리 경로
        let directory_path = PathBuf::from(&dir_str);
        let normalized_dir = if directory_path.is_absolute() {
            fs::canonicalize(&directory_path).unwrap_or(directory_path)
        } else {
            directory_path
        };

        // 소스 파일 경로 (디렉토리 기준 상대 경로 처리)
        let raw_file_path = PathBuf::from(&file_str);
        let full_file_path = if raw_file_path.is_absolute() {
            raw_file_path
        } else {
            normalized_dir.join(raw_file_path)
        };

        let normalized_file = fs::canonicalize(&full_file_path).unwrap_or(full_file_path);

        // 인자 목록 추출 (command 또는 arguments)
        let raw_args = if let Some(args) = entry.arguments {
            args
        } else if let Some(cmd) = entry.command {
            // 쉘을 실행하지 않고 단순 렉싱 수행 (SDS DD-009)
            shlex_split(&cmd)
        } else {
            return Err(Diagnostic::analysis(AnalysisIssue {
                severity: Severity::Recoverable,
                context: None,
                location: None,
                message: format!("엔트리 #{index}: 'arguments' 또는 'command' 필드가 없습니다."),
                cause: AnalysisCause::Other("MissingArgumentsOrCommand".to_string()),
            }));
        };

        if raw_args.is_empty() {
            return Err(Diagnostic::analysis(AnalysisIssue {
                severity: Severity::Recoverable,
                context: None,
                location: None,
                message: format!("엔트리 #{index}: 컴파일러 인자가 비어 있습니다."),
                cause: AnalysisCause::Other("EmptyArguments".to_string()),
            }));
        }

        // 인자 정규화 (컴파일러 실행파일, 소스 파일명, 빌드 출력 옵션 제거)
        let semantic_args = normalize_compiler_args(&raw_args, &file_str);
        let clang_os_args: Vec<OsString> = semantic_args.iter().map(OsString::from).collect();

        // 고유 CompilationKey 생성 (안정적인 다이제스트)
        let key_content = format!(
            "{}|{}|{}",
            normalized_dir.display(),
            normalized_file.display(),
            semantic_args.join(" ")
        );
        let key = CompilationKey(format!("{:x}", calculate_hash(&key_content)));

        Ok(Some(CompilationContext {
            key,
            directory: normalized_dir,
            source_file: normalized_file,
            clang_args: clang_os_args,
        }))
    }

    /// 특정 소스 파일에 해당하는 모든 컴파일 컨텍스트 조회 (다중 컨텍스트 보존)
    pub fn contexts_for_source(&self, source_file: &Path) -> &[CompilationContext] {
        let canonical = fs::canonicalize(source_file).unwrap_or_else(|_| source_file.to_path_buf());
        if let Some(ctxs) = self.source_file_to_contexts.get(&canonical) {
            return ctxs.as_slice();
        }
        if let Some(ctxs) = self.source_file_to_contexts.get(source_file) {
            return ctxs.as_slice();
        }
        &[]
    }

    /// 고유 키에 해당하는 컴파일 컨텍스트 조회
    pub fn context_by_key(&self, key: &CompilationKey) -> Option<&CompilationContext> {
        self.source_file_to_contexts
            .values()
            .flatten()
            .find(|context| &context.key == key)
    }

    /// 데이터베이스에 등록된 모든 고유 소스 파일 목록 반환
    pub fn all_source_files(&self) -> Vec<PathBuf> {
        self.source_file_to_contexts.keys().cloned().collect()
    }
}

/// 쉘 실행 없이 명령어 문자열을 인자 벡터로 분리
fn shlex_split(cmd: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_single_quotes = false;
    let mut in_double_quotes = false;
    let mut escaped = false;

    for ch in cmd.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' if !in_single_quotes => {
                escaped = true;
            }
            '\'' if !in_double_quotes => {
                in_single_quotes = !in_single_quotes;
            }
            '"' if !in_single_quotes => {
                in_double_quotes = !in_double_quotes;
            }
            ' ' | '\t' | '\r' | '\n' if !in_single_quotes && !in_double_quotes => {
                if !current.is_empty() {
                    args.push(current);
                    current = String::new();
                }
            }
            _ => {
                current.push(ch);
            }
        }
    }

    if !current.is_empty() {
        args.push(current);
    }

    args
}

/// Clang 시맨틱 분석에 필요한 인자만 정규화하여 추출
/// 컴파일러 바이너리(argv[0]), 대상 소스 파일, 출력 옵션(-o, -c 등)을 제거하고
/// -I, -D, -std=, -target, -isystem 등 시맨틱 옵션은 유지
fn normalize_compiler_args(raw_args: &[String], target_file_spelling: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut skip_next = false;

    // 첫 번째 인자(컴파일러 이름)는 건너뜀
    let args_slice = if raw_args.len() > 1 {
        &raw_args[1..]
    } else {
        raw_args
    };

    let target_file_name = Path::new(target_file_spelling)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(target_file_spelling);

    for arg in args_slice {
        if skip_next {
            skip_next = false;
            continue;
        }

        // 출력 관련 플래그 건너뛰기
        if arg == "-o" || arg == "/Fo" || arg == "-MF" || arg == "-dependency-file" {
            skip_next = true;
            continue;
        }
        if arg.starts_with("-o") && arg.len() > 2 {
            continue;
        }
        if arg.starts_with("/Fo") && arg.len() > 3 {
            continue;
        }

        // 단순 컴파일 단계 플래그 건너뛰기
        if arg == "-c"
            || arg == "/c"
            || arg == "-emit-obj"
            || arg == "-MD"
            || arg == "-MMD"
            || arg == "-MP"
        {
            continue;
        }

        // 대상 소스 파일 인수 자체는 Clang TU 생성 시 전달하므로 제외
        if arg == target_file_spelling || arg.ends_with(target_file_name) {
            continue;
        }

        result.push(arg.clone());
    }

    result
}

/// 문자열 해시 계산 (안정적인 u64 다이제스트)
fn calculate_hash(s: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}
