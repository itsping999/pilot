#!/bin/bash

# 启动LLM生成模式的KnowledgePilot服务

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "========================================="
echo "启动KnowledgePilot (LLM生成模式)"
echo "========================================="

# 检查Ollama服务
echo "[1/4] 检查Ollama服务..."
if ! curl -s http://127.0.0.1:11434 > /dev/null 2>&1; then
    echo "⚠️  Ollama服务未运行，正在启动..."
    ollama serve &
    sleep 3
fi
echo "✓ Ollama服务运行正常"

# 检查嵌入模型
echo ""
echo "[2/4] 检查qwen3-embedding模型..."
if ! ollama list | grep -q "qwen3-embedding:0.6b"; then
    echo "⚠️  嵌入模型未找到，正在下载..."
    ollama pull qwen3-embedding:0.6b
fi
echo "✓ qwen3-embedding模型就绪"

# 检查聊天模型
echo ""
echo "[3/4] 检查qwen2.5聊天模型..."
if ! ollama list | grep -q "qwen2.5:1.5b"; then
    echo "⚠️  聊天模型未找到，正在下载..."
    ollama pull qwen2.5:1.5b
fi
echo "✓ qwen2.5:1.5b模型就绪"

# 启动KnowledgePilot
echo ""
echo "[4/4] 启动KnowledgePilot服务..."
echo "配置："
echo "  - 嵌入模型：qwen3-embedding:0.6b"
echo "  - 聊天模型：qwen2.5:1.5b"
echo "  - 生成模式：generative（LLM生成）"
echo "  - 数据库：knowledge-pilot-ollama.db"
echo "  - 端口：8083"
echo ""

cd "$PROJECT_ROOT"
KNOWLEDGE_PILOT_EMBEDDING_PROVIDER=ollama \
KNOWLEDGE_PILOT_EMBEDDING_MODEL=qwen3-embedding:0.6b \
KNOWLEDGE_PILOT_DB_PATH=./data/knowledge-pilot-ollama.db \
KNOWLEDGE_PILOT_ADDR=127.0.0.1:8083 \
KNOWLEDGE_PILOT_LLM_PROVIDER=custom \
KNOWLEDGE_PILOT_LLM_BASE_URL=http://127.0.0.1:11434/v1 \
KNOWLEDGE_PILOT_LLM_MODEL=qwen2.5:1.5b \
cargo run
