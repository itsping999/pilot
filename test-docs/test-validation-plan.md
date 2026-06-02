# KnowledgePilot 商用级别测试验证计划

## 📋 测试验证清单

### 阶段1：嵌入优化验证（当前任务）

#### 1.1 Ollama嵌入导入验证
```bash
# 验证步骤
□ 检查数据库文件生成
  ls -lh data/knowledge-pilot-ollama.db

□ 验证文档数量
  sqlite3 data/knowledge-pilot-ollama.db "SELECT COUNT(*) FROM documents;"

□ 验证chunk数量
  sqlite3 data/knowledge-pilot-ollama.db "SELECT COUNT(*) FROM chunks;"

□ 验证向量维度
  sqlite3 data/knowledge-pilot-ollama.db "SELECT embedding_dim FROM config;"
```

#### 1.2 服务启动验证
```bash
# 启动Ollama模式服务
./test-docs/start-ollama-service.sh

# 验证服务状态
curl http://127.0.0.1:8081/health

# 验证配置
curl http://127.0.0.1:8081/config | jq '.embedding_model'
```

#### 1.3 对比测试
```bash
# 运行对比测试脚本
./test-docs/compare-embedding-results.sh

# 查看结果
cat test-docs/comparison-results/comparison-report.md
```

### 阶段2：生成优化验证（下一步）

#### 2.1 LLM集成验证
```bash
# 安装Ollama聊天模型
ollama pull qwen2.5:1.5b

# 验证模型可用
ollama list | grep qwen2.5

# 启动带LLM的服务
KNOWLEDGE_PILOT_LLM_PROVIDER=custom \
KNOWLEDGE_PILOT_LLM_BASE_URL=http://127.0.0.1:11434/v1 \
KNOWLEDGE_PILOT_LLM_MODEL=qwen2.5:1.5b \
cargo run -- serve --port 8082
```

#### 2.2 答案质量验证
```bash
# 测试问题
TEST_QUESTIONS=(
    "Pod是什么？"
    "如何创建Deployment？"
    "ConfigMap和Secret的区别？"
)

# 生成答案并评估
for q in "${TEST_QUESTIONS[@]}"; do
    echo "问题：$q"
    echo "---"
    curl -s -X POST http://127.0.0.1:8082/rag/query \
        -H 'Content-Type: application/json' \
        -d "{\"question\": \"$q\", \"top_k\": 5}" \
        | jq -r '.data.answer'
    echo -e "\n\n"
done
```

#### 2.3 评估指标
```python
# 评估脚本（Python示例）
evaluation_criteria = {
    "relevance": {
        "weight": 0.3,
        "score_range": (0, 1),
        "description": "答案与问题的相关程度"
    },
    "completeness": {
        "weight": 0.25,
        "score_range": (0, 1),
        "description": "答案是否涵盖关键要点"
    },
    "accuracy": {
        "weight": 0.25,
        "score_range": (0, 1),
        "description": "信息是否准确无误"
    },
    "readability": {
        "weight": 0.2,
        "score_range": (0, 1),
        "description": "答案是否易于理解"
    }
}
```

### 阶段3：性能验证（后续）

#### 3.1 响应时间测试
```bash
# 基准测试
ab -n 100 -c 10 http://127.0.0.1:8080/rag/query

# 或使用wrk
wrk -t4 -c100 -d30s -s test-query.lua http://127.0.0.1:8080
```

#### 3.2 并发测试
```bash
# 并发测试脚本
for i in {1..10}; do
    curl -s -X POST http://127.0.0.1:8080/rag/query \
        -H 'Content-Type: application/json' \
        -d '{"question": "测试问题", "top_k": 3}' &
done
wait
```

#### 3.3 内存和CPU监控
```bash
# 使用htop或top监控
htop -p $(pgrep knowledge-pilot)

# 或使用性能分析工具
cargo build --release
perf record -g ./target/release/knowledge-pilot
perf report
```

## 📊 测试用例库

### 基础概念测试（15个）
```json
{
  "category": "基础概念",
  "questions": [
    {
      "id": 1,
      "question": "Pod是什么？它在Kubernetes中扮演什么角色？",
      "expected_keywords": ["最小调度单位", "容器组", "Pod生命周期"],
      "difficulty": "基础"
    },
    {
      "id": 2,
      "question": "Deployment和ReplicaSet有什么区别？",
      "expected_keywords": ["声明式更新", "回滚", "副本管理"],
      "difficulty": "基础"
    },
    {
      "id": 3,
      "question": "Kubernetes的Service是什么？它解决了什么问题？",
      "expected_keywords": ["服务发现", "负载均衡", "网络暴露"],
      "difficulty": "基础"
    }
  ]
}
```

### 配置管理测试（10个）
```json
{
  "category": "配置管理",
  "questions": [
    {
      "id": 16,
      "question": "ConfigMap和Secret有什么区别？各有什么使用场景？",
      "expected_keywords": ["非敏感数据", "敏感数据", "base64编码"],
      "difficulty": "中等"
    },
    {
      "id": 17,
      "question": "如何在Pod中使用ConfigMap？有哪些注入方式？",
      "expected_keywords": ["环境变量", "卷挂载", "envFrom"],
      "difficulty": "中等"
    }
  ]
}
```

