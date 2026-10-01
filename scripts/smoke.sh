#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CUSTOM_BIN="${BIN:-}"
BIN="${CUSTOM_BIN:-$ROOT_DIR/target/debug/knowledge-pilot}"
ADDR="${KNOWLEDGE_PILOT_ADDR:-127.0.0.1:18080}"
BASE_URL="http://$ADDR"
DB_PATH="${KNOWLEDGE_PILOT_DB_PATH:-/tmp/knowledge-pilot-smoke.db}"
TOKEN="${KNOWLEDGE_PILOT_API_TOKEN:-smoke-token}"
AUTH_HEADER="Authorization: Bearer $TOKEN"

cleanup() {
  if [[ -n "${SERVER_PID:-}" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill "$SERVER_PID"
    wait "$SERVER_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT

rm -f "$DB_PATH" "$DB_PATH-shm" "$DB_PATH-wal"
BIZ_DB_PATH="/tmp/kp-smoke-biz.db"
SKILLS_PATH="/tmp/kp-smoke-skills.json"
rm -f "$BIZ_DB_PATH" "$SKILLS_PATH"

if [[ -z "$CUSTOM_BIN" ]]; then
  cargo build
elif [[ ! -x "$BIN" ]]; then
  cargo build
fi

sqlite3 "$BIZ_DB_PATH" "CREATE TABLE service_metrics (service_name TEXT, qps INTEGER, status TEXT); INSERT INTO service_metrics VALUES ('user-service', 1250, 'healthy');"

cat <<'EOF' > "$SKILLS_PATH"
[
  {
    "name": "query_service_metrics",
    "description": "查询微服务的性能指标与健康状态",
    "connection_ref": "SMOKE",
    "sql_template": "SELECT service_name, qps, status FROM service_metrics WHERE service_name = :service_name",
    "parameters": [
      {
        "name": "service_name",
        "param_type": "string",
        "description": "微服务名称",
        "required": true,
        "default": "user-service"
      }
    ],
    "keywords": ["指标", "qps", "service_metrics", "metrics"]
  }
]
EOF

KNOWLEDGE_PILOT_ADDR="$ADDR" \
KNOWLEDGE_PILOT_DB_PATH="$DB_PATH" \
KNOWLEDGE_PILOT_API_TOKEN="$TOKEN" \
KNOWLEDGE_PILOT_SKILLS_PATH="$SKILLS_PATH" \
KP_DB_SMOKE="sqlite://$BIZ_DB_PATH" \
RUST_LOG=info \
"$BIN" >/tmp/knowledge-pilot-smoke.log 2>&1 &
SERVER_PID="$!"

for _ in $(seq 1 50); do
  if curl -fsS "$BASE_URL/health" >/dev/null 2>&1; then
    break
  fi
  sleep 0.1
done

curl -fsS "$BASE_URL/health" >/tmp/kp-smoke-health.json
grep -q '"code":"ok"' /tmp/kp-smoke-health.json
curl -fsS "$BASE_URL/health/detail" >/tmp/kp-smoke-health-detail.json
grep -q '"embedding_model":"hash-256"' /tmp/kp-smoke-health-detail.json
curl -fsS -o /tmp/kp-smoke-upload.html "$BASE_URL/upload"
curl -fsS -o /tmp/kp-smoke-qa.html "$BASE_URL/qa"

unauthorized_code="$(curl -s -o /tmp/kp-smoke-unauthorized.json -w '%{http_code}' "$BASE_URL/documents")"
test "$unauthorized_code" = "401"
grep -q '"code":"unauthorized"' /tmp/kp-smoke-unauthorized.json

curl -fsS \
  -H "$AUTH_HEADER" \
  -H 'content-type: application/json' \
  -d '{"id":"smoke-json","title":"Smoke JSON","source":"smoke-json","text":"KnowledgePilot smoke tests JSON document indexing."}' \
  "$BASE_URL/documents" >/tmp/kp-smoke-json-response.json
grep -q '"code":"ok"' /tmp/kp-smoke-json-response.json
grep -q '"status":"indexed"' /tmp/kp-smoke-json-response.json

curl -fsS \
  -X PUT \
  -H "$AUTH_HEADER" \
  -H 'content-type: application/json' \
  -d '{"title":"Smoke JSON Updated","source":"smoke-json","text":"KnowledgePilot smoke tests JSON document updates and chunk listing."}' \
  "$BASE_URL/documents/smoke-json" >/tmp/kp-smoke-update-response.json
grep -q '"status":"updated"' /tmp/kp-smoke-update-response.json

curl -fsS -H "$AUTH_HEADER" "$BASE_URL/documents/smoke-json/chunks" >/tmp/kp-smoke-chunks.json
grep -q '"chunks":' /tmp/kp-smoke-chunks.json

printf 'KnowledgePilot smoke tests multipart Markdown upload.\n' >/tmp/kp-smoke-upload.md
curl -fsS \
  -H "$AUTH_HEADER" \
  -F 'source=smoke-upload' \
  -F 'file=@/tmp/kp-smoke-upload.md' \
  "$BASE_URL/documents/upload" >/tmp/kp-smoke-upload-response.json
grep -q '"code":"ok"' /tmp/kp-smoke-upload-response.json

curl -fsS -H "$AUTH_HEADER" "$BASE_URL/documents" >/tmp/kp-smoke-documents.json
grep -q '"code":"ok"' /tmp/kp-smoke-documents.json
curl -fsS \
  -H "$AUTH_HEADER" \
  -H 'content-type: application/json' \
  -d '{"question":"What does KnowledgePilot smoke test?","top_k":3}' \
  "$BASE_URL/rag/query" >/tmp/kp-smoke-query.json
grep -q '"code":"ok"' /tmp/kp-smoke-query.json

curl -fsS \
  -H "$AUTH_HEADER" \
  -H 'content-type: application/json' \
  -d '{"question":"What does KnowledgePilot smoke test?","top_k":3}' \
  "$BASE_URL/rag/query/stream" >/tmp/kp-smoke-stream.txt
grep -q '"type":"metadata"' /tmp/kp-smoke-stream.txt
grep -q '"type":"token"' /tmp/kp-smoke-stream.txt
grep -q '"type":"done"' /tmp/kp-smoke-stream.txt

curl -fsS -H "$AUTH_HEADER" "$BASE_URL/rag/history?limit=1" >/tmp/kp-smoke-history.json
grep -q '"code":"ok"' /tmp/kp-smoke-history.json
grep -q '"question":"What does KnowledgePilot smoke test?' /tmp/kp-smoke-history.json

curl -fsS -X DELETE -H "$AUTH_HEADER" "$BASE_URL/documents/smoke-json" >/tmp/kp-smoke-delete.json
grep -q '"code":"ok"' /tmp/kp-smoke-delete.json

# Test Skills API
curl -fsS -H "$AUTH_HEADER" "$BASE_URL/skills" >/tmp/kp-smoke-skills-list.json
grep -q '"code":"ok"' /tmp/kp-smoke-skills-list.json
grep -q '"query_service_metrics"' /tmp/kp-smoke-skills-list.json

curl -fsS \
  -H "$AUTH_HEADER" \
  -H 'content-type: application/json' \
  -d '{"skill_name":"query_service_metrics","parameters":{"service_name":"user-service"}}' \
  "$BASE_URL/skills/test" >/tmp/kp-smoke-skill-test.json
grep -q '"code":"ok"' /tmp/kp-smoke-skill-test.json
grep -q '"service_name":"user-service"' /tmp/kp-smoke-skill-test.json
grep -q '"qps":1250' /tmp/kp-smoke-skill-test.json

# Test Bot Direct Greeting
curl -fsS \
  -H "$AUTH_HEADER" \
  -H 'content-type: application/json' \
  -d '{"message":"你好"}' \
  "$BASE_URL/bot/chat" >/tmp/kp-smoke-bot-direct.json
grep -q '"code":"ok"' /tmp/kp-smoke-bot-direct.json
grep -q '"action_taken":"direct"' /tmp/kp-smoke-bot-direct.json

# Test Bot Skill Execution
curl -fsS \
  -H "$AUTH_HEADER" \
  -H 'content-type: application/json' \
  -d '{"message":"查询服务指标"}' \
  "$BASE_URL/bot/chat" >/tmp/kp-smoke-bot-skill.json
grep -q '"code":"ok"' /tmp/kp-smoke-bot-skill.json
grep -q '"action_taken":"skill"' /tmp/kp-smoke-bot-skill.json
grep -q '"skill_used":"query_service_metrics"' /tmp/kp-smoke-bot-skill.json
grep -q 'user-service' /tmp/kp-smoke-bot-skill.json

# Test Bot Streaming
curl -fsS \
  -H "$AUTH_HEADER" \
  -H 'content-type: application/json' \
  -d '{"message":"查询服务指标"}' \
  "$BASE_URL/bot/chat/stream" >/tmp/kp-smoke-bot-stream.txt
grep -q '"action":"skill"' /tmp/kp-smoke-bot-stream.txt
grep -q '"type":"data"' /tmp/kp-smoke-bot-stream.txt
grep -q '"type":"done"' /tmp/kp-smoke-bot-stream.txt

echo "smoke_result ok"
