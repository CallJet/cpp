# CallJet C++ — Architecture

## Architectural goal

Minimize semantic work per query without sacrificing the ability to explain
and qualify every returned call edge.

The central constraints are:

> Tree-sitter discovery is the usable baseline. Clang verification is an
> optional, demand-driven strengthening step batched by translation unit.

## Query pipeline

```text
Query
  ↓
Resolve the requested symbol
  ↓
Search the lightweight discovery index
  ↓
Collect candidate call sites
  ↓
Map candidates to translation units
  ↓
If build context is usable, parse each relevant TU once with Clang
  ↓
Verify pending candidates, otherwise retain syntactic candidates as POSSIBLE
  ↓
Add qualified edges to the query frontier
  ↓
Stop on answer, depth limit, or exhausted frontier
```

CallJet may scan project sources to build or refresh a lightweight syntactic
index. It must not eagerly perform project-wide Clang resolution or construct
a complete semantic call graph. This distinction is essential: broad cheap
discovery is allowed; broad expensive verification is not.

## Components

```text
CallJet C++
├── Discovery
│   └── Tree-sitter parsing and candidate extraction
├── Discovery index
│   ├── function declarations and definitions
│   ├── call sites
│   └── syntactic candidate relationships
├── Semantic verification
│   ├── compilation database
│   ├── translation-unit scheduling
│   └── Clang resolution
├── Path engine
│   ├── callers
│   ├── callees
│   └── source-to-target path search
└── CLI
    ├── query input
    ├── result rendering
    └── edge explanation
```

### Discovery

Tree-sitter extracts enough syntax to narrow the semantic search:

- function and method definitions
- namespaces and enclosing types
- call expressions and their textual names
- source ranges
- files likely to contain callers or callees

Discovery records facts it can observe and candidates it infers. It may assign
a location-based traversal identity to a complete candidate, but it never marks
an inferred edge as confirmed.

### Discovery index

The index supports reverse lookup from a target-like name to candidate call
sites. It is a performance aid, not the final semantic graph. It may be built
in memory for the PoC and persisted later only if measured startup cost
justifies it.

### Semantic verification

When present and usable, the compilation database supplies the command, working
directory, include paths, defines, and language options required to parse each
translation unit as the project builds it. A missing or invalid database is a
recoverable loss of semantic precision, not a discovery failure.

Candidates are grouped by translation unit before invoking Clang:

```text
candidate edges
      ↓
group by translation unit
      ↓
parse TU once
      ↓
verify every pending candidate in that TU
```

Starting or parsing Clang once per candidate is prohibited by design because
it destroys the expected performance advantage of syntactic discovery.

Headers do not independently define reliable compilation context. A candidate
in a header must be verified through a translation unit that includes it.
An unrelated source file is never associated merely because it contains the
same identifier spelling.
When no such context can be parsed, complete Tree-sitter candidates remain in
default traversal as `POSSIBLE`; `--verified-only` excludes them.

### Path engine

The path engine consumes language-neutral symbols and qualified edges. It owns
traversal, cycle handling, depth limits, termination, and path reconstruction.
It does not depend on Clang AST types.

The engine expands only the frontier required by the active query. A callers
query traverses edges in reverse; a callees query traverses forward; a path
query stops when it reaches its destination or exhausts its bounded search.
Omitting a maximum depth imposes no logical depth limit; cycle detection still
prevents repeated expansion.

### CLI

The CLI translates user-provided names into symbol candidates, reports
ambiguity instead of silently choosing an overload, runs the requested query,
and renders evidence retained by the analysis layers.

## Core model

The following Rust-like types describe responsibilities rather than a final
public API:

```rust
struct SymbolId {
    language: Language,
    backend_id: String,
}

struct Symbol {
    id: SymbolId,
    name: String,
    qualified_name: Option<String>,
    signature: Option<String>,
    location: Location,
}

struct CallEdge {
    caller: SymbolId,
    callee: Option<SymbolId>,
    callsite: Location,
    kind: CallKind,
    confidence: Confidence,
    compile_contexts: Vec<CompileContextId>,
}

enum Confidence {
    Confirmed,
    Possible,
    Unresolved,
}

enum CallKind {
    Direct,
    Virtual,
    FunctionPointer,
    Template,
    MacroExpanded,
    Foreign,
    Unresolved,
}
```

`callee` is optional because an unresolved call site is still useful evidence.
Equivalent edges from multiple applicable compile contexts are merged while
their context provenance is retained. The semantic distinction between kind
and confidence must remain.

## Symbol identity

Names are search input, not stable identity. These declarations are distinct:

```cpp
foo(int);
foo(float);
A::foo();
B::foo();
```

Verified C/C++ symbols should use a stable canonical identity derived from
Clang, such as a USR or equivalent. The path engine sees only `SymbolId` and
must not interpret backend-specific contents.

Before verification, discovery records a candidate identity containing its
available name, scope, signature hints, and source location. Candidate and
verified identities are not interchangeable proof, but traversal bridges them
by syntactic identity when a later step regains Clang verification.

## Query behavior

### Callers

1. Resolve the target symbol or report ambiguity.
2. Find syntactic call sites that may refer to it.
3. Verify candidates in TU batches when their contexts are usable.
4. Add accepted callers, or complete syntactic candidates after unavailable
   verification, to the frontier with their corresponding confidence.
5. Repeat until the depth limit or frontier is exhausted.

### Callees

1. Resolve the source function.
2. Discover calls inside its definition.
3. Verify those call sites in the owning TU when possible.
4. Add verified callees or complete syntactic candidates to the frontier.
5. Repeat within the requested bound.

### Path

1. Resolve source and target symbols.
2. Traverse a bounded frontier using verified or explicitly qualified
   syntactic candidate edges.
3. Stop when a path reaches the target.
4. Return the path with source locations and confidence.

The initial implementation may use breadth-first search to return a path with
the fewest edges. More elaborate ranking belongs only after real result data
shows that shortest-edge paths are insufficient.

### Explain

An explanation is rendered from evidence already stored on an edge. It should
not require reconstructing the analysis from scratch. At minimum it includes
the call site, expression or source range, resolved symbol when available,
call kind, confidence, and verifier outcome.

## Cache and security boundary

All state remains local. A cache must contain only what is necessary to avoid
repeated discovery or TU parsing and must be safe to delete. It is never an
authoritative source of truth.

Source-derived indexes may reveal names, paths, structure, and code fragments.
They therefore receive the same confidentiality assumptions as the source
tree. Network transport is outside the architecture.

## Performance invariants

- Never run project-wide Clang analysis as a prerequisite for one query.
- Never make Tree-sitter discovery depend on Clang or a compilation database.
- Never parse the same translation unit once per candidate within a query.
- Stop expanding when the query is answered or its bound is reached.
- Measure discovery breadth, verified TU count, and elapsed time separately.
- Prefer an in-memory cache until persistence has a demonstrated benefit.

## Initial dependency direction

```text
CLI → Query → Path engine
              ↑       ↑
        Discovery   Semantic provider
                         ↑
                       Clang
```

The core may define a narrow semantic-provider boundary when multiple
implementations or test substitution require it. The PoC should not build a
general plugin system in anticipation of future languages.
