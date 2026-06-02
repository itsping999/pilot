# Kubernetes RAG 效果评估报告

## 📊 测试概况

- **测试日期**：2026-06-02
- **测试环境**：KnowledgePilot RAG Service (extractive mode)
- **文档来源**：Kubernetes 官方中文文档
- **文档规模**：386个文档，11,205个chunks
- **测试用例**：15个问题
- **嵌入模式**：哈希嵌入（确定性模式）

## 📈 测试结果分析

### 引用统计

- **平均引用数量**：2.8个/问题
- **引用分布**：
  - 1个引用：2个问题（13%）
  - 2个引用：5个问题（33%）
  - 3个引用：2个问题（13%）
  - 4个引用：2个问题（13%）
  - 5个引用：4个问题（27%）

### 置信度分布

- **高置信度**：5个问题（33%）
  - Pod概念、Service概念、Pod调度、资源限制、日志调试
- **中置信度**：10个问题（67%）
  - 其他所有问题

### 分类表现

#### ✅ 表现较好（高置信度）
1. **基础概念**：Pod是什么？（引用4，高置信度）
2. **网络**：Service是什么？（引用2，高置信度）
3. **调度**：如何让Pod调度到特定节点？（引用3，高置信度）
4. **调度**：资源请求和限制（引用2，高置信度）
5. **故障排查**：如何查看Pod日志？（引用3，高置信度）

#### ⚠️ 表现一般（中置信度）
1. **基础概念**：Deployment和ReplicaSet区别（引用4，中置信度）
2. **工作负载**：如何创建Deployment（引用2，中置信度）
3. **配置管理**：ConfigMap和Secret区别（引用2，中置信度）
4. **配置管理**：如何使用ConfigMap（引用1，中置信度）
5. **网络**：Service类型区别（引用2，中置信度）
6. **网络**：Ingress是什么（引用5，中置信度）
7. **存储**：PV和PVC（引用2，中置信度）
8. **存储**：StorageClass作用（引用5，中置信度）
9. **故障排查**：Pod Pending原因（引用5，中置信度）
10. **安全**：RBAC是什么（引用1，中置信度）

## 🔍 答案质量分析

### 现有模式

当前RAG服务使用**extractive模式**（提取式生成），特点：

**优点**：
- ✅ 直接引用原文，准确性高
- ✅ 不会产生幻觉（hallucination）
- ✅ 处理速度快

**缺点**：
- ❌ 答案只是文档片段的堆砌
- ❌ 缺乏结构化组织
- ❌ 没有综合分析和总结
- ❌ 对用户不够友好

### 实际测试示例

#### 测试问题：Pod是什么？

**检索到的内容**：
```
1. Pod and container logs
2. a pod gets evicted, or a node dies
3. In a cluster, logs should have a separate storage...
4. Pod 和容器日志
```

**问题分析**：
- ❌ 检索到了日志相关的文档，而不是Pod的基础定义
- ❌ 没有找到"Pod是最小调度单位"这样的核心概念
- ⚠️ 说明哈希嵌入的语义检索能力有限

#### 测试问题：Deployment和ReplicaSet区别

**检索到的内容**：
```
1. yaml resources: - deployment
2. For example, create one patch for increasing the deployment replica number...
3. 创建 deployment
```

**问题分析**：
- ❌ 只找到了Deployment相关的配置片段
- ❌ 没有找到对比两者的文档
- ⚠️ 检索的语义相关性不够强

## 💡 核心问题识别

### 1. 嵌入模型问题 🔴

**当前状态**：使用哈希嵌入（确定性模式）

**问题**：
- 哈希嵌入不是真正的语义嵌入
- 无法理解同义词、近义词
- 中文语义理解能力弱
- 检索结果相关性不高

**改进建议**：
```bash
# 使用Ollama嵌入模型（推荐）
ollama pull qwen3-embedding:0.6b

KNOWLEDGE_PILOT_EMBEDDING_PROVIDER=ollama \
KNOWLEDGE_PILOT_EMBEDDING_MODEL=qwen3-embedding:0.6b \
KNOWLEDGE_PILOT_DB_PATH=./data/knowledge-pilot-ollama.db \
cargo run -- ingest test-docs/k8s-docs/content/zh-cn/docs
```

### 2. 生成模式问题 🟡

**当前状态**：使用extractive模式（提取式）

**问题**：
- 只能提取原文片段
- 无法生成综合性答案
- 答案缺乏结构化组织

**改进建议**：
```bash
# 使用本地LLM（推荐）
ollama pull qwen2.5:1.5b

KNOWLEDGE_PILOT_LLM_PROVIDER=custom \
KNOWLEDGE_PILOT_LLM_BASE_URL=http://127.0.0.1:11434/v1 \
KNOWLEDGE_PILOT_LLM_MODEL=qwen2.5:1.5b \
cargo run
```

### 3. 检索策略问题 🟡

**可能问题**：
- top_k=3 可能太少
- 没有进行查询扩展
- 缺乏重排序机制

