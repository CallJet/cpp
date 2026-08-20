# CallJet C++ — Product Concept

## Product statement

**CallJet C++ is a fast, on-demand static call path analysis tool for C and C++.**

It answers focused questions such as:

- Who calls this function?
- How can execution reach this function?
- Is there a call path from `A()` to `Z()`?
- Which entry points eventually reach this function?
- Why does CallJet think two functions are connected?

CallJet does not build a complete semantic call graph before answering a
query. It discovers likely relationships cheaply and performs expensive
semantic analysis only where the requested path requires it.

```text
Find the path.
Skip the whole graph.
```

## Principles

```text
Parse fast.
Resolve precisely.
Trace only what matters.
```

### Query first

Traditional code-intelligence systems commonly index and resolve a project
before they can answer questions. CallJet begins with the question and expands
the analysis frontier only as needed.

```text
Traditional tool                 CallJet

Index everything                 Accept a query
Resolve everything               Discover candidates
Build a complete graph           Verify relevant candidates
Answer a query                   Stop when the query is answered
```

### Discovery is not proof

Tree-sitter provides fast syntactic discovery of functions, methods,
namespaces, call expressions, names, and source locations. Its output is a set
of candidates, not a claim of exact C++ symbol identity.

For example:

```cpp
void Worker::run() {
    foo_->execute();
}
```

Discovery may produce:

```text
Worker::run -> execute
```

Clang can then verify the relevant call as:

```text
Worker::run -> Foo::execute(Context&)
```

Tree-sitter discovers. Clang verifies.

### Uncertainty is part of the result

C++ calls are not always statically reducible to one definite target. CallJet
must expose uncertainty instead of presenting a guess as fact.

Initial result confidence levels are:

| Confidence | Meaning |
| --- | --- |
| `CONFIRMED` | Semantic evidence identifies the edge. |
| `POSSIBLE` | The edge is a valid candidate among multiple targets. |
| `UNRESOLVED` | CallJet cannot identify a target with available evidence. |

The initial product does not emit `PROBABLE`. Such a state may be considered
later only if real usage establishes a distinct, testable evidence rule.

Typical call classifications are direct, virtual, function pointer, template,
macro-expanded, foreign, and unresolved. Confidence and call kind are separate:
a virtual call, for example, may have multiple possible runtime targets even
when its static dispatch point is known.

### Explainability creates trust

Every reported edge should retain enough evidence to explain itself:

- caller and callee identity
- call-site source location
- source expression when available
- call kind
- confidence
- semantic resolution or reason it remains unresolved

CallJet should prefer “I cannot prove this call” to an unjustified exact edge.

### Local by default

CallJet is intended for proprietary and local source code. Normal operation
must require no network access and send no source-derived data outside the
machine.

```text
Source code
    ↓
Local CallJet process
    ↓
Local temporary cache or index
```

There is no external API, telemetry, cloud dependency, LLM, or embedding
service. Caches and indexes are sensitive source-derived artifacts and must be
treated accordingly.

## Initial product

| Area | Initial choice |
| --- | --- |
| Language | C and C++ |
| Interface | CLI |
| Analysis | Static |
| Execution | Local |
| Build context | `compile_commands.json` |
| Network | None |

Primary inputs are a source root, compilation database, target function, and
an optional source function. Primary outputs are call paths, callers, callees,
source locations, call classifications, confidence, and verification evidence.

## CLI experience

Find callers:

```bash
calljet callers Foo::bar
```

```text
Foo::bar
├── Worker::execute
│   └── Service::run
│       └── main
└── Controller::dispatch
    └── Application::start
```

Find a path:

```bash
calljet path main Foo::bar
```

```text
main
→ Application::start
→ Service::run
→ Worker::execute
→ Foo::bar
```

Bound the search or restrict output:

```bash
calljet callers Foo::bar --max-depth 8
calljet callers Foo::bar --verified-only
```

Explain an edge:

```bash
calljet explain Worker::execute Foo::bar
```

```text
Worker::execute -> Foo::bar

Callsite:
  src/worker.cpp:184

Expression:
  foo_->bar(ctx);

Resolution:
  foo_ : Foo*
  Foo::bar(Context&)

Verification:
  confirmed by Clang
```

The examples define intended user experience, not a frozen output format for
the PoC.

## Positioning

CallJet is an on-demand call path resolver, not a whole-codebase graph
platform.

```text
Full code-intelligence tool:
“Build a model of the entire codebase.”

CallJet:
“Tell me how this function gets called.”
```

Its performance hypothesis is that most focused call-path queries require
semantic verification for only a small subset of the source tree. Tree-sitter
may inspect broad source coverage cheaply, while Clang work should remain
restricted to the translation units reached by the query.

## Non-goals

The initial product is not a:

- GUI, web server, or IDE plugin
- complete project visualizer
- complete semantic call-graph generator
- graph database or general code-search engine
- data-flow or taint-analysis engine
- runtime tracer or profiler
- cloud, LLM, or embedding product
- polyglot call-path analyzer

The initial product remains:

```text
CLI
+ call path
+ source location
+ semantic verification
```

## Future compatibility

C and C++ are the only initial languages. The path model should nevertheless
avoid exposing Clang-specific identifiers as its public abstraction. A symbol
identity includes its language and an opaque backend identity so another
semantic provider could be added without redesigning path traversal.

Potential providers include Clang for C/C++, rust-analyzer or rustc for Rust,
Roslyn for C#, JDT for Java, and native language tooling for Go, TypeScript,
and Python. Cross-language paths are a possibility, not a current requirement.

## Product shorthand

```text
Small.
Fast.
Local.
On-demand.

Tree-sitter discovers.
Clang verifies.
CallJet traces.

Do not index the world.
Resolve only what the user asked for.
```
