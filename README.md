# KnowledgePilot

KnowledgePilot is a lightweight embedded RAG service for traditional and optional Agentic retrieval-augmented question answering. It runs as one Rust binary with SQLite persistence, configurable embeddings, citation-backed answers, and a small built-in web UI.

The project keeps the production path embedded first: no external vector database, message queue, object store, or web asset build step is required.

Human-facing project documentation is maintained in this `README.md`. Agent-facing implementation guidance and repository rules are maintained in `AGENTS.md`.

## Features

- Embedded SQLite storage for documents, chunks, embeddings, and query history.
- Deterministic hash embeddings for local development and repeatable tests.
- Optional Ollama embedding models for stronger local semantic retrieval.
- SQLite-backed in-process vector retrieval behind a `Retriever` trait.
- Extractive answer generator behind a `Generator` trait.
- Optional OpenAI-compatible chat completion generator.
- Citation-backed RAG responses.
- Answer confidence with explicit low-confidence insufficient-context fallback.
- Clickable references that open original documents and highlight cited chunks.
- Protected query history API for lightweight audit review.
- Permission-scoped retrieval with `public` as the default document scope.
- Document management APIs for create, list, detail, chunks, and delete.
- Enterprise platform assistant bot coordinating RAG documentation Q&A and live database SQL skills.
- Predefined parameterized SQL skills with MySQL and SQLite connection pools and strict read-only safety.
- Text file upload for `.md`, `.markdown`, and `.txt`.
- Local ingest command for files and directories.
- Separate upload and Q&A pages at `/upload` and `/qa`.
- Optional built-in UI that can be disabled for API-only deployments.
- Docker, Docker Compose, systemd, and GitHub Actions CI assets.

## Stack

```text
Rust 2024
Axum
SQLite / rusqlite & MySQL / mysql
serde / serde_json
log / env_logger
Vanilla HTML/CSS/JS
```

The planned file-database vector upgrade path is `sqlite-vec`. The current implementation keeps vector search in process so the service can be deployed and tested without native SQLite extension loading.

## Quick Start

```bash
cargo test
cargo run
```

### Recommended: Local Ollama Setup

