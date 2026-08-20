# CallJet C++

Fast, on-demand static call path analysis for C++.

> Find the path. Skip the whole graph.

CallJet answers focused call-path questions without first building a complete
semantic call graph for the entire codebase. Tree-sitter discovers candidate
calls quickly; Clang verifies only the translation units needed by the query.

The project is currently documentation-first and preparing for its initial
proof of concept.

## Documentation

These documents are the source of truth for the initial design. Product
behavior, analysis semantics, CLI output, confidence rules, and scope changes
must update the relevant document before or with the implementation.

1. [Product concept](docs/concept.md) — problem, product boundaries, and principles.
2. [Software Requirements Specification](docs/srs.md) — normative product requirements and acceptance criteria.
3. [Software Design Specification](docs/sds.md) — implementation design and SRS traceability.
4. [Architecture](docs/architecture.md) — query pipeline, components, and data model.
5. [PoC scope](docs/poc.md) — first deliverable and acceptance criteria.

## Initial scope

- C and C++ source
- Local CLI
- `compile_commands.json` input
- Caller, callee, path, and edge-explanation queries
- Explicit confidence for results that cannot be proven
- No network, telemetry, cloud service, or source-code upload
