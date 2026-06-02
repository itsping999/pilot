#!/bin/bash

# Kubernetes RAG测试运行脚本
# 自动执行测试用例并收集结果

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RESULTS_FILE="$SCRIPT_DIR/test-results-$(date +%Y%m%d-%H%M%S).md"
API_BASE="http://127.0.0.1:8080"

echo "========================================="
echo "Kubernetes RAG 效果测试"
echo "========================================="

# 检查服务是否运行
echo "[检查] 验证KnowledgePilot服务..."
if ! curl -s "$API_BASE/health" > /dev/null 2>&1; then
    echo "❌ KnowledgePilot服务未运行"
    echo "请先启动服务：cd $SCRIPT_DIR/.. && cargo run"
    exit 1
fi
echo "✓ 服务运行正常"
echo ""

# 初始化结果文件
cat > "$RESULTS_FILE" << HEADER
# Kubernetes RAG 效果测试报告

测试时间：$(date)
测试环境：KnowledgePilot RAG Service

## 测试用例结果

| # | 类别 | 问题 | 难度 | 答案摘要 | 引用数量 | 评估 |
|---|------|------|------|---------|---------|------|
HEADER

# 测试用例数组
declare -a QUESTIONS=(
    "Pod是什么？它在Kubernetes中扮演什么角色？"
    "Deployment和ReplicaSet有什么区别？"
    "Kubernetes的Service是什么？它解决了什么问题？"
    "如何创建一个Deployment？需要哪些必填字段？"
    "ConfigMap和Secret有什么区别？各有什么使用场景？"
    "如何在Pod中使用ConfigMap？有哪些注入方式？"
    "ClusterIP、NodePort和LoadBalancer三种Service类型有什么区别？"
    "Ingress是什么？它与Service的关系是什么？"
    "PersistentVolume和PersistentVolumeClaim是什么？它们如何工作？"
    "StorageClass的作用是什么？如何动态创建存储？"
    "如何让Pod调度到特定的节点上？"
    "Pod的资源请求和限制是什么？如何配置？"
    "Pod一直处于Pending状态可能是什么原因？"
    "如何查看Pod的日志？如何调试容器问题？"
    "Kubernetes的RBAC是什么？如何配置角色和权限？"
)

declare -a CATEGORIES=(
    "基础概念"
    "基础概念"
    "基础概念"
    "工作负载"
    "配置管理"
    "配置管理"
    "网络"
    "网络"
    "存储"
    "存储"
    "调度"
    "调度"
    "故障排查"
    "故障排查"
    "安全"
)

declare -a DIFFICULTIES=(
    "基础"
    "基础"
    "基础"
    "基础"
    "中等"
    "中等"
    "中等"
    "中等"
    "中等"
    "中等"
    "高级"
    "高级"
    "高级"
    "基础"
    "高级"
)

# 运行测试
for i in "${!QUESTIONS[@]}"; do
    NUM=$((i + 1))
    echo "[$NUM/15] 测试：${QUESTIONS[$i]}"
    
    # 调用RAG API
    RESPONSE=$(curl -s -X POST "$API_BASE/rag/query" \
        -H 'Content-Type: application/json' \
        -d "{
            \"question\": \"${QUESTIONS[$i]}\",
            \"top_k\": 3,
            \"access_scopes\": [\"public\"]
        }")
    
    # 提取答案（截取前200字符）
    ANSWER=$(echo "$RESPONSE" | jq -r '.data.answer' 2>/dev/null | head -c 200)
    
    # 提取引用数量
    CITATIONS_COUNT=$(echo "$RESPONSE" | jq -r '.data.citations | length' 2>/dev/null)
    
    # 提取置信度
    CONFIDENCE=$(echo "$RESPONSE" | jq -r '.data.confidence.level' 2>/dev/null)
    
    # 记录到结果文件
    echo "| $NUM | ${CATEGORIES[$i]} | ${QUESTIONS[$i]:0:30}... | ${DIFFICULTIES[$i]} | ${ANSWER:0:50}... | $CITATIONS_COUNT | $CONFIDENCE |" >> "$RESULTS_FILE"
    
    echo "  ✓ 完成（引用: $CITATIONS_COUNT, 置信度: $CONFIDENCE）"
    
    # 短暂延迟避免过快请求
    sleep 0.5
done

echo ""
echo "========================================="
echo "✓ 测试完成！"
echo "========================================="
echo ""
echo "结果已保存到：$RESULTS_FILE"
echo ""

# 添加统计信息
cat >> "$RESULTS_FILE" << FOOTER

## 统计信息

- **测试用例数**：15
- **测试时间**：$(date)
- **文档来源**：Kubernetes 官方中文文档

## 建议

基于测试结果，建议：
1. 检查检索相关性是否准确
2. 评估答案完整性是否满足需求
3. 验证信息准确性
4. 测试答案的可用性和可操作性
FOOTER

echo "完整报告已生成：$RESULTS_FILE"
