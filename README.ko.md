<div align="center">
  <img src="resources/CallJet_Banner.png" alt="CallJet Banner" width="100%">
</div>

# CallJet C++

> **Find the path. Skip the whole graph.**  
> 전체 코드베이스의 거대한 시맨틱 호출 그래프를 미리 빌드하지 않고, 필요한 경로만 온디맨드로 분석하는 고속 C/C++ 정적 호출 경로 분석기(Static Call Path Analyzer).

[English](README.md) | [한국어 (Korean)](README.ko.md)

---

## 📌 핵심 원리 (Core Principle)

기존 도구들은 전체 프로젝트를 빌드하거나 모든 소스 파일에 대해 Clang AST를 생성하느라 수 분~수십 분이 소요됩니다. CallJet은 **2단계 온디맨드 하이브리드 파이프라인**으로 이를 해결합니다:

1. **Tree-sitter 경량 구문 탐색 (Candidate Discovery)**: 밀리초(ms) 단위로 전체 프로젝트의 구문을 파싱하여 호출 후보 및 역방향 피호출자 인덱스(`calls_by_spelling`)를 생성합니다.
2. **Clang 온디맨드 시맨틱 검증 (Demand-Driven Verification)**: 쿼리 순회 프론티어에서 **실제로 필요한 번역 단위(Translation Unit)만** 선별적으로 Clang 시맨틱 검증을 수행합니다.

---

## 🛠 사전 요구사항 (Prerequisites)

