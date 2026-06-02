# Kubernetes RAG 测试套件

本测试套件用于评估KnowledgePilot RAG服务对中文技术文档的检索和生成能力。

## 📋 测试概览

- **文档来源**：Kubernetes 官方中文文档 (kubernetes.io/zh-cn)
- **文档规模**：约386个Markdown文件（概念+任务）
- **测试用例**：15个问题，覆盖7个类别
- **测试难度**：基础、中等、高级

## 🚀 快速开始

### 步骤1：启动KnowledgePilot服务

```bash
cd /Users/haiwell/knowledge-pilot
cargo run
```

等待服务启动完成（默认运行在 http://127.0.0.1:8080）

### 步骤2：导入Kubernetes文档

在新的终端窗口中运行：

```bash
cd /Users/haiwell/knowledge-pilot/test-docs
./import-k8s-docs.sh
```

脚本会自动：
1. 检查服务是否运行
2. 统计文档信息
3. 整理概念和任务文档
4. 导入到KnowledgePilot

**预计时间**：2-5分钟（取决于文档数量）

### 步骤3：运行测试

```bash
./run-k8s-tests.sh
```

脚本会自动：
1. 执行15个测试用例
2. 收集每个问题的答案
3. 统计引用和置信度
4. 生成测试报告

**预计时间**：1-2分钟

### 步骤4：查看测试结果

测试报告会保存为：`test-results-YYYYMMDD-HHMMSS.md`

## 📊 测试用例列表

| # | 类别 | 问题 | 难度 |
|---|------|------|------|
| 1 | 基础概念 | Pod是什么？它在Kubernetes中扮演什么角色？ | 基础 |
| 2 | 基础概念 | Deployment和ReplicaSet有什么区别？ | 基础 |
| 3 | 基础概念 | Kubernetes的Service是什么？它解决了什么问题？ | 基础 |
| 4 | 工作负载 | 如何创建一个Deployment？需要哪些必填字段？ | 基础 |
| 5 | 配置管理 | ConfigMap和Secret有什么区别？各有什么使用场景？ | 中等 |
| 6 | 配置管理 | 如何在Pod中使用ConfigMap？有哪些注入方式？ | 中等 |
| 7 | 网络 | ClusterIP、NodePort和LoadBalancer三种Service类型有什么区别？ | 中等 |
| 8 | 网络 | Ingress是什么？它与Service的关系是什么？ | 中等 |
| 9 | 存储 | PersistentVolume和PersistentVolumeClaim是什么？它们如何工作？ | 中等 |
| 10 | 存储 | StorageClass的作用是什么？如何动态创建存储？ | 中等 |
| 11 | 调度 | 如何让Pod调度到特定的节点上？ | 高级 |
| 12 | 调度 | Pod的资源请求和限制是什么？如何配置？ | 高级 |
| 13 | 故障排查 | Pod一直处于Pending状态可能是什么原因？ | 高级 |
| 14 | 故障排查 | 如何查看Pod的日志？如何调试容器问题？ | 基础 |
| 15 | 安全 | Kubernetes的RBAC是什么？如何配置角色和权限？ | 高级 |

## 📈 评估指标

### 1. 检索相关性
- 检索到的文档片段是否与问题相关
- 是否能找到正确的文档章节

### 2. 答案完整性
- 答案是否涵盖问题的关键要点
- 是否遗漏重要信息

### 3. 答案准确性
- 信息是否准确无误
- 是否存在错误或误导性信息

### 4. 答案可用性
- 答案是否易于理解和应用
- 是否包含实际可操作的指导

## 🔧 手动测试

如果想手动测试特定问题，可以使用curl：

```bash
curl -X POST http://127.0.0.1:8080/rag/query \
  -H 'Content-Type: application/json' \
  -d '{
    "question": "您的问题",
    "top_k": 3,
    "access_scopes": ["public"]
  }'
```

或者打开Web UI：http://127.0.0.1:8080/qa

## 📁 文件结构

```
test-docs/
├── README.md                    # 本文件
├── k8s-docs/                    # Kubernetes文档仓库
│   └── content/zh-cn/docs/     # 中文文档
├── kubernetes-test-plan.md      # 测试计划
├── kubernetes-test-cases.json   # 测试用例定义
├── import-k8s-docs.sh          # 文档导入脚本
├── run-k8s-tests.sh            # 测试运行脚本
└── test-results-*.md           # 测试结果报告
```

## 🎯 预期结果

理想的RAG服务应该能够：
1. 准确检索到相关文档片段
2. 生成完整、准确的答案
3. 提供可引用的来源
4. 处理中英文混合的技术术语
5. 在置信度低时给出合理提示

## 📝 注意事项

1. **文档更新**：Kubernetes文档会定期更新，建议定期重新导入
2. **服务性能**：大量文档导入可能需要较长时间，请耐心等待
3. **测试环境**：建议在本地环境测试，避免网络延迟影响结果
4. **结果分析**：结合具体场景评估答案质量，不同场景的要求可能不同

## 🔄 后续改进

根据测试结果，可以考虑：
1. 调整chunk_size和chunk_overlap参数
2. 优化嵌入模型配置
3. 增加更多测试用例
4. 对比不同嵌入模型的效果

## 📞 问题反馈

如果遇到问题，请检查：
1. KnowledgePilot服务是否正常运行
2. 文档是否正确导入
3. API是否能正常访问
4. 查看服务日志获取详细错误信息
