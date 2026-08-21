<div align="center">
  <img src="resources/CallJet_Banner.png" alt="CallJet Banner" width="100%">
</div>

# CallJet C++

> **Find the path. Skip the whole graph.**  
> 전체 코드베이스의 거대한 시맨틱 호출 그래프를 미리 빌드하지 않고, 필요한 경로만 온디맨드로 분석하는 고속 C/C++ 정적 호출 경로 분석기(Static Call Path Analyzer).

[English](README.md) | [한국어 (Korean)](README.ko.md)

---

## 📌 핵심 원리 (Core Principle)

기존 도구들은 전체 프로젝트를 빌드하거나 모든 소스 파일에 대해 Clang AST를 생성하느라 수 분~수십 분이 소요됩니다. CallJet은 **3단계 온디맨드 파이프라인**으로 이를 해결합니다:

1. **텍스트 사전 필터 (Text Prefilter)**: 요청한 심볼 이름을 먼저 검색하여 관련 선언이나 호출이 존재할 수 있는 파일만 추립니다.
2. **Tree-sitter 지연 탐색 (Lazy Discovery)**: 사전 필터와 일치한 파일만 파싱하고, 순회가 확장될 때 발견한 후보를 재사용합니다. 시작 시 프로젝트 전체 AST 인덱스를 만들지 않습니다.
3. **선택적 Clang 시맨틱 강화 (Optional Semantic Strengthening)**: 현재 쿼리 후보에 연결된 컴파일 컨텍스트만 `libclang`으로 검증합니다. 데이터베이스·컨텍스트·`libclang`을 사용할 수 없으면 완전한 Tree-sitter 후보를 `[POSSIBLE]`로 유지하며, 쿼리를 실패시키거나 `compile_commands.json` 전체 엔트리를 폴백 순회하지 않습니다.

---

## 🛠 사전 요구사항 (Prerequisites)