* **Rust**: `1.80` 이상 (Cargo 포함)
* **LLVM / Clang**: `libclang` (버전 16 이상 권장)
  * **Windows**: `winget install LLVM.LLVM` 또는 [LLVM 공식 릴리스](https://github.com/llvm/llvm-project/releases) 설치
  * **Linux (Ubuntu/Debian)**: `sudo apt-get install libclang-dev clang`
  * **macOS**: `brew install llvm`
* **Compilation Database**: 프로젝트의 `compile_commands.json`
  * CMake 사용 시: `cmake -B build -DCMAKE_EXPORT_COMPILE_COMMANDS=ON`
  * 파일이 없으면 소스 파싱 전에 중단하고 CMake 생성 명령과
    `--compile-commands` 사용 예시를 출력합니다.

---

## 🚀 빌드 및 설치 (Build & Installation)

```bash
# 저장소 클론 후 cpp 디렉토리로 이동
cd cpp

# 릴리스 바이너리 빌드
cargo build --release

# 생성된 실행 파일 위치: target/release/calljet (또는 target/release/calljet.exe)
```

---

## 💻 실행 방법 및 CLI 사용 가이드 (Usage)

CallJet은 4가지 핵심 쿼리 명령어를 제공합니다.

### 1. `callers` — 역방향 호출자 탐색
특정 함수/심볼을 호출하는 모든 상위 함수 체인을 온디맨드로 역추적합니다.

```bash
calljet callers <TARGET_SYMBOL> [OPTIONS]

# 예시: LeafFunction을 호출하는 모든 경로 탐색
calljet callers LeafFunction --root . --compile-commands build/compile_commands.json

# 예시: 최대 2단계 상위 호출자까지만 탐색
calljet callers Math::Calculator::add --max-depth 2

# 예시: 확정된(CONFIRMED) 직접 호출만 필터링하여 순회
calljet callers process_data --verified-only
```

---

### 2. `callees` — 순방향 피호출자 탐색
특정 함수/심볼 내부에서 호출하는 하위 함수 체인을 온디맨드로 순방향 추적합니다.

```bash
calljet callees <SOURCE_SYMBOL> [OPTIONS]

# 예시: main 함수에서 출발하는 모든 하위 호출 관계 탐색
calljet callees main --root .

# 예시: 네임스페이스 및 클래스 메서드 지정
calljet callees "App::Controller::handle_request"
```

---

### 3. `path` — 최단 호출 경로 탐색
시작 심볼(`source`)에서 도착 심볼(`target`)까지의 구체적인 호출 경로(Call Path)를 도출합니다.

```bash
calljet path <SOURCE_SYMBOL> <TARGET_SYMBOL> [OPTIONS]

# 예시: handle_packet에서 send_response로 도달하는 호출 경로 탐색
calljet path handle_packet send_response --compile-commands build/compile_commands.json

# 예시: 최대 5단계 이내의 경로만 탐색
calljet path main calculate_checksum --max-depth 5
```

---

### 4. `explain` — 단일 호출 엣지 상세 검증 및 근거
호출자(`caller`)와 피호출자(`callee`) 사이의 호출 엣지가 존재하는 이유와 Clang 시맨틱 검증 근거를 상세 출력합니다.

```bash
calljet explain <CALLER_SYMBOL> <CALLLEE_SYMBOL> [OPTIONS]

# 예시: process_event 내부의 dispatch 호출 검증 사유 확인
calljet explain process_event dispatch
```

---

## ⚙️ 공통 옵션 (Global Options)

| 옵션 | 단축 | 기본값 | 설명 |
| --- | :---: | --- | --- |
| `--root <PATH>` | `-r` | `.` (현재 디렉토리) | 프로젝트 소스 루트 디렉토리 경로 |
| `--compile-commands <PATH>` | `-c` | `<root>/compile_commands.json` | `compile_commands.json` 파일 경로 |
| `--format <FORMAT>` | `-f` | `text` | 출력 형식 (`text`, `json`, `mermaid`, `dot`) |
| `--output <FILE>` | `-o` | 표준 출력(stdout) | 분석 결과를 지정한 파일로 직접 저장 |
| `--max-depth <N>` | `-d` | 무제한 (사이클 자동 감지) | 순회 탐색의 최대 깊이 제한 |
| `--verified-only` | - | `false` | `[CONFIRMED]` 신뢰도를 가진 엣지만 순회 |
| `--no-unresolved` | - | `false` | `[UNRESOLVED]` 미해결 엣지를 결과에서 제외 |
| `--no-foreign` | - | `false` | 외부 라이브러리 경계 호출을 결과에서 제외 |
| `--metrics` | - | `false` | 소요 시간 및 메모리 등 성능 메트릭 상세 출력 |
| `--verbose` | - | `false` | 시맨틱 검증된 파일 및 생략된 번역 단위(TU) 목록 상세 출력 |
| `--help` | `-h` | - | 도움말 출력 |

---

## 📊 결과 출력 및 신뢰도 모델 (Confidence Model)

CallJet은 정적 분석의 한계를 솔직하게 표시하는 3단계 신뢰도(Confidence) 시스템을 사용합니다:

* **`[CONFIRMED]`**: Clang을 통해 정적으로 정확한 대상 심볼이 확인된 직접 호출 (Direct call)
* **`[POSSIBLE]`**: 가상 함수(Virtual dispatch) 등 런타임에 여러 타겟으로 분기될 수 있는 호출
* **`[UNRESOLVED]`**: 함수 포인터 등 정적으로 대상을 확정할 수 없는 호출

### 출력 예시 (`calljet callers c_leaf`)
```text
=== 호출 관계 (Call Edges) ===
• [CONFIRMED] c_mid -> c_leaf (direct, at src/c_chain.c:3:16-24)
    표현식: `c_leaf()`
    사유: ExactReference, 컨텍스트: src/c_chain.c#0
• [CONFIRMED] c_root -> c_mid (direct, at src/c_chain.c:4:17-24)
    표현식: `c_mid()`
    사유: ExactReference, 컨텍스트: src/c_chain.c#0

--- 분석 결과 요약 ---
상태: 분석 완료 (Complete)
통계: 총 심볼 3개, 확정 엣지(CONFIRMED) 2개, 가능 엣지(POSSIBLE) 0개, 미해결 엣지(UNRESOLVED) 0개
```

---

## 🚦 프로세스 종료 코드 (Exit Codes)

| 종료 코드 | 상태 | 의미 |
| :---: | --- | --- |
| **`0`** | `Complete` / `NoResult` / `Truncated` | 정상 완료 (결과 없음 또는 깊이 제한 도달 포함) |
| **`1`** | `Partial` / `InputError` / `QueryError` | 부분 분석 실패(일부 TU 오류) 또는 입력 오류 / 심볼 미발견 |
| **`2`** | `FatalError` | 심각한 내부 오류 또는 libclang 로드 실패 |

---

## 🧪 테스트 및 벤치마크 실행

```bash
# 전체 테스트 실행 (단위, Clang FFI, 쿼리 엔진, 인수 테스트 스위트)
cargo test

# SRS Acceptance Criteria (AC-001 ~ AC-018) 전수 테스트 실행
cargo test --test acceptance_suite_tests

# PoC 아키텍처 가설 벤치마크 실측 실행
cargo test --test benchmark_measurements -- --nocapture
```

---

## 📚 설계 문서 (Documentation)

* [제품 컨셉 (Concept)](docs/concept.md) — 제품 철학 및 범위 기준
* [요구사항 명세서 (SRS)](docs/srs.md) — 81개 기능 요구사항 및 인수 기준(AC)
* [설계 명세서 (SDS)](docs/sds.md) — 아키텍처, 데이터 모델, 쿼리 알고리즘
* [벤치마크 실측 보고서](docs/benchmark_report.md) — PoC 벤치마크 및 TU 절감 실측치