### 网络测试（10个）
```json
{
  "category": "网络",
  "questions": [
    {
      "id": 26,
      "question": "ClusterIP、NodePort和LoadBalancer三种Service类型有什么区别？",
      "expected_keywords": ["集群内部", "节点端口", "云负载均衡"],
      "difficulty": "中等"
    },
    {
      "id": 27,
      "question": "Ingress是什么？它与Service的关系是什么？",
      "expected_keywords": ["入口控制器", "HTTP路由", "域名"],
      "difficulty": "中等"
    }
  ]
}
```

### 存储测试（10个）
```json
{
  "category": "存储",
  "questions": [
    {
      "id": 36,
      "question": "PersistentVolume和PersistentVolumeClaim是什么？",
      "expected_keywords": ["存储抽象", "声明", "绑定"],
      "difficulty": "中等"
    }
  ]
}
```

### 调度测试（10个）
```json
{
  "category": "调度",
  "questions": [
    {
      "id": 46,
      "question": "如何让Pod调度到特定的节点上？",
      "expected_keywords": ["nodeSelector", "nodeAffinity", "污点"],
      "difficulty": "高级"
    }
  ]
}
```

### 故障排查测试（10个）
```json
{
  "category": "故障排查",
  "questions": [
    {
      "id": 56,
      "question": "Pod一直处于Pending状态可能是什么原因？",
      "expected_keywords": ["资源不足", "节点选择器", "PVC未绑定"],
      "difficulty": "高级"
    }
  ]
}
```

### 安全测试（5个）
```json
{
  "category": "安全",
  "questions": [
    {
      "id": 66,
      "question": "Kubernetes的RBAC是什么？如何配置角色和权限？",
      "expected_keywords": ["Role", "ClusterRole", "RoleBinding"],
      "difficulty": "高级"
    }
  ]
}
```

## 🎯 评估标准

### 商用级别标准

#### 优秀（90-100分）
- 检索准确率 > 90%
- 答案相关性 > 90%
- 响应时间 < 1秒
- 并发能力 > 100 QPS
- 系统可用性 > 99.9%

#### 良好（80-89分）
- 检索准确率 80-90%
- 答案相关性 80-90%
- 响应时间 1-2秒
- 并发能力 50-100 QPS
- 系统可用性 99-99.9%

#### 及格（70-79分）
- 检索准确率 70-80%
- 答案相关性 70-80%
- 响应时间 2-3秒
- 并发能力 20-50 QPS
- 系统可用性 95-99%

#### 不及格（<70分）
- 检索准确率 < 70%
- 答案相关性 < 70%
- 响应时间 > 3秒
- 并发能力 < 20 QPS
- 系统可用性 < 95%

### 评分公式
```python
def calculate_score(metrics):
    score = (
        metrics['retrieval_accuracy'] * 0.25 +
        metrics['answer_relevance'] * 0.25 +
        metrics['answer_completeness'] * 0.2 +
        metrics['response_time'] * 0.15 +
        metrics['concurrency'] * 0.15
    )
    return score

# 商用级别阈值
COMMERCIAL_THRESHOLD = 85
```

## 📈 测试报告模板

### 测试报告结构
```markdown
# KnowledgePilot 测试报告

## 1. 测试环境
- 硬件配置
- 软件版本
- 网络环境

## 2. 测试结果摘要
- 总体评分
- 各项指标得分
- 对比基准

## 3. 详细测试结果
- 基础概念测试
- 配置管理测试
- 网络测试
- 存储测试
- 调度测试
- 故障排查测试
- 安全测试

## 4. 性能测试结果
- 响应时间
- 并发能力
- 资源占用

## 5. 问题和建议
- 发现的问题
- 改进建议
- 优先级

## 6. 结论
- 总体评价
- 商用可行性
- 下一步计划
```

## 🔍 自动化测试框架

### 测试脚本结构
```
test-docs/
├── tests/
│   ├── unit/                 # 单元测试
│   ├── integration/          # 集成测试
│   ├── performance/          # 性能测试
│   └── e2e/                  # 端到端测试
├── fixtures/                 # 测试数据
├── reports/                  # 测试报告
└── run-all-tests.sh          # 运行所有测试
```

### 测试运行命令
```bash
# 运行所有测试
./test-docs/tests/run-all-tests.sh

# 运行特定测试
./test-docs/tests/run-unit-tests.sh
./test-docs/tests/run-integration-tests.sh
./test-docs/tests/run-performance-tests.sh
./test-docs/tests/run-e2e-tests.sh

# 生成测试报告
./test-docs/tests/generate-report.sh
```

## 📚 参考资源

### 测试工具
- Apache Bench (ab)
- wrk
- hey
- vegeta

### 性能监控
- htop
- iostat
- vmstat
- perf

### 质量评估
- BLEU分数
- ROUGE分数
- 人工评估

## 🎓 最佳实践

### 测试原则
1. **全面覆盖**：覆盖所有功能点
2. **自动化优先**：尽可能自动化测试
3. **持续集成**：集成到CI/CD流程
4. **结果可追溯**：记录所有测试结果

### 测试频率
- **单元测试**：每次提交
- **集成测试**：每日构建
- **性能测试**：每周一次
- **全面测试**：每月一次

## 📞 联系方式

**测试负责人**：Claude Code  
**测试环境**：macOS + Ollama  
**测试框架**：自定义Shell脚本  
**报告格式**：Markdown  

---

**文档版本**：v1.0  
**创建时间**：2026-06-02  
**最后更新**：2026-06-02
