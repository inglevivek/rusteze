# Rusteze v1.2 Architecture Refactor Plan

This document is the implementation checkpoint reference for the `v1.2` branch. It breaks the work into stages, provides agent implementation prompts for each stage, and includes a progress tracker so work can resume cleanly if context is lost.

The plan refactors the current system into explicit stages: **Ingestion -> Extraction -> Resolver -> Query Planning -> Evidence -> Report**, and adds a first-class real-time observability layer that shows actual input/output payloads and transformations at every stage.[1][2][3]

## Target architecture

```text
Client Upload
   |
   v
[Stage 1] Ingestion Pipeline
   -> IngestedDocument { text, source_format, extraction_method, metadata }
   |
   v
[Stage 2] Extraction Pipeline
   -> CaseFacts { diagnoses, medications, procedures, labs, claim_questions, spans }
   |
   v
[Stage 3] Resolver Agent (Rig + tools)
   -> ResolvedCaseFacts { canonical concepts, IDs, confidence, provenance }
   |
   v
[Stage 4] Query Planning
   -> validated Cypher queries using BODHI schema index
   |
   v
[Stage 5] Evidence Retrieval
   -> Neo4j graph evidence + Qdrant semantic evidence
   |
   v
[Stage 6] Report Generation
   -> adjudication report

All stages emit real-time PipelineEvent records.
```

## Core implementation principles

- Keep the resolver **inside the main server** but isolate it under its own module boundary so it can be extracted later if needed.
- Replace the current linker-centric bridge flow with a standardized resolved-concept contract before Neo4j planning begins.
- Refactor the current file handling into a true **ingestion pipeline** that accepts images, PDFs, TXT, and JSON, not an OCR-only abstraction.
- Add a **Pipeline Event Bus** that captures the actual payloads entering and leaving every stage, streamed in real time to a debug surface and persisted for replay.[1][2][3][4]

## Stage breakdown

## Stage 0: Baseline instrumentation first

### Goal
Instrument the current `v1.2` flow before changing behavior so there is a baseline of how data moves today.

### Deliverables
- `src/observability/` module
- `PipelineEvent` model
- `trace_id` / `case_id` correlation through the current flow
- JSON event emission to disk or database
- WebSocket debug stream endpoint

### Changes
- Add structured tracing spans around current handlers and major functions.
- Emit `stage_enter`, `stage_output`, `stage_error`, and `stage_complete` events.
- Capture actual payload snapshots, truncated safely when too large.
- Add a debug route such as `/api/debug/stream` for live event streaming and `/api/cases/:id/pipeline-events` for replay.

### Checkpoint criteria
- A single ingestion request produces a timeline of payload events viewable in real time.
- Existing behavior is unchanged.

### Agent implementation prompt

```text
You are modifying the rusteze v1.2 branch.

Goal: add a first-class observability subsystem before changing pipeline behavior.

Implement:
1. A new src/observability/ module.
2. A PipelineEvent struct with fields:
   - trace_id
   - case_id
   - stage
   - event_type
   - timestamp
   - payload_json
   - metadata_json
   - duration_ms
3. An in-memory broadcaster for live streaming events.
4. A persistence mechanism for replay (JSONL file or DB table).
5. WebSocket route for live stream.
6. Helper functions so any stage can emit before/after/error events.

Requirements:
- Use structured tracing-compatible data.
- Payloads must contain actual transformed data, not vague status text.
- Keep behavior of current pipeline unchanged.
- Make event emission cheap and non-blocking.
- Keep module boundaries clean and reusable.

Output:
- file-by-file changes
- any schema additions
- routes added
- exact data model used
```

***

## Stage 1: Ingestion pipeline refactor

### Goal
Refactor file handling into a dedicated ingestion pipeline that accepts images, PDFs, TXT, and JSON and always returns normalized text plus metadata.

### Deliverables
- `src/ingestion/`
- extractor modules for `pdf`, `image`, `text`, `json`
- `IngestedDocument` model
- existing handlers updated to use ingestion service

