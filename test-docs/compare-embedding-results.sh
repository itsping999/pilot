#!/bin/bash

# 嵌入模型对比测试脚本
# 比较哈希嵌入和Ollama嵌入的效果

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RESULTS_DIR="$SCRIPT_DIR/comparison-results"
mkdir -p "$RESULTS_DIR"

echo "========================================="
echo "嵌入模型效果对比测试"
echo "========================================="

# 定义测试问题
QUESTIONS=(
    "Pod是什么？它在Kubernetes中扮演什么角色？"
    "Deployment和ReplicaSet有什么区别？"
    "Kubernetes的Service是什么？它解决了什么问题？"
    "如何创建一个Deployment？需要哪些必填字段？"
    "ConfigMap和Secret有什么区别？各有什么使用场景？"
)

# 测试哈希嵌入
echo ""
echo "[1/2] 测试哈希嵌入模型..."
echo "注意：需要先启动哈希模式的服务"
echo ""

HASH_RESULTS="$RESULTS_DIR/hash-embedding-results.md"
cat > "$HASH_RESULTS" << HASH_HEADER
# 哈希嵌入测试结果

| 问题 | 引用数量 | 置信度 | 答案摘要 |
|------|---------|--------|---------|
HASH_HEADER

for q in "${QUESTIONS[@]}"; do
    echo "  测试：${q:0:30}..."
    RESPONSE=$(curl -s -X POST http://127.0.0.1:8080/rag/query \
        -H 'Content-Type: application/json' \
        -d "{\"question\": \"$q\", \"top_k\": 3, \"access_scopes\": [\"public\"]}")
    
    CITATIONS=$(echo "$RESPONSE" | jq -r '.data.citations | length' 2>/dev/null)
    CONFIDENCE=$(echo "$RESPONSE" | jq -r '.data.confidence.level' 2>/dev/null)
    ANSWER=$(echo "$RESPONSE" | jq -r '.data.answer' 2>/dev/null | head -c 100)
    
    echo "| ${q:0:30}... | $CITATIONS | $CONFIDENCE | ${ANSWER:0:50}... |" >> "$HASH_RESULTS"
    sleep 0.5
done

echo "✓ 哈希嵌入测试完成"

# 测试Ollama嵌入
echo ""
echo "[2/2] 测试Ollama嵌入模型..."
echo "注意：需要启动Ollama模式的服务（端口8081）"
echo ""

OLLAMA_RESULTS="$RESULTS_DIR/ollama-embedding-results.md"
cat > "$OLLAMA_RESULTS" << OLLAMA_HEADER
# Ollama嵌入测试结果

| 问题 | 引用数量 | 置信度 | 答案摘要 |
|------|---------|--------|---------|
OLLAMA_HEADER

for q in "${QUESTIONS[@]}"; do
    echo "  测试：${q:0:30}..."
    RESPONSE=$(curl -s -X POST http://127.0.0.1:8081/rag/query \
        -H 'Content-Type: application/json' \
        -d "{\"question\": \"$q\", \"top_k\": 3, \"access_scopes\": [\"public\"]}")
    
    CITATIONS=$(echo "$RESPONSE" | jq -r '.data.citations | length' 2>/dev/null)
    CONFIDENCE=$(echo "$RESPONSE" | jq -r '.data.confidence.level' 2>/dev/null)
    ANSWER=$(echo "$RESPONSE" | jq -r '.data.answer' 2>/dev/null | head -c 100)
    
    echo "| ${q:0:30}... | $CITATIONS | $CONFIDENCE | ${ANSWER:0:50}... |" >> "$OLLAMA_RESULTS"
    sleep 0.5
done

echo "✓ Ollama嵌入测试完成"

# 生成对比报告
echo ""
echo "生成对比报告..."
cat > "$RESULTS_DIR/comparison-report.md" << REPORT
# 嵌入模型对比报告

测试时间：$(date)

## 测试环境
- **哈希嵌入**：确定性哈希向量
- **Ollama嵌入**：qwen3-embedding:0.6b模型

## 对比结果

### 哈希嵌入
$(cat "$HASH_RESULTS" | tail -n +4)

### Ollama嵌入
$(cat "$OLLAMA_RESULTS" | tail -n +4)

## 分析

### 检索质量对比
[待填写]

### 答案质量对比
[待填写]

### 性能对比
[待填写]

## 结论
[待填写]
REPORT

echo "✓ 对比报告已生成"
echo ""
echo "结果文件："
echo "  - $HASH_RESULTS"
echo "  - $OLLAMA_RESULTS"
echo "  - $RESULTS_DIR/comparison-report.md"
echo ""
echo "========================================="
echo "对比测试完成！"
echo "========================================="
