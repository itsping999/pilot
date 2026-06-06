# KnowledgePilot Agent Instructions

## Project Purpose

KnowledgePilot is a single-binary Rust 2024 embedded RAG service. It keeps the production path local and lightweight: Axum HTTP server, SQLite persistence through `rusqlite`, configurable embeddings, optional LLM generation/planning, citation-backed answers, query history, and embedded vanilla HTML/CSS/JS UI. Do not introduce external vector databases, queues, asset build steps, tracing stacks, or service dependencies unless the task explicitly asks for that tradeoff.

## Project Map

- `src/main.rs` - binary entrypoint, `env_logger` init, CLI dispatch, server bootstrap, graceful shutdown.
- `src/app.rs` - wires `SqliteStore`, `Embedder`, `Retriever`, `Generator`, and optional `QueryPlanner` into `RagService`; validates Ollama embedding model names.
- `src/api.rs` - Axum routes, HTTP extraction, auth enforcement, blocking task boundary, SSE stream response.
- `src/middleware.rs` - CORS, per-client token-bucket rate limiting, and `X-Request-Id` propagation.
- `src/config.rs` - all environment-backed runtime configuration. Keep deployment defaults and docs aligned with this file.
- `src/constants.rs` - centralized constants and limits; some are future-facing and intentionally allowed as dead code.
- `src/contracts.rs` - HTTP request/response DTOs.
- `src/http/` - shared success envelope and stable API error codes.
- `src/db.rs` - SQLite opening pragmas: WAL, foreign keys, busy timeout, synchronous NORMAL.
- `src/store.rs` - SQLite schema, migrations-by-column-addition, persistence, query history serialization.
- `src/models.rs` - shared serializable domain structs plus access-scope helpers.
- `src/ingestion.rs` - Markdown-title extraction, chunking, stable chunk ids, section metadata.
- `src/embedding.rs` - `Embedder` trait, deterministic hash embedder, Ollama embedder, vector encoding.
- `src/retrieval.rs` - `Retriever` trait and current SQLite-backed in-process vector scan with lexical/title boosts.
- `src/generation.rs` - `Generator` trait, extractive generator, OpenAI-compatible generator, Claude generator.
- `src/agent.rs` - optional Agentic RAG query planning and follow-up planning.
- `src/rag.rs` - pipeline orchestration, context expansion, confidence, citation filtering, history recording.
- `src/ui.rs` and `static/` - embedded UI pages and assets; changes require rebuilding the Rust binary.
- `eval/documents/` and `eval/cases.json` - quality gate corpus and cases, not human-facing project docs.
- `test-docs/` - auxiliary large-scale/local testing materials, not the main eval gate.

## Source Of Truth

| Concern | File/Directory | Notes |
| --- | --- | --- |
| Human-facing docs | `README.md` | Keep operator behavior here; it can drift, so verify against source before copying commands. |
| Agent-facing rules | `AGENTS.md` | Update when architecture, verification gates, deployment paths, or cross-file workflows change. |
| Runtime config | `src/config.rs`, `.env.example` | The app reads environment variables directly; no `.env` loader exists. |
| HTTP routes | `src/api.rs` | Current routes include `/health/detail`, `PUT /documents/{id}`, and `/rag/query/stream`. |
| API DTOs | `src/contracts.rs`, `src/models.rs` | Keep JSON shape changes here, then adjust UI/smoke/eval if affected. |
| Error envelope/codes | `src/http/error.rs`, `src/http/response.rs` | Successful API responses use `{code,message,data}`; errors use `{code,message}`. |
| SQLite schema | `src/store.rs` | Existing migrations are additive `ALTER TABLE ... ADD COLUMN` checks; do not use external migration tooling unless requested. |
| Provider selection | `src/app.rs`, `src/config.rs` | Provider-specific code belongs behind `Embedder`, `Retriever`, `Generator`, or `QueryPlanner`. |
| Eval gate | `eval/cases.json`, `eval/documents/` | Current fixture has 91 Markdown docs and 184 cases, including `eval/documents/diverse/`. |
| Static UI | `static/index.html`, `static/qa.html`, `static/app.css`, `static/app.js`, `src/ui.rs` | UI is embedded with `include_str!`; rebuild before browser/smoke verification. |
| Deployment | `Dockerfile`, `compose.yaml`, `deploy/systemd/knowledge-pilot.service`, `.github/workflows/ci.yml` | These files contain deploy-specific env overrides; inspect all of them when config changes. |
| Smoke verification | `scripts/smoke.sh` | Starts a local service with a bearer token and checks auth, detailed health, JSON ingest/update, chunks, multipart upload, query, SSE stream, history, delete. |

## Current API Surface

Protected by bearer auth when `KNOWLEDGE_PILOT_API_TOKEN` is set:

