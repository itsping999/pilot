#!/bin/bash

# 监控文档导入进度

TASK_ID="b48b7jtta"
OUTPUT_FILE="/private/tmp/claude-501/-Users-haiwell-knowledge-pilot/79149ad1-2146-4724-a006-4b2ac45b31af/tasks/${TASK_ID}.output"

echo "========================================="
echo "监控Ollama文档导入进度"
echo "========================================="
echo ""

while true; do
    # 检查任务是否还在运行
    STATUS=$(curl -s "http://127.0.0.1:8080/api/tasks/${TASK_ID}" 2>/dev/null | jq -r '.status' 2>/dev/null)
    
    if [ "$STATUS" = "completed" ]; then
        echo "✓ 任务已完成！"
        break
    elif [ "$STATUS" = "failed" ]; then
        echo "❌ 任务失败"
        break
    fi
    
    # 显示最新输出
    echo "[$(date '+%H:%M:%S')] 导入进行中..."
    tail -3 "$OUTPUT_FILE" 2>/dev/null
    
    # 检查是否有错误
    if grep -q "error" "$OUTPUT_FILE" 2>/dev/null; then
        echo "⚠️  发现错误："
        grep "error" "$OUTPUT_FILE" | tail -3
    fi
    
    sleep 10
done

echo ""
echo "完整日志："
tail -20 "$OUTPUT_FILE"