### Changes
- Remove inline file-type branching from `main.rs` handlers.
- Move PDF native extraction and OCR fallback into dedicated extractor logic.
- Pass TXT through after UTF-8 validation.
- Convert JSON into flattened labeled text rather than raw JSON blobs.
- Emit ingestion events with file name, format, extractor used, output text, and metadata.

### Checkpoint criteria
- `/api/ingest` and `/api/ingest/batch` both work using the new ingestion module.
- Each input file produces observable ingestion payloads.

### Agent implementation prompt

```text
You are modifying rusteze v1.2.

Goal: replace ad hoc file text extraction with a formal ingestion pipeline.

Implement:
1. src/ingestion/mod.rs
2. src/ingestion/pipeline.rs
3. src/ingestion/extractors/{pdf,image,text,json}.rs
4. A shared IngestedDocument struct:
   - text
   - source_format
   - extraction_method
   - metadata
5. Refactor handlers so they call ingestion::pipeline::ingest_document(...)
6. Emit observability events before and after ingestion

Rules:
- Supported formats: image, pdf, txt, json
- PDF uses native extraction first, then OCR fallback
- Image uses OCR sidecar
- TXT is validated passthrough
- JSON is flattened into labeled text for downstream extraction
- Remove extraction branching from handlers
- Preserve current API behavior

Output:
- file-by-file modifications
- new module tree
- exact data contract for IngestedDocument
```

***

## Stage 2: Extraction stage cleanup

### Goal
Make extraction a pure structured-facts stage that consumes `IngestedDocument.text` and outputs `CaseFacts`, without doing resolution or adjudication.

### Deliverables
- `CaseFacts` model
- extraction service boundary
- extraction prompt isolated from adjudication/report prompts
- extraction events with raw text input and structured output

### Changes
- Separate the extraction call from downstream linking/grounding.
- Output typed fields: diagnoses, medications, procedures, labs, claim questions, optional evidence spans.
- Ensure extraction consumes ingestion output only.
- Persist extraction payload for replay and debugging.

### Checkpoint criteria
- A case can be ingested and produce `CaseFacts` without any Neo4j or resolver dependency.

### Agent implementation prompt

```text
You are modifying rusteze v1.2.

Goal: isolate a pure extraction stage.

Implement:
1. A CaseFacts struct for typed extraction output.
2. An extraction service that accepts normalized ingestion text and returns CaseFacts.
3. Prompt separation so extraction does not share adjudication logic.
4. Observability events capturing:
   - input text
   - raw model response
   - validated CaseFacts output

Requirements:
- Extraction must not resolve terms.
- Extraction must not query Neo4j or Qdrant.
- Extraction must not generate final adjudication text.
- Failures should emit validation/debug payloads visibly.

Output:
- exact struct definitions
- call site changes
- prompt separation plan
```

***

## Stage 3: Resolver agent inside main server

### Goal
Add an internal Rig-based resolver domain that standardizes extracted terms using authoritative tools and returns `ResolvedCaseFacts`.

### Deliverables
- `src/resolver/`
- Rig agent
- tool wrappers for RxNorm, ICD-11, UMLS, NRCES enrichment, optional web fallback
- resolver service entrypoint
- resolved concept persistence

### Changes
- Add `models.rs`, `tools/`, `agent.rs`, `service.rs` under `src/resolver/`.
- Implement resolution policy by term type.
- Persist `ResolvedCaseFacts` per case.
- Instrument each tool call with actual request/response payloads and timing.[5][6][7][8]

### Checkpoint criteria
- Resolver can accept `CaseFacts` and return canonical concepts with provenance.
- Tool-level observability shows real external request/response data.

### Agent implementation prompt

```text
You are modifying rusteze v1.2.

Goal: add an internal resolver domain using Rig inside the main server.

Implement:
1. src/resolver/{mod.rs,models.rs,agent.rs,service.rs}
2. src/resolver/tools/{rxnorm.rs,icd11.rs,umls.rs,nrces.rs,web_fallback.rs,mod.rs}
3. ResolvedConcept and ResolvedCaseFacts data contracts
4. Rig agent configured for term resolution only
5. Resolver service entrypoint callable from adjudication pipeline
6. Observability for:
   - incoming raw terms
   - each tool invocation
   - each tool response
   - final resolved output

Resolution policy:
- drugs: RxNorm exact -> RxNorm approximate -> UMLS -> web fallback -> NRCES enrich
- diagnoses: ICD-11 -> UMLS -> web fallback
- never invent IDs
- always emit structured output with provenance and confidence

Requirements:
- Keep resolver embedded in main server
- Keep it isolated as its own module boundary
- Return deterministic typed structs to the rest of the app
```

