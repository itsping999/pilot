#!/bin/bash

# Kubernetes文档导入脚本
# 使用KnowledgePilot的ingest命令导入文档

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
K8S_DOCS_DIR="$SCRIPT_DIR/k8s-docs/content/zh-cn/docs"

echo "========================================="
echo "Kubernetes文档导入工具"
echo "========================================="

# 检查KnowledgePilot服务是否运行
echo "[1/4] 检查KnowledgePilot服务..."
if ! curl -s http://127.0.0.1:8080/health > /dev/null 2>&1; then
    echo "⚠️  KnowledgePilot服务未运行"
    echo "请先启动服务：cd $PROJECT_ROOT && cargo run"
    exit 1
fi
echo "✓ KnowledgePilot服务运行正常"

# 统计文档信息
echo ""
echo "[2/4] 统计文档信息..."
TOTAL_FILES=$(find "$K8S_DOCS_DIR" -name "*.md" -type f | wc -l | tr -d ' ')
echo "✓ 找到 $TOTAL_FILES 个Markdown文档"

# 选择要导入的文档
echo ""
echo "[3/4] 准备导入文档..."
echo "导入范围：概念(concepts) + 任务(tasks)文档"

# 创建临时目录整理文档
TEMP_DIR=$(mktemp -d)
mkdir -p "$TEMP_DIR/kubernetes"

# 复制概念文档
echo "  - 复制概念文档..."
cp -r "$K8S_DOCS_DIR/concepts" "$TEMP_DIR/kubernetes/"

# 复制任务文档
echo "  - 复制任务文档..."
cp -r "$K8S_DOCS_DIR/tasks" "$TEMP_DIR/kubernetes/"

IMPORT_FILES=$(find "$TEMP_DIR/kubernetes" -name "*.md" -type f | wc -l | tr -d ' ')
echo "✓ 准备导入 $IMPORT_FILES 个文档"

# 执行导入
echo ""
echo "[4/4] 开始导入文档到KnowledgePilot..."
cd "$PROJECT_ROOT"
KNOWLEDGE_PILOT_DB_PATH=./data/knowledge-pilot.db \
    cargo run -- ingest "$TEMP_DIR/kubernetes"

# 清理临时目录
rm -rf "$TEMP_DIR"

echo ""
echo "========================================="
echo "✓ 文档导入完成！"
echo "========================================="
echo ""
echo "下一步："
echo "1. 打开QA页面：http://127.0.0.1:8080/qa"
echo "2. 或运行测试用例进行效果评估"
echo ""
