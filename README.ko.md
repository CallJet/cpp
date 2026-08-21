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

### 어떤 명령을 선택해야 하나?

| 질문 | 명령 | 탐색 방향 | 기본 text 출력 |
| --- | --- | --- | --- |
| “이 메서드까지 어디서 들어오지?” | `trace <METHOD>` | 대상에서 caller 방향으로 역추적 | 발견된 최상위 caller → 대상 메서드 경로 |
| “이 함수를 누가 호출하지?” | `callers <TARGET>` | 대상에서 caller 방향으로 역추적 | 최상위 caller별 결정적 최단 경로 |
| “이 함수가 아래로 무엇을 호출하지?” | `callees <SOURCE>` | source에서 callee 방향으로 순방향 추적 | source → 하위 callee 순회 순서 |

아래 예시는 다음 소스를 기준으로 합니다.

```cpp
namespace app {
void flush() {}
void save() { flush(); }
void handle() { save(); }
}

void scheduled_job() { app::save(); }
int main() { app::handle(); }
```

### 1. `trace` — 메서드 하나로 호출 경로 탐색

가장 일반적인 명령입니다. 메서드 하나를 주면 해당 메서드를 호출하는 지점을 역방향으로 확장하여, 발견된 각 최상위 caller에서 대상까지 연결되는 경로를 만듭니다.

* 탐색은 대상에서 바깥쪽 caller 방향으로 진행하지만, 출력은 읽기 쉬운 **caller → callee → 대상** 순서입니다.
* 최상위 caller가 여러 개면 caller마다 결정적인 최단 경로 하나를 출력하며 경로 사이는 빈 줄로 구분합니다.
* Tree-sitter가 완전한 direct/qualified/member 호출을 찾았지만 Clang 검증이 불가능하거나 참조 cursor를 찾지 못하면 `[POSSIBLE]` 경로로 계속 탐색합니다.
* 함수 포인터처럼 대상과 연결할 수 없는 targetless `[UNRESOLVED]` 호출은 경로에 넣지 않습니다.
* 대상 심볼은 존재하지만 연결된 caller edge가 없으면 `결과 없음 (No Result)`을 출력합니다.

```bash
calljet trace <METHOD> [OPTIONS]

# app::flush까지 들어오는 경로 자동 탐색
calljet trace "app::flush" --root . --compile-commands build/compile_commands.json
```

기본 출력:

```text
scheduled_job
app::save
app::flush

main
app::handle
app::save
app::flush
```

```bash
# 대상에서 역방향으로 최대 2개 호출 edge까지만 확장
calljet trace "app::flush" --max-depth 2

# 경로별 callsite와 신뢰도 확인
calljet trace "app::flush" -v

# Clang이 CONFIRMED한 edge만 사용해 경로 구성
calljet trace "app::flush" --verified-only
```

---

### 2. `callers` — 역방향 호출자 탐색

특정 대상 심볼의 직접 caller를 찾고, 찾은 caller를 다시 대상으로 삼아 상위 caller 체인을 반복해서 확장합니다. `trace`와 같은 역방향 경로를 만들지만, 호출자 분석용 저수준 결과도 보존합니다.

* 기본 text 출력은 대상부터 거꾸로 표시하지 않고 **최상위 caller → 요청한 target** 순서로 표시합니다.
* 각 최상위 caller에서 target까지 여러 경로가 있으면 기본 경로 목록에는 결정적인 최단 경로 하나가 선택됩니다. 전체 edge 집합은 JSON 출력에서 확인할 수 있습니다.
* 완전한 Tree-sitter 호출 후보는 Clang 참조 해석이 실패해도 target에 연결된 `[POSSIBLE]` edge로 유지되어 상위 탐색을 계속합니다.
* 대상 자체를 식별할 수 없는 진짜 간접 호출은 targetless `[UNRESOLVED]` 근거로 보존할 수 있지만, 연결 경로가 아니므로 compact text에는 나오지 않고 다음 frontier로도 확장되지 않습니다.
* `--max-depth N`은 target을 깊이 0으로 보고 역방향 caller edge를 최대 `N`개까지 확장합니다.

```bash
calljet callers <TARGET_SYMBOL> [OPTIONS]

# app::save의 상위 caller 체인 탐색
calljet callers "app::save" --root . --compile-commands build/compile_commands.json
```

기본 출력:

```text
scheduled_job
app::save

main
app::handle
app::save
```

```bash
# app::save를 직접 호출하는 1단계 caller까지만 탐색
calljet callers "app::save" --max-depth 1

# 전체 edge, confidence, context를 구조화된 결과로 저장
calljet callers "app::save" --format json --output callers.json

# CONFIRMED edge만 사용해 역방향 순회
calljet callers "app::save" --verified-only
```

---

### 3. `callees` — 순방향 피호출자 탐색

특정 source 심볼의 함수 본문에서 호출되는 callee를 찾고, 식별된 callee 내부를 다시 분석하여 하위 호출 그래프를 순방향으로 확장합니다.

* 탐색과 출력 모두 **source caller → 하위 callee** 방향입니다.
* 같은 함수에서 여러 함수를 호출하면 callsite 소스 위치 순으로 방문합니다. 여러 경로에서 공유되는 심볼은 compact text에 한 번만 표시됩니다.
* `--max-depth N`은 source를 깊이 0으로 보고 순방향 callee edge를 최대 `N`개까지 확장합니다.
* callee 식별자가 없는 `[UNRESOLVED]` edge는 근거로 남을 수 있지만, 다음 함수로 이동할 수 없으므로 하위 frontier를 확장하지 않습니다.
* 경로 분기와 각 edge의 정확한 관계가 필요하면 `-v`, JSON, Mermaid 또는 DOT 출력을 사용합니다.

```bash
calljet callees <SOURCE_SYMBOL> [OPTIONS]

# app::handle에서 시작하는 하위 호출 탐색
calljet callees "app::handle" --root . --compile-commands build/compile_commands.json
```

기본 출력:

```text
app::handle
app::save
app::flush
```

```bash
# main에서 2개 호출 edge까지만 순방향 확장: main, app::handle, app::save
calljet callees main --max-depth 2

# 분기 구조를 Mermaid 파일로 저장
calljet callees main --format mermaid --output callees.mmd

# CONFIRMED edge만 사용해 하위 순회
calljet callees "app::handle" --verified-only
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
| `--verbose` | `-v` | `0` | 결과와 진행 로그 상세도 증가 (`-v`: 집계 진행률과 심볼 계층/callsite, `-vv`: 파일별 진행률과 근거·컨텍스트·TU·성능 지표) |
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

성공한 기본 text 출력은 `namespace::class::function` 형태의 정규화된 함수 심볼만 한 줄씩 표시합니다. 배너, 진행 로그, 파일 경로, 라벨, 화살표, 신뢰도, callsite는 표시하지 않습니다. `--progress`를 지정하면 compact 결과는 그대로 유지하면서 프로젝트·파일 경로 없이 집계 진행률만 추가합니다. `-v`는 집계 진행률과 `Directory`, `FullSymbol`, `Namespace`, `Class`, `Function`, callsite를 표시합니다. `-vv`는 파일별 진행률, 시맨틱 근거, 컨텍스트, 번역 단위(TU) 상세, 성능 지표까지 추가합니다.

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