***

## Stage 4: Query planning stage

### Goal
Introduce a dedicated query-planning stage that consumes resolved concepts and generates validated Cypher using a static BODHI schema index.

### Deliverables
- planner module
- static BODHI schema index
- Cypher validator
- planning events with input concepts, generated queries, validation results

### Changes
- Build planner prompt using only `ResolvedCaseFacts` + schema index.
- Validate all labels and relationships before execution.
- Remove direct dependence on current linker/bridge logic.

### Checkpoint criteria
- Planner emits a small set of valid Cypher queries for a resolved case.
- Validation failures are visible with exact rejected query payloads.

### Agent implementation prompt

```text
You are modifying rusteze v1.2.

Goal: add a dedicated query-planning stage after resolution.

Implement:
1. A planner module that accepts ResolvedCaseFacts
2. A static BODHI schema index
3. A prompt that generates targeted Cypher only
4. A validator that rejects unknown labels, relationships, and unsafe query shapes
5. Observability events for planner input, raw model output, validated query set, and rejected queries

Requirements:
- planner must not do resolution
- planner must not write final reports
- planner input is only canonical resolved concepts and schema
- all generated Cypher must be auditable from logs/events
```

***

## Stage 5: Evidence retrieval stage

### Goal
Separate evidence retrieval from query planning and report generation.

### Deliverables
- graph evidence service
- semantic evidence service
- combined `EvidencePacket`
- real-time visibility into graph rows and semantic results

### Changes
- Execute validated Cypher against Neo4j.
- Query Qdrant as supporting semantic evidence where needed.
- Emit exact query strings, exact Neo4j rows returned, exact semantic hits returned.

### Checkpoint criteria
- Evidence retrieval can run independently from report generation.
- Real returned rows are visible in observability stream.

### Agent implementation prompt

```text
You are modifying rusteze v1.2.

Goal: isolate evidence retrieval after query planning.

Implement:
1. graph evidence execution service
2. semantic evidence retrieval service
3. EvidencePacket struct combining both
4. Observability events containing:
   - exact Cypher executed
   - raw Neo4j rows returned
   - exact Qdrant query used
   - returned semantic hits
   - timings and failures

Requirements:
- no hidden graph transformations
- preserve raw evidence before summarization
- evidence packet should be reusable by report and chat stages
```

***

## Stage 6: Report stage cleanup

### Goal
Make final report generation consume only `EvidencePacket` + resolved facts, not raw source documents or hidden intermediate heuristics.

### Deliverables
- report service boundary
- separated report prompt
- report observability showing exact prompt payload and final response

### Changes
- Report model should no longer depend on raw bridge logic.
- Emit prompt input snapshot, model output, and persisted report.
- Make report generation easy to replay from stored `EvidencePacket`.

### Checkpoint criteria
- A report can be regenerated from persisted evidence without rerunning ingestion or resolution.

### Agent implementation prompt

```text
You are modifying rusteze v1.2.

Goal: make report generation a pure final stage.

Implement:
1. report generation service that consumes only ResolvedCaseFacts + EvidencePacket
2. prompt separation from extraction and planning
3. observability events for prompt input, raw model output, and final stored report

Requirements:
- no direct dependence on ingestion text or old linker output
- report must be replayable from stored evidence
- keep the prompt compact and fully auditable
```

***

## Stage 7: Legacy path removal

### Goal
Remove the old linker/bridge/sweep path once the new staged pipeline is stable.

### Deliverables
- deprecated code removed or feature-flagged off
- pipeline exclusively uses new staged contracts

