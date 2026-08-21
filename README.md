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

Traditional static analysis tools parse the entire codebase with a compiler front-end (such as Clang), often taking minutes to tens of minutes just to answer a focused question. CallJet solves this with a **three-step demand-driven pipeline**:

1. **Text Prefilter**: Searches for the requested symbol spelling and extracts only files that can contain a relevant declaration or call.
2. **Lazy Tree-sitter Discovery**: Parses only those matched files, then reuses their candidate symbols and call sites as traversal expands. It does not build a project-wide AST index at startup.
3. **Optional Clang Semantic Strengthening**: Uses `libclang` to upgrade candidates only in compilation contexts connected to the active query. If the database, context, or `libclang` is unavailable, CallJet keeps complete Tree-sitter candidates as `[POSSIBLE]`; it neither fails the query nor scans every entry in `compile_commands.json`.

---

## 🛠 Prerequisites

* **Rust**: `1.80` or newer (with Cargo)
* **LLVM / Clang (optional)**: `libclang` (v16+ recommended) enables `[CONFIRMED]` semantic results
  * **Windows**: `winget install LLVM.LLVM` or download from [LLVM Releases](https://github.com/llvm/llvm-project/releases)
  * **Linux (Ubuntu/Debian)**: `sudo apt-get install libclang-dev clang`
  * **macOS**: `brew install llvm`
* **Compilation Database (optional, recommended)**: `compile_commands.json` supplies build context for Clang verification
  * When using CMake: `cmake -B build -DCMAKE_EXPORT_COMPILE_COMMANDS=ON`
  * If it is missing or invalid, CallJet continues with Tree-sitter candidates. Use `-v` to see the recoverable diagnostic.

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

For the common case, pass one method to `trace`. The lower-level commands remain available for directional and two-endpoint queries.

### Which command should I use?

| Question | Command | Search direction | Default text output |
| --- | --- | --- | --- |
| “Where does execution enter this method?” | `trace <METHOD>` | Reverse, from target toward callers | Discovered top-level caller → target paths |
| “Who calls this function?” | `callers <TARGET>` | Reverse, from target toward callers | One deterministic shortest path per top-level caller |
| “What does this function call downstream?” | `callees <SOURCE>` | Forward, from source toward callees | Source → downstream callee traversal order |

The examples below use this source:

```cpp
namespace app {
void flush() {}
void save() { flush(); }
void handle() { save(); }
}

void scheduled_job() { app::save(); }
int main() { app::handle(); }
```

### 1. `trace` — One-method Call Path

This is the common entry point. Given one method, it expands incoming calls in reverse and reconstructs a connected path from every discovered top-level caller to the requested target.

* Search runs outward from the target toward callers, but output is ordered as **caller → callee → target**.
* If multiple top-level callers exist, one deterministic shortest path is printed for each, separated by a blank line.
* A complete direct, qualified, or member-call candidate remains a `[POSSIBLE]` path when Clang is unavailable or cannot obtain a referenced cursor.
* A targetless `[UNRESOLVED]` call, such as an unidentifiable function-pointer target, is excluded because it cannot form a connected path to the requested method.
* If the target symbol exists but no connected incoming edge is found, compact output is `결과 없음 (No Result)`.

```bash
calljet trace <METHOD> [OPTIONS]

# Discover paths entering app::flush
calljet trace "app::flush" --root . --compile-commands build/compile_commands.json
```

Default output:

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
# Expand at most 2 incoming call edges from the target
calljet trace "app::flush" --max-depth 2

# Show callsites and confidence for each path
calljet trace "app::flush" -v

# Construct paths using only Clang-CONFIRMED edges
calljet trace "app::flush" --verified-only
```

---

### 2. `callers` — Reverse Caller Traversal

Finds direct callers of a target symbol, then repeatedly treats each caller as the next reverse-search target to build its upstream caller chain. It reconstructs the same connected reverse paths as `trace`, while retaining lower-level caller-analysis evidence.

* Compact text is ordered as **top-level caller → requested target**, never target-first reverse order.
* If one top-level caller can reach the target through several routes, the compact path list selects one deterministic shortest path. Use JSON to inspect the complete edge set.
* Complete Tree-sitter call candidates remain target-connected `[POSSIBLE]` edges when Clang reference resolution fails, so reverse traversal can continue.
* A genuinely indirect call whose target cannot be identified may remain as targetless `[UNRESOLVED]` evidence. It is omitted from compact text and never expands the reverse frontier because it is not a connected path.
* `--max-depth N` treats the requested target as depth 0 and expands at most `N` incoming caller edges.

```bash
calljet callers <TARGET_SYMBOL> [OPTIONS]

# Discover the upstream caller chains of app::save
calljet callers "app::save" --root . --compile-commands build/compile_commands.json
```

Default output:

```text
scheduled_job
app::save

main
app::handle
app::save
```

```bash
# Stop after the direct callers of app::save
calljet callers "app::save" --max-depth 1

# Save the complete edge, confidence, and context data
calljet callers "app::save" --format json --output callers.json

# Traverse only Clang-CONFIRMED edges
calljet callers "app::save" --verified-only
```

---

### 3. `callees` — Forward Callee Traversal

Finds calls inside one source symbol, then analyzes each identified callee to expand the downstream call graph in the forward direction.

* Search and output both follow **source caller → downstream callee** direction.
* Calls made by the same function are visited in callsite source order. A symbol shared by several branches is printed once in compact text.
* `--max-depth N` treats the source as depth 0 and expands at most `N` outgoing callee edges.
* An `[UNRESOLVED]` edge without a callee identity may remain as evidence, but it cannot expand the downstream frontier.
* Use `-v`, JSON, Mermaid, or DOT output when exact branches and edge relationships matter.

```bash
calljet callees <SOURCE_SYMBOL> [OPTIONS]

# Discover downstream calls starting from app::handle
calljet callees "app::handle" --root . --compile-commands build/compile_commands.json
```

Default output:

```text
app::handle
app::save
app::flush
```

```bash
# Expand 2 outgoing edges from main: main, app::handle, app::save
calljet callees main --max-depth 2

# Save the branch structure as Mermaid
calljet callees main --format mermaid --output callees.mmd

# Traverse only Clang-CONFIRMED edges
calljet callees "app::handle" --verified-only
```

---

### 4. `path` — Shortest Call Path Discovery
Finds the shortest call path from a `<source>` symbol to a `<target>` symbol.

```bash
calljet path <SOURCE_SYMBOL> <TARGET_SYMBOL> [OPTIONS]

# Example: Trace path from handle_packet to send_response
calljet path handle_packet send_response --compile-commands build/compile_commands.json

# Example: Bounded path search within 5 hops
calljet path main calculate_checksum --max-depth 5
```

---

### 5. `explain` — Call Edge Evidence Explanation
Inspects the available syntactic or semantic evidence between a `<caller>` and a `<callee>`.

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
| `--compile-commands <PATH>` | `-c` | `<root>/compile_commands.json` | Optional build context for Clang verification |
| `--format <FORMAT>` | `-f` | `text` | Output format (`text`, `json`, `mermaid`, `dot`) |
| `--output <FILE>` | `-o` | stdout | Save result directly to a specified output file |
| `--max-depth <N>` | `-d` | Unbounded | Maximum traversal depth limit |
| `--verified-only` | - | `false` | Filter traversal to only `[CONFIRMED]` edges; no Clang context may therefore produce no result |
| `--no-unresolved` | - | `false` | Exclude `[UNRESOLVED]` indirect edges from results |
| `--no-foreign` | - | `false` | Exclude foreign external library boundary calls |
| `--metrics` | - | `false` | Output detailed timing and performance metrics |
| `--progress` | - | `false` | Show aggregate discovery/traversal progress without changing result detail or printing project paths |
| `--verbose` | `-v` | `0` | Increase result and progress detail (`-v`: aggregate progress plus symbol hierarchy/callsite, `-vv`: file-aware progress plus evidence, contexts, TU report, and metrics) |
| `--help` | `-h` | - | Display help information |

---

## 📊 Result Output & Confidence Model

CallJet employs an honest **3-state Confidence Model**:

* **`[CONFIRMED]`**: Statically proven direct call verified by Clang.
* **`[POSSIBLE]`**: A complete Tree-sitter candidate not checked by Clang, or a verified call with multiple runtime targets (e.g. virtual dispatch).
* **`[UNRESOLVED]`**: Semantic call target could not be statically determined (e.g. indirect function pointer).

### Default Output (`calljet trace c_leaf`)
```text
c_root
c_mid
c_leaf
```

Successful default text output prints only qualified function symbols, such as `namespace::class::function`, one per line. It omits the banner, progress logs, file paths, labels, arrows, confidence, and callsites. Use `--progress` to add aggregate progress while keeping that compact result and hiding project/file paths. `-v` implies aggregate progress and adds `Directory`, `FullSymbol`, `Namespace`, `Class`, `Function`, and callsites. `-vv` adds file-aware progress, semantic evidence, contexts, Translation Unit details, and performance metrics.

---

## 🚦 Exit Status Codes

| Exit Code | Completion Status | Meaning |
| :---: | --- | --- |
| **`0`** | `Complete` / `NoResult` / `Truncated` | Successful query (including empty results or depth bound reached) |
| **`1`** | `Partial` / `InputError` / `QueryError` | Partial analysis due to some TU errors, input error, or symbol not found |
| **`2`** | `FatalError` | Internal fatal error |

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
