# PADAGONIA Integration Roadmap

Follow `/home/sal/padagonia/docs/enterprise-integration-directives.md`.

## Modules

- `code_entity_extractor`: assign stable IDs to modules, symbols, dependencies,
  cycles, and repository revisions.
- `finding_writer`: persist entropy, cohesion, duplication, cycle, and repair
  findings with tool/version and commit provenance.
- `regression_reader`: compare current findings with historical graph facts and
  expose source timestamps and confidence semantics.
- `repair_lineage`: link proposed changes, applied patches, tests, and results.
- `evidence_export`: emit deterministic reports without embedding full source.

## Acceptance gates

Rescans preserve entity identity, duplicate findings are suppressed, repairs are
replayable, and Padagonia outages do not block local analysis.