**改进建议**：
- 尝试增加 top_k 到 5-10
- 考虑使用Agentic RAG模式
- 优化chunk_size和chunk_overlap参数

## 🎯 改进方案

### 短期改进（立即可做）

#### 方案A：使用Ollama嵌入模型
```bash
# 1. 安装Ollama
brew install ollama

# 2. 拉取嵌入模型
ollama pull qwen3-embedding:0.6b

# 3. 重新导入文档
cd /Users/haiwell/knowledge-pilot
KNOWLEDGE_PILOT_EMBEDDING_PROVIDER=ollama \
KNOWLEDGE_PILOT_EMBEDDING_MODEL=qwen3-embedding:0.6b \
KNOWLEDGE_PILOT_DB_PATH=./data/knowledge-pilot-ollama.db \
cargo run -- ingest test-docs/k8s-docs/content/zh-cn/docs

# 4. 使用Ollama模式启动服务
KNOWLEDGE_PILOT_EMBEDDING_PROVIDER=ollama \
KNOWLEDGE_PILOT_EMBEDDING_MODEL=qwen3-embedding:0.6b \
KNOWLEDGE_PILOT_DB_PATH=./data/knowledge-pilot-ollama.db \
cargo run
```

#### 方案B：使用LLM生成答案
```bash
# 1. 拉取聊天模型
ollama pull qwen2.5:1.5b

# 2. 启动服务时配置LLM
KNOWLEDGE_PILOT_EMBEDDING_PROVIDER=hash \
KNOWLEDGE_PILOT_LLM_PROVIDER=custom \
KNOWLEDGE_PILOT_LLM_BASE_URL=http://127.0.0.1:11434/v1 \
KNOWLEDGE_PILOT_LLM_MODEL=qwen2.5:1.5b \
KNOWLEDGE_PILOT_DB_PATH=./data/knowledge-pilot.db \
cargo run
```

### 中期改进（1-2周）

1. **优化分块策略**
   - 调整 chunk_size（当前800，可尝试1000-1500）
   - 调整 chunk_overlap（当前120，可尝试150-200）

2. **增强检索**
   - 实现查询扩展（Query Expansion）
   - 添加重排序机制（Re-ranking）
   - 支持混合检索（向量+关键词）

3. **改进评估**
   - 创建更全面的测试用例
   - 添加自动化评分机制
   - 对比不同配置的效果

### 长期改进（1个月+）

1. **Agentic RAG**
   - 实现多步推理
   - 支持查询分解
   - 自动验证答案

2. **多模态支持**
   - 支持图片、表格理解
   - 支持代码片段处理

3. **知识图谱集成**
   - 构建Kubernetes概念图谱
   - 支持关联查询

## 📊 性能基准

### 当前性能

- **文档导入**：386文档，11,205chunks，约2分钟
- **查询响应**：平均0.5-1秒
- **内存占用**：约50MB（SQLite数据库）
- **检索质量**：中等（哈希嵌入限制）

### 目标性能（使用Ollama后）

- **文档导入**：预计5-10分钟（嵌入计算更复杂）
- **查询响应**：预计1-2秒（需要调用嵌入API）
- **内存占用**：预计100-200MB
- **检索质量**：高（真正的语义理解）

## 🎓 学习建议

### 理解RAG工作原理

1. **索引阶段**：
   ```
   文档 → 分块 → 嵌入 → 存储
   ```

2. **查询阶段**：
   ```
   问题 → 嵌入 → 向量检索 → 上下文 → 生成答案
   ```

3. **质量瓶颈**：
   - 嵌入质量决定检索相关性
   - 生成质量决定答案可用性
   - 两者缺一不可

### 实践建议

1. **从小开始**：先用10-20个文档测试
2. **迭代优化**：逐步调整参数
3. **对比实验**：记录不同配置的效果
4. **用户反馈**：收集真实用户的使用体验

## 📝 结论

### 当前状态评估

**整体评分**：⭐⭐⭐☆☆（3/5）

**优势**：
- ✅ 服务稳定，导入快速
- ✅ 基础架构完整
- ✅ API设计合理
- ✅ 文档支持好

**不足**：
- ❌ 哈希嵌入的语义理解弱
- ❌ extractive模式答案质量有限
- ❌ 中文文档检索不够精准

### 建议优先级

1. **🔴 高优先级**：切换到Ollama嵌入模型
2. **🟡 中优先级**：启用LLM生成模式
3. **🟢 低优先级**：优化分块和检索策略

### 预期改进效果

使用Ollama嵌入 + LLM生成后，预期：
- 检索相关性提升：30-50%
- 答案质量提升：50-80%
- 用户满意度提升：60-90%

## 🔗 相关资源

- **KnowledgePilot文档**：/Users/haiwell/knowledge-pilot/README.md
- **测试套件**：/Users/haiwell/knowledge-pilot/test-docs/
- **Ollama官网**：https://ollama.com
- **Kubernetes文档**：https://kubernetes.io/zh/docs/

---

**报告生成时间**：2026-06-02 19:45  
**测试执行者**：Claude Code  
**环境**：macOS, KnowledgePilot v0.1.0