- `GET /documents`
- `POST /documents`
- `POST /documents/upload`
- `GET /documents/{id}`
- `PUT /documents/{id}`
- `GET /documents/{id}/chunks`
- `DELETE /documents/{id}`
- `POST /rag/query`
- `POST /rag/query/stream`
- `GET /rag/history`

Public/basic routes:

- `GET /health`
- `GET /health/detail`
- `GET /`, `GET /upload`, `GET /qa`
- `GET /static/app.css`, `GET /static/app.js`

`/rag/query/stream` currently computes the full RAG response first, then emits metadata and character tokens as SSE. Treat it as a streaming presentation endpoint, not true incremental LLM streaming.

## Runtime Configuration

Important defaults from `src/config.rs`:

```text
KNOWLEDGE_PILOT_ADDR=127.0.0.1:8080
KNOWLEDGE_PILOT_DB_PATH=./data/knowledge-pilot.db
KNOWLEDGE_PILOT_REQUEST_BODY_LIMIT_BYTES=2097152
KNOWLEDGE_PILOT_CHUNK_SIZE=600
KNOWLEDGE_PILOT_CHUNK_OVERLAP=100
KNOWLEDGE_PILOT_EMBEDDING_PROVIDER=hash
KNOWLEDGE_PILOT_EMBEDDING_BASE_URL=http://127.0.0.1:11434
KNOWLEDGE_PILOT_EMBEDDING_PATH=/api/embed
KNOWLEDGE_PILOT_EMBEDDING_MODEL=hash-256 or qwen3-embedding:0.6b
KNOWLEDGE_PILOT_RAG_MODE=auto
KNOWLEDGE_PILOT_LLM_PROVIDER=extractive
KNOWLEDGE_PILOT_UI_ENABLED=true
KNOWLEDGE_PILOT_RATE_LIMIT_RPM=300
RUST_LOG=info
```

Embedding and answer generation are separate. `KNOWLEDGE_PILOT_EMBEDDING_MODEL` must be an embedding model; chat/generation models belong under `KNOWLEDGE_PILOT_LLM_MODEL`. `src/app.rs` rejects common Ollama chat model names for embeddings unless `KNOWLEDGE_PILOT_EMBEDDING_ALLOW_UNVERIFIED_MODEL=true` is explicitly set.

Changing embedding provider/model requires re-ingesting documents into a database for that model. Retrieval filters by embedding model name and vector dimension, so mismatched stored vectors are ignored.

## Commands

| Task | Command |
| --- | --- |
| Format check | `cargo fmt --check` |
| Lint | `cargo clippy --all-targets -- -D warnings` |
| Unit tests | `cargo test` or CI-style `cargo test --locked` |
| Build | `cargo build` |
| Release build | `cargo build --release --locked` |
| Hash eval | `KNOWLEDGE_PILOT_DB_PATH=/tmp/knowledge-pilot-eval-hash.db KNOWLEDGE_PILOT_EMBEDDING_PROVIDER=hash cargo run -- eval` |
| Ollama eval | `KNOWLEDGE_PILOT_DB_PATH=/tmp/knowledge-pilot-eval-ollama.db KNOWLEDGE_PILOT_EMBEDDING_PROVIDER=ollama KNOWLEDGE_PILOT_EMBEDDING_MODEL=qwen3-embedding:0.6b cargo run -- eval` |
| Ingest files | `KNOWLEDGE_PILOT_DB_PATH=/tmp/knowledge-pilot-ingest.db cargo run -- ingest README.md eval/documents` |
| Smoke test | `./scripts/smoke.sh` |
| Docker Compose | `docker compose up --build -d` |

Ollama eval requires a running Ollama service and the `qwen3-embedding:0.6b` model. If unavailable, say so explicitly and still run hash eval.

## Common Workflows

### Add Or Change An API Endpoint

1. Update route wiring and handler logic in `src/api.rs`.
2. Add or adjust DTOs in `src/contracts.rs` or shared structs in `src/models.rs`.
3. Preserve the success/error envelopes from `src/http/`.
4. Apply bearer auth consistently for document and RAG APIs.
5. Update `static/app.js` if the UI calls the endpoint.
6. Extend `scripts/smoke.sh` for externally visible behavior.
7. Update `README.md` for operators and this file for future agents.

### Add A Runtime Knob

1. Add parsing and defaults in `src/config.rs`.
2. Update `.env.example`.
3. Update `README.md` configuration notes and any deployment defaults in `Dockerfile`, `compose.yaml`, or `deploy/systemd/knowledge-pilot.service`.
4. Add a config unit test if parsing has aliases, defaults, validation, or fallbacks.

### Add An Embedder, Retriever, Generator, Or Planner

1. Put concrete provider code behind the relevant trait: `Embedder`, `Retriever`, `Generator`, or `QueryPlanner`.
2. Wire provider selection only through `src/app.rs` and `src/config.rs`.
3. Keep `RagService` focused on orchestration, not provider-specific request formats.
4. Add deterministic unit tests or fake implementations where possible.
5. Run both hash eval and provider-specific eval when retrieval/generation behavior can change.

