# CallJet C++ — Software Requirements Specification

| Field | Value |
| --- | --- |
| Status | Initial baseline |
| Product | CallJet C++ |
| Primary source | [Product Concept](concept.md) |
| Supporting sources | [Architecture](architecture.md), [Initial PoC](poc.md) |
| Normative language | `MUST` and `SHALL` are mandatory |

## 1. Purpose and precedence

This SRS defines CallJet C++'s required behavior and externally verifiable
qualities. It does not prescribe internal modules, APIs, data structures,
algorithms, or implementation sequence.

The product concept is authoritative. Architecture and PoC documents provide
supporting detail. Differences are resolved in favor of the concept unless
that would make a requirement technically incoherent. Resolutions are recorded
in [Section 10](#10-source-ambiguities-and-resolutions).

## 2. Product purpose and scope

CallJet C++ is a local, on-demand static call-path analysis CLI for C and C++.
It answers focused questions about callers, callees, paths between functions,
and evidence for individual call edges without first requiring a complete
project-wide semantic call graph.

The initial product accepts a source root, a `compile_commands.json`
compilation database, a target symbol, and, where applicable, a source symbol.
It reports qualified symbols, call relationships, call paths, source
locations, call classifications, confidence, and semantic evidence.

### 2.1 Initial scope

- Languages: C and C++ only.
- Interface: command-line interface only.
- Analysis: static analysis only.
- Execution: local and offline.
- Build context: a local `compile_commands.json`.
- Query model: focused, bounded, and on-demand.

### 2.2 Actors

| Actor | Role |
| --- | --- |
| Developer | Queries callers, callees, and paths while navigating or debugging a C/C++ codebase. |
| Reviewer or maintainer | Inspects reachability and evidence behind reported call edges. |
| Build system | Produces source and the compilation database consumed by CallJet. |
| Local environment | Supplies local files, process execution, and temporary storage. |

No remote service is an actor in the initial product.

## 3. Assumptions, constraints, and terminology

### 3.1 Assumptions

- The user is authorized to read and analyze the supplied project.
- The compilation database represents the build configuration of interest.
- Required source, generated headers, dependencies, and compiler resources are
  locally accessible.
- Static evidence cannot uniquely prove every runtime target of virtual calls,
  function pointers, callbacks, templates, macros, or foreign interfaces.
- Source or build-context changes may invalidate reused analysis results.

### 3.2 Constraints

| ID | Constraint |
| --- | --- |
| CON-001 | The initial product SHALL analyze only C and C++ source. |
| CON-002 | The initial product SHALL expose analysis through a local CLI. |
| CON-003 | The product SHALL perform static analysis and SHALL NOT require execution of the analyzed program. |
| CON-004 | The product SHALL use the supplied `compile_commands.json` as semantic build context. |
| CON-005 | The product SHALL separate candidate discovery from semantic verification so unverified candidates are not represented as proven edges. |
| CON-006 | Semantic verification SHALL be limited to work relevant to the active query and SHALL NOT require a complete project-wide semantic call graph. |
| CON-007 | Normal operation SHALL require no network connection or remote service. |
| CON-008 | The initial product SHALL use Tree-sitter for syntactic discovery and Clang for C/C++ semantic verification, as mandated by the product concept; this does not prescribe internal component design. |

### 3.3 Terminology

| Term | Definition |
| --- | --- |
| Source root | Local directory bounding project source supplied for analysis. |
| Compilation database | Supplied `compile_commands.json` containing translation-unit build contexts. |
| Compilation context | One distinct build interpretation of a translation unit, including its working directory and semantic compiler arguments. |
| Translation unit (TU) | A source file interpreted with one compilation command and its included content. |
| Symbol query | User input intended to identify a function or method. |
| Symbol identity | Canonical identity distinguishing overload, scope, owner, and signature; a display name alone is not identity. |
| Candidate | Syntactically plausible symbol or call relationship not yet semantically proven. |
| Call edge | Reported caller-to-callee relationship at a call site. |
| Call site | Source location and expression where a call occurs. |
| Caller | Function or method containing a call site that may reach the queried callee. |
| Callee | Function or method that may be invoked by a call site in the queried caller. |
| Call path | Ordered sequence of symbols connected by call edges. |
| Verification | Semantic evaluation of a candidate using supplied build context. |
| Confidence | Degree to which available evidence supports a relationship. |
| Call kind | Form of call: direct, virtual, function-pointer, template, macro-expanded, foreign, or unresolved. |
| Entry point | Function at the outer end of a caller chain; not necessarily only language-level `main`. |
| Unresolved result | A successfully analyzed call for which semantic evidence cannot identify a unique callee. |
| Partial result | A query result produced after one or more required analysis operations failed, containing any independently valid results and failure diagnostics. |
| Truncated result | A successful bounded-query result whose traversal stopped at the user-supplied maximum depth. |

## 4. Functional requirements

Each requirement is mandatory for the complete initial product unless the PoC
acceptance section explicitly assigns it to a later milestone. The verification
column defines minimum test evidence.

### 4.1 Project and source input

| ID | Requirement | Verification |
| --- | --- | --- |
| FR-001 | The CLI SHALL accept a local source root for the project to analyze. | Invoke with a valid root and confirm analysis is scoped to it. |
| FR-002 | The product SHALL accept C and C++ source represented by the supplied inputs. | Analyze fixtures containing C and C++ translation units. |
| FR-003 | The product SHALL reject a source root that is absent, is not a directory, or cannot be read, and SHALL identify the rejected input. | Exercise each invalid condition. |
| FR-004 | The product SHALL report a required source file that cannot be read and SHALL NOT present dependent edges as confirmed. | Remove or deny access to a required fixture file. |
| FR-005 | The product SHALL restrict normal source inputs and source-derived outputs to the local environment. | Monitor file and network activity during a query. |

### 4.2 Compilation database handling

| ID | Requirement | Verification |
| --- | --- | --- |
| FR-006 | The CLI SHALL accept a local `compile_commands.json` as input. | Invoke with an explicitly supplied valid database. |
| FR-007 | The product SHALL validate that the database exists, is readable, is syntactically valid, and contains usable TU entries before relying on it. | Test missing, unreadable, malformed, empty, and valid databases. |
| FR-008 | The product SHALL interpret each analyzed TU using the working directory, source file, compiler arguments or command, and build options in its database entry. | Use a fixture whose resolution depends on an include path and definition. |
| FR-009 | The product SHALL report unusable database entries with the affected entry and reason. | Supply missing-file and unusable-command entries. |
| FR-010 | The product SHALL report when no database entry supplies build context for a TU required by the query. | Query a source omitted from the database. |
| FR-011 | The product SHALL NOT guess missing build options and report the resulting relationship as confirmed. | Omit required compile context and inspect status. |
| FR-078 | When multiple compilation contexts apply to a required source file, the product SHALL analyze every applicable context, merge equivalent results, and preserve the contributing context provenance on each reported edge. | Analyze a source whose two contexts select different conditional calls and inspect both edges and their provenance. |

### 4.3 Target symbol identification

| ID | Requirement | Verification |
| --- | --- | --- |
| FR-012 | The CLI SHALL accept a function or method symbol query wherever a command requires a source or target symbol. | Invoke relevant commands with free functions and methods. |
| FR-013 | The product SHALL distinguish symbols sharing a name but differing by namespace, owning type, overload signature, or source identity. | Query a fixture containing every collision type. |
| FR-014 | The product SHALL assign a stable canonical identity to a verified C/C++ symbol within analysis of unchanged inputs. | Resolve one overload through multiple sites and compare identities. |
| FR-015 | The product SHALL report all viable matches for an ambiguous query and SHALL NOT silently select one. | Query an ambiguous overload or unqualified name. |
| FR-016 | The product SHALL report when no symbol matches the query. | Query an absent name. |
| FR-017 | A displayed symbol SHALL include its qualified name when available. | Inspect namespaced and member symbols. |
| FR-018 | A displayed symbol SHALL include its signature when needed to distinguish viable symbols. | Inspect overloaded symbol output. |

### 4.4 Caller discovery

| ID | Requirement | Verification |
| --- | --- | --- |
| FR-019 | A callers query SHALL return functions and methods containing call sites that may invoke the target. | Run against direct and method-call fixtures. |
| FR-020 | A callers query SHALL support recursive traversal from the target toward outer callers. | Query a chain of at least three levels. |
| FR-021 | A callers query SHALL support a user-supplied maximum depth. | Run with limits below and above a known chain length. |
| FR-022 | A callers query SHALL stop when its frontier is exhausted, its depth limit is reached, or requested results are complete. | Exercise empty, bounded, and completed searches. |
| FR-023 | A callers query SHALL prevent cycles from causing infinite traversal or unbounded duplicate output. | Query a cyclic fixture under a timeout. |
| FR-024 | A callers query SHALL retain and report non-confirmed candidates unless verified-only output is requested. | Compare default and filtered mixed-confidence output. |

### 4.5 Callee discovery

| ID | Requirement | Verification |
| --- | --- | --- |
| FR-025 | A callees query SHALL return functions and methods that may be invoked by call sites within the source symbol. | Query a function containing direct and method calls. |
| FR-026 | A callees query SHALL support recursive traversal toward downstream callees. | Query a chain of at least three levels. |
| FR-027 | A callees query SHALL support a user-supplied maximum depth. | Run with limits below and above a known chain length. |
| FR-028 | A callees query SHALL stop on exhausted frontier or depth limit and SHALL prevent cycles from causing infinite traversal or unbounded duplicate output. | Exercise leaf, bounded, and cyclic fixtures. |
| FR-029 | A callees query SHALL retain and report non-confirmed candidates unless verified-only output is requested. | Compare default and filtered mixed-confidence output. |

### 4.6 Path queries

| ID | Requirement | Verification |
| --- | --- | --- |
| FR-030 | A path query SHALL accept one source symbol and one target symbol. | Invoke with two resolvable symbols. |
| FR-031 | A path query SHALL determine whether at least one static call path connects the selected symbols within search bounds. | Test connected and disconnected fixtures. |
| FR-032 | When a path is found, the product SHALL return an ordered source-to-target sequence of symbols and edges. | Compare order with the fixture. |
| FR-033 | When no path is found after complete search, the product SHALL report that outcome without fabricating an edge. | Query disconnected symbols. |
| FR-034 | A path query SHALL support a user-supplied maximum depth. | Query on both sides of the required depth. |
| FR-035 | When a user-supplied depth limit prevents further traversal, the product SHALL return a successful truncated result that identifies the reached limit and does not claim that no deeper path exists. | Set a limit shorter than a known path and inspect result metadata. |
| FR-036 | A path query SHALL terminate in the presence of call cycles. | Query a cyclic fixture under a timeout. |
| FR-037 | Every returned path SHALL preserve confidence and call kind for each edge. | Inspect a mixed-confidence path. |
| FR-077 | When maximum depth is omitted, recursive callers, callees, and path queries SHALL have no logical depth limit and SHALL still apply cycle detection. | Run an unbounded query over a chain longer than bounded fixtures and a cyclic graph. |

### 4.7 Semantic verification

| ID | Requirement | Verification |
| --- | --- | --- |
| FR-038 | The product SHALL semantically verify candidates relevant to the query using supplied C/C++ build context before labeling them `CONFIRMED`. | Query same-name candidates where C++ semantics select one. |
| FR-039 | Verification SHALL distinguish exact symbol identity, namespace, owning type, and overload signature. | Exercise collisions in one fixture. |
| FR-040 | Verification SHALL account for relevant templates and compile-time dispatch information available in the TU. | Analyze template and virtual-call fixtures. |
| FR-041 | The product SHALL associate a verified symbol with its resolved signature and declaration or definition location when available. | Inspect verified symbol evidence. |
| FR-042 | The product SHALL NOT elevate a candidate to `CONFIRMED` solely because its textual name matches. | Use unrelated same-named symbols. |
| FR-043 | The product SHALL make verification status available for every reported edge. | Inspect mixed-status output. |
| FR-044 | The product SHALL NOT semantically verify a TU that supplies no candidate or build context required by the active query. | Instrument a focused multi-TU query containing an unrelated TU. |
| FR-045 | Within one query, the product SHALL NOT repeat semantic interpretation of the same unchanged TU build context for separate pending candidates. | Instrument multiple candidates in one TU. |

### 4.8 Confidence and unresolved results

| ID | Requirement | Verification |
| --- | --- | --- |
| FR-046 | Every reported edge SHALL have exactly one state: `CONFIRMED`, `POSSIBLE`, or `UNRESOLVED`. | Validate every edge in a mixed fixture. |
| FR-047 | `CONFIRMED` SHALL mean semantic evidence identifies the edge. | Compare a direct call with semantic evidence. |
| FR-049 | `POSSIBLE` SHALL mean the edge is a valid candidate among targets that cannot be uniquely proven. | Analyze multiple virtual or indirect targets. |
| FR-050 | `UNRESOLVED` SHALL mean semantic analysis completed successfully but available evidence could not identify a unique callee target. | Analyze an unresolved indirect call without causing an analysis failure. |
| FR-051 | The product SHALL expose unresolved call sites instead of omitting them or converting them into definite targets. | Inspect unresolved fixture output. |
| FR-052 | Every edge SHALL have a call kind independent of confidence. | Validate both fields for direct and virtual calls. |
| FR-053 | The product SHALL support direct, virtual, function-pointer, template, macro-expanded, foreign, and unresolved call kinds. | Exercise or unit-check each classification. |
| FR-054 | Verified-only output SHALL exclude every edge not marked `CONFIRMED`. | Compare default and verified-only output. |
| FR-076 | The initial product SHALL NOT emit or expose a `PROBABLE` confidence state. | Inspect all CLI operations and confidence constructors. |
| FR-079 | A query containing `UNRESOLVED` edges but no analysis failure SHALL be a successfully completed query with zero process status. | Run an indirect-call fixture that resolves to `UNRESOLVED` and inspect status. |

`FR-048` is retired because the initial `PROBABLE` definition was removed. The
identifier is permanently reserved and is not reused.

### 4.9 Source-location reporting

| ID | Requirement | Verification |
| --- | --- | --- |
| FR-055 | Every reported edge SHALL include call-site file path and line when available. | Compare output with fixture source. |
| FR-056 | Every resolved symbol SHALL include declaration or definition file path and line when available. | Compare output with fixture source. |
| FR-057 | The product SHALL distinguish unavailable location and SHALL NOT invent a path or line. | Analyze a construct lacking physical location. |
| FR-058 | Reported paths SHALL preserve enough per-edge location data to navigate every transition. | Inspect a multi-edge path. |

### 4.10 Explainability

| ID | Requirement | Verification |
| --- | --- | --- |
| FR-059 | The CLI SHALL provide an edge-explanation query accepting a caller and callee. | Invoke it for an existing edge. |
| FR-060 | An explanation SHALL identify caller, callee or unresolved target, call site, call kind, confidence, and verification outcome. | Compare fields with a fixture edge. |
| FR-061 | An explanation SHALL include source expression or source range when available. | Compare a direct-call explanation with source. |
| FR-062 | For a non-confirmed edge, the explanation SHALL state why evidence did not prove a unique target. | Explain virtual, indirect, and unresolved edges. |
| FR-063 | If multiple call sites connect requested symbols, explanation SHALL identify each site or require further disambiguation. | Create two calls between the same symbols. |
| FR-064 | If the requested edge is absent, the product SHALL report no matching edge. | Explain a disconnected pair. |

### 4.11 CLI behavior and error handling

| ID | Requirement | Verification |
| --- | --- | --- |
| FR-065 | The executable SHALL be invocable as `calljet`. | Invoke the built executable by name. |
| FR-066 | The complete initial product CLI SHALL provide callers, callees, path, and explain operations. | Inspect help and invoke each operation. |
| FR-067 | The CLI SHALL provide help describing required inputs and supported options. | Invoke top-level and command help. |
| FR-068 | The CLI SHALL reject missing arguments, unsupported options, and invalid values with a diagnostic identifying the problem. | Exercise every invalid class. |
| FR-069 | The CLI SHALL return non-zero status for invalid invocation, invalid required input, ambiguous required symbol selection, or analysis failure. | Capture status for every condition. |
| FR-070 | A completed query SHALL return zero status, including a query with no result, unresolved edges without analysis failure, or user-requested depth truncation. | Capture positive, empty, unresolved, and truncated-query statuses. |
| FR-071 | The CLI SHALL distinguish invalid input, ambiguity, no result, truncated result, unresolved result, partial result, and internal failure. | Trigger and compare each outcome. |
| FR-072 | The CLI SHALL provide maximum-depth control for recursive callers, callees, and path queries. | Invoke each operation with a depth. |
| FR-073 | The CLI SHALL provide verified-only control for callers, callees, and path queries. | Compare mixed-confidence output. |
| FR-074 | Human-readable output SHALL include qualified names where available, source locations, confidence for non-confirmed edges, and confirmed and unresolved edge counts. | Inspect mixed multi-edge output. |
| FR-075 | The initial release SHALL NOT require a machine-readable output format. | Confirm acceptance does not depend on such output. |
| FR-080 | If a required analysis operation fails and independently valid results exist, the product SHALL return a partial result containing those results and diagnostics identifying the failed work. | Cause one required TU to fail while another yields a confirmed edge. |
| FR-081 | A partial result SHALL return non-zero process status, while a truncated result caused only by the user-supplied depth limit SHALL return zero process status. | Capture status for one partial query and one bounded truncated query. |

## 5. Non-functional requirements

### 5.1 Performance and resource usage

| ID | Requirement | Verification |
| --- | --- | --- |
| NFR-001 | The product SHALL begin from the query and SHALL NOT require a complete project-wide semantic call graph as a prerequisite. | Instrument semantic work during a focused multi-TU query. |
| NFR-002 | The product SHALL stop expanding analysis when the answer is complete, configured depth is reached, or reachable frontier is exhausted. | Inspect work counters for all three cases. |
| NFR-003 | Each benchmark run SHALL report source files inspected, candidate call sites, available TUs, verified TUs, discovery time, verification time, total query time, and peak resident memory. | Run the benchmark and validate every field. |
| NFR-004 | End-to-end latency targets SHALL remain `TBD` until measured on a versioned representative corpus and reference machine using repeated cold and warm runs with median and tail observations. | Review the benchmark record and approved target before gating release latency. |
| NFR-005 | Peak-memory and source-derived storage targets SHALL remain `TBD` until the same setup records peak resident memory and maximum local cache or temporary-storage bytes. | Review the benchmark record and approved target before gating resources. |
| NFR-006 | Maximum supported project size SHALL remain `TBD` until tests increase source-file, TU, and candidate counts and identify where correctness fails or approved resource targets are exceeded. | Review the scale report and published limit. |
| NFR-007 | The product SHALL expose or record enough counters to determine whether semantic verification was restricted to a query-relevant subset. | Compare available and verified TU counts. |
| NFR-008 | Temporary or cached artifacts SHALL be safe to delete without loss or modification of source files. | Delete artifacts, compare source hashes, and rerun the query. |

### 5.2 Correctness and determinism

| ID | Requirement | Verification |
| --- | --- | --- |
| NFR-009 | The product SHALL NOT mark an edge `CONFIRMED` unless semantic evidence supports the exact identities at the reported call site. | Run adversarial same-name, overload, and dispatch fixtures. |
| NFR-010 | The product SHALL preserve uncertainty where runtime behavior cannot be uniquely proven by static evidence. | Analyze virtual, pointer, callback, and unresolved fixtures. |
| NFR-011 | With unchanged source, database, options, toolchain, and environment, repeated queries SHALL produce the same symbols, edges, confidence, ordering, diagnostics, and status; timing and resource values are excluded. | Compare normalized repeated output. |
| NFR-012 | Result ordering SHALL remain stable when only incidental file enumeration or work scheduling order changes. | Vary incidental order and compare normalized output. |
| NFR-013 | The product SHALL detect source or build-context changes that make reused results stale before reporting them as current. | Change a call and a compile definition between queries. |
| NFR-014 | Failure to analyze one relationship SHALL NOT raise another relationship's confidence beyond its evidence. | Inject a localized analysis failure into a mixed fixture. |

### 5.3 Local operation and security

| ID | Requirement | Verification |
| --- | --- | --- |
| NFR-015 | All normal analysis SHALL function with network access disabled. | Run acceptance in a network-denied environment. |
| NFR-016 | The product SHALL make no outbound network request during normal operation. | Monitor outbound connections during acceptance. |
| NFR-017 | The product SHALL include no telemetry, cloud-analysis dependency, external API dependency, LLM service, or embedding service in normal operation. | Inspect dependencies and monitor execution. |
| NFR-018 | Source and source-derived data SHALL remain on the local machine during normal operation. | Monitor file and network activity. |
| NFR-019 | Every cache, index, log, diagnostic artifact, or temporary file containing source-derived data SHALL be stored locally and documented as sensitive. | Inspect artifacts and documentation. |
| NFR-020 | The product SHALL NOT modify analyzed source or the supplied compilation database. | Compare pre-query and post-query hashes. |
| NFR-021 | The product SHALL limit source-derived artifacts to data needed for local analysis, reporting, diagnostics, or measured reuse. | Compare artifact contents with documented purpose. |

### 5.4 Scalability and portability

| ID | Requirement | Verification |
| --- | --- | --- |
| NFR-022 | For a focused query whose candidate set excludes at least one TU, the product SHALL NOT semantically verify an excluded TU. | Run a controlled multi-TU fixture and compare candidate and verification sets. |
| NFR-023 | Recursive queries SHALL remain bounded by frontier exhaustion or user depth and SHALL terminate for finite project input. | Run deep and cyclic scale fixtures under a timeout. |
| NFR-024 | Release documentation SHALL list every supported host OS, architecture, required Clang compatibility range, and compilation-database assumption. | Inspect release documentation. |
| NFR-025 | The CLI SHALL meet the same functional requirements on every documented host, except for host-native path representation and platform-specific environment diagnostics. | Run conformance on each supported host. |
| NFR-026 | The initial supported-host list SHALL remain `TBD` until repeatable build and conformance checks pass on each claimed host. | Review conformance evidence before a support claim. |

## 6. Non-goals

The following are outside the initial product and are excluded from acceptance:

- GUI, web server, or IDE plugin
- complete project visualization or project-wide semantic call graph
- graph database or general code-search engine
- data-flow or taint analysis
- runtime tracing or profiling
- cloud processing, external APIs, telemetry, LLMs, or embeddings
- machine-readable output or a stable programmatic API
- persistent index or background daemon
- automatic remote build or dependency discovery
- languages other than C and C++
- cross-language paths or a general backend-plugin framework

Future language compatibility is an architectural possibility, not an initial
functional requirement.

## 7. Initial PoC / first acceptance milestone

This is the first acceptance milestone. It does not remove `callees` or
`explain` from the complete initial product; those remain requirements but are
deferred beyond this PoC milestone as resolved in Section 10.

| ID | Acceptance criterion | Covered requirements |
| --- | --- | --- |
| AC-001 | A local C/C++ fixture SHALL be analyzed using a valid `compile_commands.json`. | FR-001–FR-011, CON-001–CON-004 |
| AC-002 | `calljet callers <target>` SHALL find a multi-edge entry-to-target chain and report navigable source locations. | FR-012–FR-024, FR-055–FR-058, FR-065 |
| AC-003 | `calljet path <source> <target>` SHALL return a known path and correctly report disconnected and depth-limited cases. | FR-030–FR-037, FR-071–FR-073 |
| AC-004 | Direct calls SHALL be confirmed, while same-name and overloaded symbols SHALL not be merged by text alone. | FR-013–FR-018, FR-038–FR-043, NFR-009 |
| AC-005 | A virtual or indirect call SHALL not be presented as one definite runtime target when evidence cannot prove it. | FR-046–FR-047, FR-049–FR-053, NFR-010 |
| AC-006 | An unresolved call SHALL be retained, labeled, counted, and reported without a fabricated target. | FR-043, FR-046, FR-050–FR-052, FR-074 |
| AC-007 | Recursive traversal SHALL stop at depth and terminate in the presence of a cycle. | FR-021–FR-023, FR-034–FR-036, NFR-023 |
| AC-008 | Relevant candidates sharing one TU build context SHALL require one semantic interpretation of that context within a query. | FR-045 |
| AC-009 | A focused multi-TU query SHALL show that a complete semantic graph is not prerequisite and SHALL report the NFR-003 workload counters. | FR-044, NFR-001–NFR-003, NFR-007 |
| AC-010 | Acceptance SHALL succeed with network denied and SHALL produce no outbound request or source modification. | FR-005, NFR-015–NFR-020 |
| AC-011 | Invalid roots, databases, invocations, ambiguous symbols, and missing symbols SHALL produce specified diagnostics and statuses. | FR-003, FR-006–FR-010, FR-015–FR-016, FR-068–FR-071 |
| AC-012 | Repeated execution with unchanged inputs SHALL produce deterministic semantic output. | NFR-011–NFR-012 |
| AC-013 | The acceptance fixture SHALL cover a direct free-function call, method call through a pointer or reference, overloads, a virtual call with multiple possible targets, an unresolved indirect call, and a cycle. | Fixture source inspection and build. |
| AC-014 | An automated end-to-end check SHALL assert the expected confirmed path and unresolved count. | Execute the check against the acceptance fixture. |
| AC-015 | The PoC SHALL produce every measurement and workload counter specified by NFR-003. | Inspect benchmark output. |
| AC-016 | A recursive query without maximum depth SHALL traverse beyond an arbitrary shallow bound and SHALL terminate when a cycle is present. | Run an unbounded deep-chain and cyclic fixture. |
| AC-017 | A source compiled once with `FEATURE_A` and once with `FEATURE_B` SHALL report context-specific edges from both contexts and preserve their provenance after merge. | Run the two-context conditional-compilation fixture. |
| AC-018 | An unresolved-only query SHALL exit zero, a depth-truncated query SHALL exit zero and report truncation, and a required-TU failure with usable results SHALL return partial output and exit non-zero. | Execute and capture all three result classes. |

Fixed latency, memory, storage, scale, and supported-host gates are not
fabricated for the PoC. NFR-004–NFR-006 and NFR-026 govern when evidence is
sufficient to replace each `TBD`.

## 8. Requirements traceability

Concept references are primary. Supporting references clarify testing or PoC
phasing but do not override the concept.

| Requirement IDs | Primary concept source | Supporting source |
| --- | --- | --- |
| CON-001–CON-004, CON-007 | [Initial product](concept.md#initial-product), [Local by default](concept.md#local-by-default) | [PoC inputs](poc.md#required-inputs) |
| CON-005–CON-006, CON-008 | [Principles](concept.md#principles), [Discovery is not proof](concept.md#discovery-is-not-proof), [Query first](concept.md#query-first) | [Query pipeline](architecture.md#query-pipeline) |
| FR-001–FR-005 | [Initial product](concept.md#initial-product), [Local by default](concept.md#local-by-default) | [PoC inputs](poc.md#required-inputs) |
| FR-006–FR-011, FR-078 | [Initial product](concept.md#initial-product) | [Semantic verification](architecture.md#semantic-verification), [PoC behavior](poc.md#required-behavior) |
| FR-012–FR-018 | [Initial product](concept.md#initial-product), [Discovery is not proof](concept.md#discovery-is-not-proof) | [Symbol identity](architecture.md#symbol-identity) |
| FR-019–FR-024, FR-077 | [Product statement](concept.md#product-statement), [CLI experience](concept.md#cli-experience) | [PoC behavior](poc.md#required-behavior) |
| FR-025–FR-029, FR-077 | [Product statement](concept.md#product-statement), [Initial product](concept.md#initial-product) | [Callees](architecture.md#callees) |
| FR-030–FR-037, FR-077 | [Product statement](concept.md#product-statement), [CLI experience](concept.md#cli-experience) | [Path](architecture.md#path), [PoC CLI](poc.md#minimum-cli) |
| FR-038–FR-045 | [Discovery is not proof](concept.md#discovery-is-not-proof), [Positioning](concept.md#positioning) | [Semantic verification](architecture.md#semantic-verification) |
| FR-046–FR-047, FR-049–FR-054, FR-076, FR-079 | [Uncertainty](concept.md#uncertainty-is-part-of-the-result) | [Core model](architecture.md#core-model) |
| FR-055–FR-058 | [Initial product](concept.md#initial-product), [Explainability](concept.md#explainability-creates-trust) | [PoC output](poc.md#output-contract) |
| FR-059–FR-064 | [Explainability](concept.md#explainability-creates-trust), [CLI experience](concept.md#cli-experience) | [Explain](architecture.md#explain) |
| FR-065–FR-075, FR-080–FR-081 | [Initial product](concept.md#initial-product), [CLI experience](concept.md#cli-experience) | [PoC CLI](poc.md#minimum-cli), [PoC output](poc.md#output-contract) |
| NFR-001–NFR-008 | [Query first](concept.md#query-first), [Positioning](concept.md#positioning) | [Performance invariants](architecture.md#performance-invariants), [PoC acceptance](poc.md#acceptance-criteria) |
| NFR-009–NFR-014 | [Discovery is not proof](concept.md#discovery-is-not-proof), [Uncertainty](concept.md#uncertainty-is-part-of-the-result) | [Symbol identity](architecture.md#symbol-identity) |
| NFR-015–NFR-021 | [Local by default](concept.md#local-by-default) | [Cache and security](architecture.md#cache-and-security-boundary) |
| NFR-022–NFR-026 | [Positioning](concept.md#positioning), [Initial product](concept.md#initial-product) | [PoC acceptance](poc.md#acceptance-criteria) |
| AC-001–AC-015 | [Product statement](concept.md#product-statement), [Initial product](concept.md#initial-product) | [PoC acceptance](poc.md#acceptance-criteria) |
| AC-016 | [CLI experience](concept.md#cli-experience) | [Path engine](architecture.md#path-engine) |
| AC-017 | [Initial product](concept.md#initial-product) | [Semantic verification](architecture.md#semantic-verification) |
| AC-018 | [Uncertainty](concept.md#uncertainty-is-part-of-the-result), [CLI experience](concept.md#cli-experience) | [PoC acceptance](poc.md#acceptance-criteria) |

## 9. TBD register

| TBD | Required evidence | Resolution point |
| --- | --- | --- |
| End-to-end latency | Versioned corpus and reference machine; repeated cold and warm runs; discovery, verification, total, median, and tail measurements. | Before latency is a release gate. |
| Peak memory and local storage | Peak resident memory and maximum cache or temporary bytes on the same setup. | Before resource usage is a release gate. |
| Maximum project size | Scale runs varying files, TUs, and candidates until correctness or approved resource targets fail. | Before publishing a size claim. |
| Supported hosts | Repeatable build and functional conformance results for every OS, architecture, and Clang range claimed. | Before claiming platform support. |

## 10. Source ambiguities and resolutions

| ID | Ambiguity or contradiction | Resolution |
| --- | --- | --- |
| RES-001 | The concept includes callers, callees, path, and explain, while the PoC requires only callers and path and defers the other two. | The concept governs product scope: all four remain requirements. The PoC is a narrower milestone and may defer callees and explain. |
| RES-002 | “Skip the whole graph” could be read as forbidding any broad scan, while the concept permits broad inexpensive discovery. | Broad syntactic inspection is permitted when needed. A complete project-wide semantic call graph is prohibited as query prerequisite. |
| RES-003 | The concept is on-demand, while supporting documents describe a discovery index. | An index is optional source-derived state, not a required feature or semantic prerequisite. |
| RES-004 | An earlier concept draft defined `PROBABLE` without an operational threshold. | The initial product uses only `CONFIRMED`, `POSSIBLE`, and `UNRESOLVED`; `PROBABLE` is excluded rather than assigned a heuristic rule. |
| RES-005 | The concept lists kind and confidence separately, while an example combines “POSSIBLE / virtual.” | Kind and confidence are independent fields even if displayed together. |
| RES-006 | The concept offers verified-only filtering but does not state the default treatment of uncertain edges. | Default results retain qualified uncertainty; verified-only excludes all non-confirmed edges. |
| RES-007 | Architecture suggests shortest-edge path search, but the concept requires no ranking rule. | The SRS requires at least one valid bounded path and mandates no selection algorithm or shortest-path guarantee. |
| RES-008 | Architecture specifies TU batching, while an SRS should avoid internal design. | The testable behavior is one semantic interpretation per TU build context within a query and no unrelated project-wide verification; no scheduler design is prescribed. |
| RES-009 | PoC lists output fields, but concept examples are not a frozen format. | Required information is fixed; tree versus linear layout and exact wording are not. |
| RES-010 | No source gives validated latency, memory, scale, or platform numbers. | Values remain `TBD`; NFR-003–NFR-006 and NFR-024–NFR-026 define evidence for setting them. |
| RES-011 | The concept names source root and compilation database as inputs, while CLI examples omit how those inputs are supplied. | The CLI must accept both inputs, but this SRS does not freeze flags, defaults, environment variables, or discovery conventions. |
| RES-012 | The concept explicitly assigns Tree-sitter to discovery and Clang to verification, while an SRS normally avoids implementation choices. | Their selection is retained only as the concept-mandated technology constraint CON-008. Functional requirements remain stated as observable behavior and do not prescribe internal integration design. |
| RES-013 | The source documents do not define recursive-query behavior when maximum depth is omitted. | Omission means no logical depth limit; cycle detection still bounds repeated traversal. |
| RES-014 | The source documents do not define how to select among multiple compilation contexts for one source file. | Analyze all applicable contexts, merge equivalent results, and retain context provenance rather than choosing one. |
| RES-015 | The source documents do not clearly separate unresolved semantic results from failed analysis work. | `UNRESOLVED` is a successful analysis result; failure of required work produces a partial or failed analysis result instead. |
| RES-016 | The source documents distinguish depth-limited output but do not define its process status. | Reaching a user-supplied depth limit is a successful truncated result with exit status zero. |

## 11. SRS quality review

| Review area | Result |
| --- | --- |
| Testability | Every normative requirement has a stable ID and explicit verification evidence. Quantitative unknowns are isolated in the TBD register with measurement procedures. |
| Implementation independence | Requirements specify observable inputs, behavior, outputs, limits, and qualities. Tree-sitter and Clang appear only as concept-mandated constraints; no internal modules, APIs, schemas, traversal algorithm, or implementation sequence are required. |
| Missing requirements | Coverage includes every requested functional and non-functional category plus input failures, ambiguity, cycles, depth exhaustion, stale results, confidence semantics, locations, and CLI outcomes. |
| Scope expansion | GUI, services, machine APIs, persistence, polyglot support, complete graphs, data flow, runtime analysis, and cloud features remain non-goals. |
| Concept consistency | Section 10 resolves source tensions with concept precedence. No concept capability is removed; PoC-only deferrals are labeled as milestone decisions. |