### Changes
- Remove or retire `src/knowledge/linker.rs` usage from core adjudication path.
- Remove keyword sweep and rescue behaviors from prompt builder once planner is live.
- Simplify `pipeline.rs` around explicit stage boundaries.

### Checkpoint criteria
- Main adjudication path no longer depends on legacy linker logic.
- Full end-to-end execution runs through staged architecture only.

### Agent implementation prompt

```text
You are modifying rusteze v1.2.

Goal: remove the legacy linker-centric flow after the new staged system is stable.

Implement:
1. removal or deactivation of old linker-first logic
2. pipeline simplification so only staged contracts remain
3. cleanup of obsolete prompt-builder sweep/rescue logic
4. observability confirmation that no legacy stage is invoked during new pipeline runs

Requirements:
- do not break chat/history/case CRUD endpoints
- keep rollback path via feature flag until final cleanup is verified
```

***

## Observability requirements

Observability is a mandatory cross-cutting concern and should be implemented consistently across all stages.

### PipelineEvent contract

```json
{
  "trace_id": "uuid",
  "case_id": "string",
  "stage": "ingestion|extraction|resolver|planner|evidence|report",
  "event_type": "enter|input|tool_call|tool_result|output|error|complete",
  "timestamp": "iso8601",
  "payload": {},
  "metadata": {
    "duration_ms": 0,
    "model": null,
    "tool": null,
    "source_format": null,
    "confidence": null
  }
}
```

### Must-show data in real time

- uploaded filename, mime/type, bytes size
- normalized text after ingestion
- exact extraction prompt input and validated `CaseFacts`
- every resolver tool request and response
- exact resolved concepts with IDs and confidence
- exact planning prompt input
- exact Cypher generated and validation result
- exact Neo4j rows and Qdrant hits
- exact report prompt payload and final report text
- errors with full stage-local context

### Debug surfaces

- `GET /api/debug/stream` -> WebSocket live event stream
- `GET /api/cases/:id/pipeline-events` -> replay endpoint
- optional internal HTML debug page showing stage timeline and expandable JSON payloads

## Checkpoint tracker

Use this section as the resume point if implementation context is lost.

| Stage | Status | Branch commit | Files touched | Notes |
|---|---|---|---|---|
| 0. Baseline observability | Not started |  |  |  |
| 1. Ingestion pipeline | Not started |  |  |  |
| 2. Extraction cleanup | Not started |  |  |  |
| 3. Resolver agent | Not started |  |  |  |
| 4. Query planning | Not started |  |  |  |
| 5. Evidence retrieval | Not started |  |  |  |
| 6. Report cleanup | Not started |  |  |  |
| 7. Legacy removal | Not started |  |  |  |

## Recommended execution order

1. Stage 0 baseline observability
2. Stage 1 ingestion pipeline
3. Stage 2 extraction cleanup
4. Stage 3 resolver agent
5. Stage 4 query planning
6. Stage 5 evidence retrieval
7. Stage 6 report cleanup
8. Stage 7 legacy removal

## Resume protocol

When resuming after context loss, do the following:

1. Open this document.
2. Find the latest completed stage in the checkpoint tracker.
3. Read the corresponding stage section and agent prompt.
4. Inspect the files listed in the latest completed checkpoint commit.
5. Continue from the next unchecked stage only.

## Architecture deltas summary

### New modules
- `src/observability/`
- `src/ingestion/`
- `src/resolver/`
- `src/planner/`
- `src/evidence/`
- optional `src/reporting/`

### Core state contracts
- `IngestedDocument`
- `CaseFacts`
- `ResolvedConcept`
- `ResolvedCaseFacts`
- `EvidencePacket`
- `PipelineEvent`

### Main server flow after refactor

```text
handle_ingest
  -> ingestion::pipeline
  -> extraction::service
  -> resolver::service
  -> planner::service
  -> evidence::service
  -> report::service
  -> persist outputs + emit pipeline events
```

### Legacy parts to retire
- inline `extract_file_text()` branching in `main.rs`
- `src/knowledge/linker.rs` as core adjudication dependency
- keyword sweep / BODHI rescue logic in prompt builder
- direct mixed bridge logic in current adjudication path