For the best experience, install [Ollama](https://ollama.com) and pull both an embedding model (for retrieval) and a chat model (for answer generation):

```bash
ollama pull qwen3-embedding:0.6b
ollama pull qwen2.5:1.5b

KNOWLEDGE_PILOT_EMBEDDING_PROVIDER=ollama \
KNOWLEDGE_PILOT_EMBEDDING_MODEL=qwen3-embedding:0.6b \
KNOWLEDGE_PILOT_LLM_PROVIDER=custom \
KNOWLEDGE_PILOT_LLM_BASE_URL=http://127.0.0.1:11434/v1 \
KNOWLEDGE_PILOT_LLM_MODEL=qwen2.5:1.5b \
KNOWLEDGE_PILOT_DB_PATH=./data/knowledge-pilot.db \
cargo run -- ingest eval/documents
```

Without a chat model, KnowledgePilot still works with extractive answers pulled directly from retrieved document chunks.

Open:

- Upload page: <http://127.0.0.1:8080/upload>
- Q&A page: <http://127.0.0.1:8080/qa>
- Health check: <http://127.0.0.1:8080/health>

## API Example

```bash
curl -s http://127.0.0.1:8080/health

curl -s -X POST http://127.0.0.1:8080/documents \
  -H 'content-type: application/json' \
  -d '{"title":"RAG roadmap","text":"KnowledgePilot starts with traditional RAG and keeps interfaces ready for Agentic RAG.","source":"demo","access_scope":"public"}'

curl -s http://127.0.0.1:8080/documents

curl -s -X POST http://127.0.0.1:8080/documents/upload \
  -F 'file=@README.md'

curl -s -X POST http://127.0.0.1:8080/rag/query \
  -H 'content-type: application/json' \
  -d '{"question":"How does KnowledgePilot evolve?","top_k":3,"access_scopes":["public"]}'

curl -s http://127.0.0.1:8080/rag/history?limit=20
```

## API Reference

Successful responses use a shared JSON envelope:

```json
{
  "code": "ok",
  "message": "ok",
  "data": {}
}
```

When `KNOWLEDGE_PILOT_API_TOKEN` is configured, document and RAG endpoints require `Authorization: Bearer <token>`.

Endpoints:

```http
GET /health
GET /health/detail
GET /documents
POST /documents
POST /documents/upload
GET /documents/{id}
PUT /documents/{id}
GET /documents/{id}/chunks
DELETE /documents/{id}
POST /rag/query
POST /rag/query/stream
GET /rag/history
POST /bot/chat
POST /bot/chat/stream
GET /skills
POST /skills/test
GET /upload
GET /qa
```

Create or replace a document:

```bash
curl -s -X POST http://127.0.0.1:8080/documents \
  -H 'content-type: application/json' \
  -d '{
    "id": "optional-stable-id",
    "title": "RAG roadmap",
    "source": "README.md",
    "access_scope": "public",
    "text": "KnowledgePilot starts with traditional RAG..."
  }'
```

Upload a UTF-8 text file:

```bash
curl -s -X POST http://127.0.0.1:8080/documents/upload \
  -F 'file=@README.md' \
  -F 'source=README.md' \
  -F 'access_scope=public'
```

Query RAG:

```bash
curl -s -X POST http://127.0.0.1:8080/rag/query \
  -H 'content-type: application/json' \
  -d '{"question":"How does KnowledgePilot evolve?","top_k":3,"access_scopes":["team-a"]}'
```

Documents default to the `public` access scope. Queries always include `public` and may add one or more extra scopes with `access_scope` or `access_scopes`. Restricted documents are retrieved only when their scope is included in the query request.

List recent RAG query history:

```bash
curl -s 'http://127.0.0.1:8080/rag/history?limit=20'
```

`limit` defaults to `20` and is clamped to `1..100`. Query history returns the recorded question, generated answer, citations, confidence, and creation time. It is protected by the same bearer token as document and RAG APIs.

RAG citations include readable source metadata:

```json
{
  "document_id": "doc-1",
  "chunk_id": "doc-1:0",
  "document_title": "RAG roadmap",
  "section": "Architecture",
  "source": "README.md",
  "access_scope": "public",
  "score": 0.75
}
```

RAG query responses also include answer confidence:

```json
{
  "confidence": {
    "level": "high",
    "score": 0.82,
    "reason": "strong_retrieval_support"
  }
}
```

When retrieved context is weak, KnowledgePilot returns a low-confidence answer instead of inventing unsupported details. Chinese questions receive a Chinese insufficient-context response, and returned citations remain available for manual checking.

Errors use the same `code` and `message` shape:

```json
{
  "code": "not_found",
  "message": "document not found"
}
```

## Enterprise Platform Assistant Bot & SQL Skills

KnowledgePilot includes a unified intelligent assistant bot that blends **Platform Knowledge Base (RAG)** and **Live Enterprise Data Queries (Predefined SQL Skills)** in a single conversation flow (similar to intelligent customer service bots):

- **Platform how-tos, manuals, policies, troubleshooting**: seamlessly handled by the embedded RAG engine.
- **Real-time business data (user accounts, service metrics, alerts, orders)**: routed to predefined, parameterized SQL skills querying MySQL or SQLite.
- **Safety**: SQL queries are strictly validated to be read-only (`SELECT`/`WITH`/`EXPLAIN`). All mutation statements (`INSERT`, `UPDATE`, `DELETE`, `DROP`, `ALTER`, etc.) and multi-statement execution are rejected. Limit clauses are automatically bounded.

### Database Credentials Configuration

Connection strings are configured via environment variables matching `KP_DB_<CONNECTION_REF>`:

```bash
# MySQL for enterprise production
export KP_DB_PRIMARY="mysql://user:password@127.0.0.1:3306/platform_db"
export KP_DB_ANALYTICS="mysql://user:password@127.0.0.1:3306/analytics_db"

# SQLite for local development or testing
export KP_DB_LOCAL="sqlite://./data/business.db"
```

### Defining Skills (`config/skills.json`)

Skills are defined in JSON (default path `./config/skills.json` or configured via `KNOWLEDGE_PILOT_SKILLS_PATH`). See [`config/skills.example.json`](config/skills.example.json) for a complete template:

```json
[
  {
    "name": "query_user_info",
    "description": "查询企业平台用户基本信息、账户状态与部门",
    "connection_ref": "PRIMARY",
    "sql_template": "SELECT id, username, email, department, status FROM users WHERE id = :user_id OR username = :username LIMIT 1",
    "parameters": [
      {
        "name": "user_id",
        "param_type": "integer",
        "description": "用户数字 ID",
        "required": false
      },
      {
        "name": "username",
        "param_type": "string",
        "description": "用户登录名",
        "required": false
      }
    ],
    "keywords": ["用户", "账号", "user"]
  }
]
```

### Bot API Examples

Chat with the bot (JSON or streaming):

```bash
# Ask how to use the platform -> Bot routes to RAG
curl -s -X POST http://127.0.0.1:8080/bot/chat \
  -H 'content-type: application/json' \
  -d '{"message": "如何配置平台的告警规则？"}'

# Query live data -> Bot extracts parameters and executes SQL skill
curl -s -X POST http://127.0.0.1:8080/bot/chat \
  -H 'content-type: application/json' \
  -d '{"message": "查询用户 1001 的基本信息"}'

# SSE real-time streaming
curl -s -N -X POST http://127.0.0.1:8080/bot/chat/stream \
  -H 'content-type: application/json' \
  -d '{"message": "查询服务 auth-service 最近的指标"}'
```

List and test skills directly:

```bash
# List available skills
curl -s http://127.0.0.1:8080/skills

# Test skill execution
curl -s -X POST http://127.0.0.1:8080/skills/test \
  -H 'content-type: application/json' \
  -d '{"skill_name": "query_user_info", "parameters": {"user_id": 1001}}'
```

## Configuration

```text
KNOWLEDGE_PILOT_ADDR=127.0.0.1:8080
KNOWLEDGE_PILOT_DB_PATH=./data/knowledge-pilot.db
KNOWLEDGE_PILOT_API_TOKEN=
KNOWLEDGE_PILOT_SKILLS_PATH=./config/skills.json
KP_DB_<REF>=mysql://user:pass@host:port/dbname
KNOWLEDGE_PILOT_REQUEST_BODY_LIMIT_BYTES=2097152
KNOWLEDGE_PILOT_CHUNK_SIZE=600
KNOWLEDGE_PILOT_CHUNK_OVERLAP=100
KNOWLEDGE_PILOT_EMBEDDING_PROVIDER=hash
KNOWLEDGE_PILOT_EMBEDDING_BASE_URL=http://127.0.0.1:11434
KNOWLEDGE_PILOT_EMBEDDING_PATH=/api/embed
KNOWLEDGE_PILOT_EMBEDDING_MODEL=hash-256
KNOWLEDGE_PILOT_EMBEDDING_ALLOW_UNVERIFIED_MODEL=false
KNOWLEDGE_PILOT_RAG_MODE=auto
KNOWLEDGE_PILOT_LLM_PROVIDER=extractive
KNOWLEDGE_PILOT_UI_ENABLED=true
KNOWLEDGE_PILOT_RATE_LIMIT_RPM=300
RUST_LOG=info
```

Use `.env.example` as a local reference. The application reads environment variables directly; it does not require a `.env` loader. Deterministic local tests can still set `KNOWLEDGE_PILOT_EMBEDDING_PROVIDER=hash`; production-like retrieval should use an embedding model such as `qwen3-embedding:0.6b` through Ollama.

Embedding and answer generation are configured separately. `KNOWLEDGE_PILOT_EMBEDDING_*` controls how documents and questions are vectorized for retrieval; it must point to an embedding model, not a general chat model. When Ollama embedding is enabled, KnowledgePilot rejects common chat-model names such as `qwen3:8b` at startup. Use `KNOWLEDGE_PILOT_EMBEDDING_ALLOW_UNVERIFIED_MODEL=true` only for a custom embedding model that you have already verified through the embedding endpoint. `KNOWLEDGE_PILOT_LLM_*` controls optional answer generation and Agentic planning. Without a usable local or remote generator, KnowledgePilot still runs traditional RAG with extractive answers.

For local semantic retrieval through Ollama:

```bash
ollama pull qwen3-embedding:0.6b

KNOWLEDGE_PILOT_EMBEDDING_PROVIDER=ollama \
KNOWLEDGE_PILOT_EMBEDDING_MODEL=qwen3-embedding:0.6b \
KNOWLEDGE_PILOT_DB_PATH=/tmp/knowledge-pilot-ollama.db \
cargo run -- ingest eval/documents
```

Switching embedding providers or models requires re-ingesting documents so stored vectors match the active model. KnowledgePilot filters retrieval by embedding model name and vector dimension to avoid mixing incompatible embeddings.

For an optional local chat generator through Ollama's OpenAI-compatible endpoint, set `KNOWLEDGE_PILOT_LLM_PROVIDER=custom`, `KNOWLEDGE_PILOT_LLM_BASE_URL=http://127.0.0.1:11434/v1`, `KNOWLEDGE_PILOT_LLM_CHAT_COMPLETIONS_PATH=/chat/completions`, and `KNOWLEDGE_PILOT_LLM_MODEL` to a locally installed chat model. This is separate from `KNOWLEDGE_PILOT_EMBEDDING_MODEL`.

Set `KNOWLEDGE_PILOT_LLM_PROVIDER=openai-compatible` and provide `KNOWLEDGE_PILOT_LLM_API_KEY` to use an OpenAI-compatible `/chat/completions` provider. Legacy `KNOWLEDGE_PILOT_OPENAI_BASE_URL`, `KNOWLEDGE_PILOT_OPENAI_MODEL`, and `KNOWLEDGE_PILOT_OPENAI_API_KEY` names are still supported.

For a custom chat-completions-compatible provider, set `KNOWLEDGE_PILOT_LLM_PROVIDER=custom`, `KNOWLEDGE_PILOT_LLM_BASE_URL`, `KNOWLEDGE_PILOT_LLM_CHAT_COMPLETIONS_PATH`, and `KNOWLEDGE_PILOT_LLM_MODEL`. `KNOWLEDGE_PILOT_LLM_API_KEY` is optional for `custom`, so local providers that do not require authentication can be used. Use `KNOWLEDGE_PILOT_LLM_AUTH_HEADER`, `KNOWLEDGE_PILOT_LLM_AUTH_SCHEME`, and `KNOWLEDGE_PILOT_LLM_HEADERS_JSON` when a provider needs custom authentication or extra headers.

For Claude, set `KNOWLEDGE_PILOT_LLM_PROVIDER=claude` and provide `KNOWLEDGE_PILOT_CLAUDE_API_KEY` or `ANTHROPIC_API_KEY`. The Claude provider uses Anthropic Messages format: `POST /v1/messages`, `x-api-key`, `anthropic-version`, top-level `system`, and `messages`. Override `KNOWLEDGE_PILOT_CLAUDE_BASE_URL`, `KNOWLEDGE_PILOT_CLAUDE_MESSAGES_PATH`, `KNOWLEDGE_PILOT_CLAUDE_MODEL`, `KNOWLEDGE_PILOT_CLAUDE_ANTHROPIC_VERSION`, and `KNOWLEDGE_PILOT_CLAUDE_MAX_TOKENS` as needed.

With `KNOWLEDGE_PILOT_RAG_MODE=auto`, a configured LLM enables Agentic RAG planning automatically; without a usable LLM, the service keeps using traditional extractive RAG. Set `KNOWLEDGE_PILOT_RAG_MODE=traditional` to force the classic pipeline, or `agentic` to request Agentic RAG while still falling back to traditional RAG when no LLM is available.

Agentic RAG uses two bounded retrieval stages. It first plans search queries from the original question, then inspects the retrieved evidence and issues follow-up searches for referenced concepts, sections, figures, tables, policies, or named items that need expansion.

Set `KNOWLEDGE_PILOT_API_TOKEN` in production to require `Authorization: Bearer <token>` for document and RAG API calls. The built-in UI sends `localStorage.kp_api_token` as the bearer token when it is present.

## Production Deployment

Docker Compose:

```bash
docker compose up --build -d
curl -s http://127.0.0.1:8080/health
```

Preload local Markdown or text documents:

```bash
KNOWLEDGE_PILOT_DB_PATH=/var/lib/knowledge-pilot/data/knowledge-pilot.db \
  knowledge-pilot ingest /srv/knowledge-base
```

Systemd install example:

```bash
cargo build --release --locked
sudo install -m 0755 target/release/knowledge-pilot /usr/local/bin/knowledge-pilot
sudo useradd --system --home-dir /var/lib/knowledge-pilot --create-home knowledge-pilot
sudo mkdir -p /var/lib/knowledge-pilot/data
sudo chown -R knowledge-pilot:knowledge-pilot /var/lib/knowledge-pilot
sudo install -m 0644 deploy/systemd/knowledge-pilot.service /etc/systemd/system/knowledge-pilot.service
sudo systemctl daemon-reload
sudo systemctl enable --now knowledge-pilot
```

For production exposure, run KnowledgePilot behind a reverse proxy that provides TLS and authentication. Keep `KNOWLEDGE_PILOT_DB_PATH` on persistent storage and include it in backups.

Back up the SQLite database and its WAL files from the configured data directory. For consistent backups on a running service, prefer SQLite backup tooling or briefly stop the service before copying the database files.

## Local Ingest

```bash
KNOWLEDGE_PILOT_DB_PATH=./data/knowledge-pilot.db cargo run -- ingest ./README.md ./eval/documents
```

The ingest command recursively imports `.md`, `.markdown`, and `.txt` files into the embedded SQLite database using stable file-path based document IDs. Markdown headings are preserved as chunk section metadata, and citations return both document title and section so the UI can show human-readable source locations. In the Q&A page, clicking a reference opens the original document and highlights the cited chunk when it can be matched in the source text.

## Architecture

```text
document -> chunking -> embedding -> SQLite persistence
question -> optional agent planning -> retrieval -> context -> answer -> citations
```

Module layout:

```text
src/
  app.rs               # dependency wiring for the RAG service
  main.rs              # server entrypoint
  api.rs               # Axum routes and HTTP extraction
  commands.rs          # eval and ingest CLI commands
  config.rs            # environment-backed configuration
  contracts.rs         # API request/response DTOs
  db.rs                # SQLite connection setup
  embedding.rs         # Embedder trait, hash embedder, and Ollama embedder
  generation.rs        # Generator trait and extractive generator
  http/                # shared response envelope and error codes
  ingestion.rs         # document chunking
  models.rs            # shared serializable contracts
  rag.rs               # traditional RAG orchestration
  retrieval.rs         # Retriever trait and SQLite-backed implementation
  store.rs             # SQLite schema and persistence
  ui.rs                # embedded static UI serving
```

Important design details:

- `RagService` owns orchestration only: optional query planning, retrieval, context expansion, generation, citation recording.
- `Embedder`, `Retriever`, `Generator`, and `QueryPlanner` keep provider details out of the core pipeline.
- Embeddings are stored with model name and vector dimension; retrieval filters on both, so hash and Ollama vectors are never mixed.
- Retrieval filters chunks by document access scope. Empty internal scope sets fail closed; API queries include `public` by default.
- Markdown headings are preserved as chunk section metadata. Citations return document title, section, source path, chunk id, and score. The UI uses chunk ids to load source chunks and highlight cited passages in the original document modal.
- RAG answers are stored in query history with citation JSON and confidence fields. `GET /rag/history` exposes recent records for lightweight audit and troubleshooting.
- Low-confidence answers use an insufficient-context fallback so unsupported questions are visible during testing and operations.
- Agentic RAG runs bounded planning and follow-up retrieval when a usable LLM planner is configured. Without a usable LLM, the service remains traditional RAG.
- Static UI assets are embedded into the Rust binary, so UI changes require rebuilding before verification.

## Verification

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
KNOWLEDGE_PILOT_EMBEDDING_PROVIDER=hash cargo run -- eval
KNOWLEDGE_PILOT_EMBEDDING_PROVIDER=ollama \
  KNOWLEDGE_PILOT_EMBEDDING_MODEL=qwen3-embedding:0.6b \
  cargo run -- eval
./scripts/smoke.sh
```

`cargo run -- eval` imports `eval/documents` and runs `eval/cases.json`. The current fixture covers 91 Chinese and English Markdown documents with 184 answer-quality cases, including 17 cross-domain fixtures under `eval/documents/diverse/`. Each case checks that retrieval cites the expected source, avoids unexpected citation sources by default, and, when configured, that the generated answer contains required business key points or the expected confidence level. This keeps regressions visible for retrieval quality, answer completeness, source cleanliness, low-confidence behavior, and formatting.

## Roadmap

1. Add sqlite-vec as an optional embedded retriever.
2. Add richer offline RAG quality metrics.
3. Add metadata filters beyond access scope.
4. Add admin-facing corpus diagnostics and ingestion quality reports.

## License

MIT
