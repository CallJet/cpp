//! 소스 루트 검증 및 프로젝트 컨텍스트 모듈
//! Source-root validation and project context module

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::ProjectInput;
use crate::compile_db::CompilationDb;
use crate::diagnostic::InputError;

/// 프로젝트 분석 컨텍스트 (Project Context)
#[derive(Debug, Clone)]
pub struct ProjectContext {
    /// 정규화된 절대 경로 소스 루트 (Canonical Source Root)
    pub source_root: PathBuf,
    /// 사용자 표시용 상대/표시 루트 (Display Root)
    pub display_root: PathBuf,
    /// 로드된 컴파일 데이터베이스 (Compilation Database)
    pub compilation_db: CompilationDb,
}

impl ProjectContext {
    /// 프로젝트 입력으로부터 유효성을 검증하고 ProjectContext 로드
    pub fn load(input: ProjectInput) -> Result<Self, InputError> {
        let root = &input.source_root;

        // 1. 소스 루트 존재 여부 확인 (FR-003)
        if !root.exists() {
            return Err(InputError::InvalidSourceRoot {
                path: root.clone(),
                reason: "지정된 소스 루트 디렉토리가 존재하지 않습니다.".to_string(),
            });
        }

        // 2. 디렉토리 여부 확인 (FR-003)
        if !root.is_dir() {
            return Err(InputError::InvalidSourceRoot {
                path: root.clone(),
                reason: "지정된 소스 루트가 디렉토리가 아닙니다.".to_string(),
            });
        }

        // 3. 정규화된 절대 경로 획득
        let canonical_root = fs::canonicalize(root).map_err(|e| InputError::InvalidSourceRoot {
            path: root.clone(),
            reason: format!("소스 루트 경로 정규화 실패: {e}"),
        })?;

        // 4. 컴파일 데이터베이스 로드 및 검증 (FR-006, FR-007)
        let compilation_db = CompilationDb::load(&input.compile_commands_path)?;

        Ok(Self {
            source_root: canonical_root,
            display_root: root.clone(),
            compilation_db,
        })
    }

    /// 파일 경로를 사용자 표시용 상대 경로로 변환
    pub fn display_path(&self, path: &Path) -> PathBuf {
        if let Ok(rel) = path.strip_prefix(&self.source_root) {
            rel.to_path_buf()
        } else {
            path.to_path_buf()
        }
    }

    /// 프로젝트 내 C/C++ 소스 파일 목록을 반환
    /// 컴파일 데이터베이스에 등록된 파일과 소스 루트 디렉토리 내의 소스 파일들을 결합
    pub fn source_files(&self) -> Vec<PathBuf> {
        let mut files = BTreeSet::new();

        // 1. 컴파일 데이터베이스에 등록된 소스 파일 추가
        for file in self.compilation_db.all_source_files() {
            if file.starts_with(&self.source_root) && file.is_file() {
                files.insert(file);
            }
        }

        // 2. 소스 루트 디렉토리 순회하여 C/C++ 확장자 파일 검색
        // 디렉토리 심볼릭 링크를 따라가면 루트 재진입/외부 디렉토리 이탈로
        // 순회량이 무한히 증가할 수 있으므로 실제 디렉토리만 방문한다.
        let mut visited_dirs = BTreeSet::new();
        let mut stack = vec![self.source_root.clone()];
        while let Some(dir) = stack.pop() {
            let canonical_dir = match fs::canonicalize(&dir) {
                Ok(path) if path.starts_with(&self.source_root) => path,
                _ => continue,
            };

            if !visited_dirs.insert(canonical_dir.clone()) {
                continue;
            }

            let entries = match fs::read_dir(&canonical_dir) {
                Ok(entries) => entries,
                Err(_) => continue,
            };

            for entry in entries.flatten() {
                let file_type = match entry.file_type() {
                    Ok(file_type) => file_type,
                    Err(_) => continue,
                };
                let path = entry.path();

                if file_type.is_symlink() {
                    continue;
                }

                if file_type.is_dir() {
                    if !is_ignored_directory(&path) {
                        stack.push(path);
                    }
                } else if file_type.is_file() && is_c_cpp_file(&path) {
                    if let Ok(canonical) = fs::canonicalize(&path) {
                        if canonical.starts_with(&self.source_root) {
                            files.insert(canonical);
                        }
                    }
                }
            }
        }

        files.into_iter().collect()
    }
}

/// 후보 탐색 가치가 없고 파일 수가 매우 큰 도구/빌드 캐시 디렉토리인지 확인
fn is_ignored_directory(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };

    matches!(
        name,
        ".git" | ".hg" | ".svn" | ".cache" | "node_modules" | "target"
    )
}

/// 파일이 C/C++ 소스 또는 헤더 파일인지 확인
pub fn is_c_cpp_file(path: &Path) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        matches!(
            ext.to_ascii_lowercase().as_str(),
            "c" | "cpp" | "cxx" | "cc" | "c++" | "h" | "hpp" | "hxx" | "hh" | "h++" | "inl" | "tpp"
        )
    } else {
        false
    }
}