* **Rust**: `1.80` 이상 (Cargo 포함)
* **LLVM / Clang (선택 사항)**: `libclang` (버전 16 이상 권장)을 설치하면 `[CONFIRMED]` 시맨틱 결과를 얻을 수 있습니다.
  * **Windows**: `winget install LLVM.LLVM` 또는 [LLVM 공식 릴리스](https://github.com/llvm/llvm-project/releases) 설치
  * **Linux (Ubuntu/Debian)**: `sudo apt-get install libclang-dev clang`
  * **macOS**: `brew install llvm`
* **Compilation Database (선택 사항, 권장)**: `compile_commands.json`은 Clang 검증에 빌드 컨텍스트를 제공합니다.
  * CMake 사용 시: `cmake -B build -DCMAKE_EXPORT_COMPILE_COMMANDS=ON`
  * 파일이 없거나 손상되어도 Tree-sitter 후보 분석은 계속합니다. 복구 진단은 `-v`에서 확인할 수 있습니다.

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

일반적인 사용은 메서드 하나만 `trace`에 전달하면 됩니다. 방향이나 양 끝점을 직접 지정하는 하위 명령도 그대로 제공합니다.

### 1. `trace` — 메서드 하나로 호출 경로 탐색
발견된 최상위 호출자에서 대상 메서드까지 도달하는 후보 경로를 자동으로 출력하고, Clang을 사용할 수 있으면 각 엣지를 시맨틱 검증합니다.

```bash
calljet trace <METHOD> [OPTIONS]

# 예시: Controller::dispatch까지 들어오는 호출 경로 자동 탐색
calljet trace "Controller::dispatch" --root . --compile-commands build/compile_commands.json
```

---

### 2. `callers` — 역방향 호출자 탐색
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

### 3. `callees` — 순방향 피호출자 탐색
특정 함수/심볼 내부에서 호출하는 하위 함수 체인을 온디맨드로 순방향 추적합니다.

```bash
calljet callees <SOURCE_SYMBOL> [OPTIONS]

# 예시: main 함수에서 출발하는 모든 하위 호출 관계 탐색
calljet callees main --root .

# 예시: 네임스페이스 및 클래스 메서드 지정
calljet callees "App::Controller::handle_request"
```

---

### 4. `path` — 최단 호출 경로 탐색
시작 심볼(`source`)에서 도착 심볼(`target`)까지의 구체적인 호출 경로(Call Path)를 도출합니다.

```bash
calljet path <SOURCE_SYMBOL> <TARGET_SYMBOL> [OPTIONS]

# 예시: handle_packet에서 send_response로 도달하는 호출 경로 탐색
calljet path handle_packet send_response --compile-commands build/compile_commands.json

# 예시: 최대 5단계 이내의 경로만 탐색
calljet path main calculate_checksum --max-depth 5
```

---

### 5. `explain` — 단일 호출 엣지 상세 검증 및 근거
호출자(`caller`)와 피호출자(`callee`) 사이의 호출 엣지가 존재하는 이유와 사용 가능한 Tree-sitter/Clang 근거를 상세 출력합니다.

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
| `--compile-commands <PATH>` | `-c` | `<root>/compile_commands.json` | Clang 검증에 사용할 선택적 빌드 컨텍스트 |
| `--format <FORMAT>` | `-f` | `text` | 출력 형식 (`text`, `json`, `mermaid`, `dot`) |
| `--output <FILE>` | `-o` | 표준 출력(stdout) | 분석 결과를 지정한 파일로 직접 저장 |
| `--max-depth <N>` | `-d` | 무제한 (사이클 자동 감지) | 순회 탐색의 최대 깊이 제한 |
| `--verified-only` | - | `false` | `[CONFIRMED]` 엣지만 순회하며, Clang 컨텍스트가 없으면 결과가 없을 수 있음 |
| `--no-unresolved` | - | `false` | `[UNRESOLVED]` 미해결 엣지를 결과에서 제외 |
| `--no-foreign` | - | `false` | 외부 라이브러리 경계 호출을 결과에서 제외 |
| `--metrics` | - | `false` | 소요 시간 및 메모리 등 성능 메트릭 상세 출력 |
| `--progress` | - | `false` | 결과 상세도를 바꾸거나 프로젝트 경로를 표시하지 않고 탐색/순회 집계 진행률만 출력 |
| `--verbose` | `-v` | `0` | 텍스트 상세도 증가 (`-v`: 심볼 계층/callsite, `-vv`: 근거·컨텍스트·TU·성능 지표) |
| `--help` | `-h` | - | 도움말 출력 |

---

## 📊 결과 출력 및 신뢰도 모델 (Confidence Model)

CallJet은 정적 분석의 한계를 솔직하게 표시하는 3단계 신뢰도(Confidence) 시스템을 사용합니다:

* **`[CONFIRMED]`**: Clang을 통해 정적으로 정확한 대상 심볼이 확인된 직접 호출 (Direct call)
* **`[POSSIBLE]`**: Clang으로 검사하지 못한 완전한 Tree-sitter 후보 또는 가상 함수처럼 검증 후에도 런타임 타겟이 여러 개인 호출
* **`[UNRESOLVED]`**: 함수 포인터 등 정적으로 대상을 확정할 수 없는 호출

### 기본 출력 (`calljet trace c_leaf`)
```text
c_root
c_mid
c_leaf
```

성공한 기본 text 출력은 `namespace::class::function` 형태의 정규화된 함수 심볼만 한 줄씩 표시합니다. 배너, 진행 로그, 파일 경로, 라벨, 화살표, 신뢰도, callsite는 표시하지 않습니다. `--progress`를 지정하면 compact 결과는 그대로 유지하면서 프로젝트·파일 경로 없이 집계 진행률만 추가합니다. `-v`는 `Directory`, `FullSymbol`, `Namespace`, `Class`, `Function`, callsite를, `-vv`는 시맨틱 근거, 컨텍스트, 번역 단위(TU) 상세, 성능 지표까지 표시합니다. 진행률과 결과 상세도는 서로 독립적입니다.

---

## 🚦 프로세스 종료 코드 (Exit Codes)

| 종료 코드 | 상태 | 의미 |
| :---: | --- | --- |
| **`0`** | `Complete` / `NoResult` / `Truncated` | 정상 완료 (결과 없음 또는 깊이 제한 도달 포함) |
| **`1`** | `Partial` / `InputError` / `QueryError` | 부분 분석 실패(일부 TU 오류) 또는 입력 오류 / 심볼 미발견 |
| **`2`** | `FatalError` | 심각한 내부 오류 |

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
