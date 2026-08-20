<div align="center">
  <img src="resources/CallJet_Banner.png" alt="CallJet Banner" width="100%">
</div>

# CallJet C++

> **Find the path. Skip the whole graph.**  
> Fast, on-demand static call path analysis for C and C++ without building the entire semantic call graph upfront.

[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.80+-orange.svg)](https://www.rust-lang.org/)

[English](README.md) | [한국어 (Korean)](README.ko.md)

---

## 📌 Core Principle

Traditional static analysis tools parse the entire codebase with a compiler front-end (such as Clang), often taking minutes to tens of minutes just to answer a focused question. CallJet solves this with a **two-phase hybrid demand-driven pipeline**:

1. **Tree-sitter Fast Candidate Discovery**: Syntactically scans the entire project in milliseconds to extract candidate call sites, declarations, definitions, and reverse lookup indexes (`calls_by_spelling`).
2. **Clang Demand-Driven Semantic Verification**: Leverages `libclang` C FFI to semantically verify **only the specific Translation Units (TUs) required along the query's active traversal frontier**.

---

## 🛠 Prerequisites

* **Rust**: `1.80` or newer (with Cargo)
* **LLVM / Clang**: `libclang` (v16+ recommended)
  * **Windows**: `winget install LLVM.LLVM` or download from [LLVM Releases](https://github.com/llvm/llvm-project/releases)
  * **Linux (Ubuntu/Debian)**: `sudo apt-get install libclang-dev clang`
  * **macOS**: `brew install llvm`
* **Compilation Database**: `compile_commands.json` for your project
  * When using CMake: `cmake -B build -DCMAKE_EXPORT_COMPILE_COMMANDS=ON`

---

## 🚀 Build & Installation

```bash
# Clone repository and navigate to cpp directory
cd cpp

# Build optimized release binary
cargo build --release

# The executable will be at: target/release/calljet (or target/release/calljet.exe on Windows)
```

---

## 💻 CLI Usage Guide

CallJet provides 4 core query commands.

### 1. `callers` — Reverse Caller Traversal
Finds all callers that lead to a specific target function/symbol on demand.

```bash
calljet callers <TARGET_SYMBOL> [OPTIONS]

# Example: Find all callers of LeafFunction
calljet callers LeafFunction --root . --compile-commands build/compile_commands.json

# Example: Limit search depth to 2 hops
calljet callers Math::Calculator::add --max-depth 2

# Example: Traverse only verified direct calls (CONFIRMED)
calljet callers process_data --verified-only
```

---

### 2. `callees` — Forward Callee Traversal
Finds all downstream functions invoked from a given source function/symbol on demand.

```bash
calljet callees <SOURCE_SYMBOL> [OPTIONS]

# Example: Find all callees starting from main
calljet callees main --root .

# Example: Target a qualified C++ method
calljet callees "App::Controller::handle_request"
```

---

### 3. `path` — Shortest Call Path Discovery
Finds the shortest call path from a `<source>` symbol to a `<target>` symbol.

```bash
calljet path <SOURCE_SYMBOL> <TARGET_SYMBOL> [OPTIONS]

# Example: Trace path from handle_packet to send_response
calljet path handle_packet send_response --compile-commands build/compile_commands.json

# Example: Bounded path search within 5 hops
calljet path main calculate_checksum --max-depth 5
```

---

### 4. `explain` — Call Edge Evidence Explanation
Inspects the semantic verification evidence between a `<caller>` and a `<callee>`.

```bash
calljet explain <CALLER_SYMBOL> <CALLEE_SYMBOL> [OPTIONS]

# Example: Explain why dispatch is called inside process_event
calljet explain process_event dispatch
```

---

## ⚙️ Options

| Option | Short | Default | Description |
| --- | :---: | --- | --- |
| `--root <PATH>` | `-r` | `.` (current directory) | Root directory of the source project |
| `--compile-commands <PATH>` | `-c` | `<root>/compile_commands.json` | Path to `compile_commands.json` |
| `--format <FORMAT>` | `-f` | `text` | Output format (`text`, `json`, `mermaid`, `dot`) |
| `--output <FILE>` | `-o` | stdout | Save result directly to a specified output file |
| `--max-depth <N>` | `-d` | Unbounded | Maximum traversal depth limit |
| `--verified-only` | - | `false` | Filter traversal to only `[CONFIRMED]` edges |
| `--no-unresolved` | - | `false` | Exclude `[UNRESOLVED]` indirect edges from results |
| `--no-foreign` | - | `false` | Exclude foreign external library boundary calls |
| `--metrics` | - | `false` | Output detailed timing and performance metrics |
| `--verbose` | - | `false` | Display detailed per-file Translation Unit (TU) breakdown report |
| `--help` | `-h` | - | Display help information |

---

## 📊 Result Output & Confidence Model

CallJet employs an honest **3-state Confidence Model**:

* **`[CONFIRMED]`**: Statically proven direct call verified by Clang.
* **`[POSSIBLE]`**: Valid candidate with multiple runtime targets (e.g. virtual method dispatch).
* **`[UNRESOLVED]`**: Semantic call target could not be statically determined (e.g. indirect function pointer).

### Example Output (`calljet callers c_leaf`)
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

## 🚦 Exit Status Codes

| Exit Code | Completion Status | Meaning |
| :---: | --- | --- |
| **`0`** | `Complete` / `NoResult` / `Truncated` | Successful query (including empty results or depth bound reached) |
| **`1`** | `Partial` / `InputError` / `QueryError` | Partial analysis due to some TU errors, input error, or symbol not found |
| **`2`** | `FatalError` | Internal error or failure to load libclang |

---

## 🧪 Testing & Benchmarks

```bash
# Run all unit, Clang FFI, traversal, and CLI tests
cargo test

# Run formal SRS Acceptance Criteria (AC-001 ~ AC-018) suite
cargo test --test acceptance_suite_tests

# Run PoC Architectural Hypothesis Benchmark
cargo test --test benchmark_measurements -- --nocapture
```

---

## 📚 Documentation

* [Product Concept](docs/concept.md) — Problem statement, product scope, and core philosophy
* [Software Requirements Specification (SRS)](docs/srs.md) — 81 functional requirements and acceptance criteria
* [Software Design Specification (SDS)](docs/sds.md) — Implementation architecture, data models, and algorithms
* [PoC Benchmark Report](docs/benchmark_report.md) — Empirical TU reduction and performance measurements
