# CallJet C++ — Initial PoC

## Objective

Prove that a focused reverse call-path query can combine cheap Tree-sitter
discovery with translation-unit-batched Clang verification and avoid
project-wide semantic analysis.

The primary demonstration is:

```bash
calljet callers Foo::bar
```

with a result such as:

```text
main
└── Application::run
    └── Worker::execute
        └── Foo::bar

3 confirmed edges
0 unresolved edges
```

## Required inputs

- C or C++ source root
- optional valid `compile_commands.json` for confirmed semantic results
- target function name
- optional source function for a path query
- optional maximum search depth

## Required behavior

The PoC must:

1. Load and validate an optional compilation database without making it a
   prerequisite for Tree-sitter discovery.
2. Parse project source with Tree-sitter to identify functions and call sites.
3. Accept a target function and report ambiguous matches.
4. Find candidate callers using the lightweight discovery data.
5. Associate candidates with translation units.
6. Group verification work by translation unit.
7. Use Clang to resolve relevant call sites when their build context is usable.
8. Retain complete Tree-sitter candidates as non-confirmed when Clang is
   unavailable, then traverse with cycle and depth protection.
9. Print at least one call path with source locations.
10. Distinguish confirmed edges from unresolved relationships.
11. Run entirely locally without network access.
12. Analyze every applicable compilation context and merge equivalent results
    while preserving context provenance.

## Minimum CLI

The PoC requires two query commands:

```bash
calljet callers Foo::bar [--max-depth N] [--verified-only]
calljet path main Foo::bar [--max-depth N] [--verified-only]
```

`callees` and `explain` are part of the product concept but are not required to
prove the initial performance hypothesis. Evidence needed by a later
`explain` command must not be discarded.

## Output contract

Human-readable output must include:

- qualified function names where available
- file and line for each edge or node
- confidence for any non-confirmed edge
- total confirmed and unresolved edge counts
- a clear message for no path, ambiguous input, recoverable compilation
  database problems, and depth-limit exhaustion

The PoC may choose tree or linear rendering. Machine-readable JSON output is
deferred until a consumer requires it.

## Acceptance fixture

The repository should contain one small C++ fixture that covers:

- a direct free-function call
- a method call through a pointer or reference
- overloaded functions
- a virtual call with more than one possible runtime target
- an unresolved indirect call
- a cycle that cannot cause infinite traversal

One automated end-to-end check should compile the fixture, generate its
compilation database, run a query, and assert the expected confirmed path and
unresolved count.

## Acceptance criteria

The PoC succeeds when all of the following are demonstrated on the fixture:

- `calljet callers <target>` finds an entry-to-target path.
- Direct calls on that path are confirmed by Clang.
- Overloads are not merged solely because their names match.
- The virtual or indirect example is not falsely reported as a single definite
  runtime target.
- Each Clang-parsed translation unit is parsed at most once per query.
- Search stops at the requested depth and terminates in the presence of cycles.
- Omitting maximum depth applies no logical depth limit while cycle detection
  still guarantees termination on finite input.
- Reaching an explicit depth limit returns a successful truncated result.
- An unresolved-only query succeeds; unavailable semantic verification retains
  complete syntactic candidates as non-confirmed and a completed query exits
  zero.
- Multiple compilation contexts are all analyzed and remain attributable in
  merged output.
- Output contains navigable source locations.
- The process performs no network request.

For a larger representative project, the run should also report enough timing
data to test the hypothesis:

```text
files discovered
candidate call sites
translation units verified
discovery time
verification time
total query time
```

No fixed speed target is set before a baseline exists. The key PoC evidence is
that verified translation units are a small subset of available translation
units for a focused query.

## Explicitly deferred

- persistent index or database
- daemon or background service
- GUI, web UI, and IDE integration
- complete call-graph generation
- graph visualization
- JSON or stable machine API
- cross-language analysis
- runtime tracing, data flow, and taint analysis
- automatic remote build or dependency discovery
- general semantic-provider plugin framework

## Implementation order

1. Optional compile database loading and fixture.
2. Tree-sitter function and call-site discovery.
3. Target lookup and candidate caller search.
4. TU-batched Clang verification.
5. Bounded recursive traversal and CLI rendering.
6. End-to-end acceptance check and timing counters.

Work that does not advance an acceptance criterion is outside the initial PoC.