### Change Ingestion, Retrieval, Answers, Confidence, Or Citations

1. Update the relevant module: `src/ingestion.rs`, `src/retrieval.rs`, `src/generation.rs`, or `src/rag.rs`.
2. Add focused unit tests for edge cases.
3. Update `eval/cases.json` only when the expected behavior genuinely changes.
4. Run both evals. Unexpected citation sources fail unless `allow_additional_sources` is intentionally set.
5. Preserve low-confidence insufficient-context behavior instead of making unsupported guesses look successful.

### Add Eval Documents Or Cases

1. Put corpus files under `eval/documents/`; cross-domain fixtures currently live in `eval/documents/diverse/`.
2. Add cases to `eval/cases.json` with these fields as needed: `question`, `expected_source`, `expected_answer_contains`, `expected_confidence_level`, `allow_additional_sources`.
3. Keep eval cases robust across both hash and Ollama embeddings. Use `allow_additional_sources` for broad questions where multiple sources are acceptable.
4. Verify `cargo run -- eval` with hash and Ollama when Ollama is available.

### Change The Static UI

1. Edit `static/index.html`, `static/qa.html`, `static/app.css`, or `static/app.js`.
2. Remember assets are compiled into the Rust binary by `src/ui.rs`.
3. Run `cargo build` or `scripts/smoke.sh` before checking the UI.
4. If behavior changes, update smoke coverage and README examples.

## Conventions

- Human-facing documentation belongs in `README.md`; agent-facing implementation rules belong in `AGENTS.md`.
- Avoid adding parallel docs such as `docs/`, `openspec/`, new changelogs, or contribution/security docs unless explicitly requested. Existing summary files are historical; keep active guidance in `README.md` and `AGENTS.md`.
- Keep the embedded SQLite baseline working; sqlite-vec is only the planned file-database upgrade path.
- Keep access-scope enforcement at the store/retriever boundary. Empty internal scope sets must fail closed; API queries include `public` by default.
- Preserve chunk metadata: document title, source, nearest Markdown section, access scope, and stable chunk id.
- Store query history with citations JSON and confidence fields for audit review.
- Use `serde`/`serde_json` for JSON and `log`/`env_logger` for logging.
- Minimize dependencies. `reqwest` is blocking and called inside blocking task paths; avoid mixing async provider calls into the current pipeline without redesigning the boundary.
- Do not commit secrets, real tokens, cookies, private URLs, or private corpora.
- Treat `FINAL-PROJECT-SUMMARY.md`, `PROJECT-COMPLETE.md`, and files under `test-docs/` as historical or auxiliary materials, not the active source of truth.

## Verification

Before handing off code changes, run the smallest set that covers the blast radius. For normal production-impact changes, run:

```bash
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
KNOWLEDGE_PILOT_DB_PATH=/tmp/knowledge-pilot-eval-hash.db \
  KNOWLEDGE_PILOT_EMBEDDING_PROVIDER=hash \
  cargo run -- eval
KNOWLEDGE_PILOT_DB_PATH=/tmp/knowledge-pilot-eval-ollama.db \
  KNOWLEDGE_PILOT_EMBEDDING_PROVIDER=ollama \
  KNOWLEDGE_PILOT_EMBEDDING_MODEL=qwen3-embedding:0.6b \
  cargo run -- eval
./scripts/smoke.sh
```

CI currently runs fmt, clippy, `cargo test --locked`, hash eval through `cargo run --locked -- eval`, and `./scripts/smoke.sh`. Local full verification is stricter because it also includes the Ollama eval when the model is installed.

## Common Pitfalls

- `README.md` may lag behind source; verify route lists and defaults from `src/api.rs` and `src/config.rs` before updating `AGENTS.md`.
- `Dockerfile`, `compose.yaml`, and systemd do not all share identical defaults. When changing runtime config, check all three.
- `POST /documents` replaces existing documents when `id` conflicts because `store.rs` uses SQLite upsert plus chunk replacement.
- `PUT /documents/{id}` updates only an existing document; missing documents return `not_found`.
- Multipart upload accepts UTF-8 text only.
- `top_k` is clamped to `1..20`; query history `limit` is clamped to `1..100`.
- `KNOWLEDGE_PILOT_RATE_LIMIT_RPM` is per client key derived from `x-forwarded-for`, `x-real-ip`, or `unknown`; reverse proxy behavior affects grouping.
- Static UI auth reads `localStorage.kp_api_token`.
- The eval command recursively imports `eval/documents`; do not store unrelated Markdown project docs there.

## Git

- Use Conventional Commits, for example `feat(rag): add context builder`.
- Keep commits focused by module or behavior.
- When behavior changes, update `README.md` for user/operator impact and `AGENTS.md` for future-agent rules in the same functional slice.
