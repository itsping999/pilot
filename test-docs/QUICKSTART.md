# 🚀 快速开始指南

## 三个步骤测试Kubernetes RAG

### 第1步：启动服务

```bash
cd /Users/haiwell/knowledge-pilot
cargo run
```

等待看到服务启动成功提示

### 第2步：导入文档

新开一个终端，运行：

```bash
cd /Users/haiwell/knowledge-pilot/test-docs
./import-k8s-docs.sh
```

预计 2-5 分钟完成

### 第3步：运行测试

```bash
./run-k8s-tests.sh
```

预计 1-2 分钟完成

## 📄 查看结果

测试完成后，查看生成的报告：

```bash
ls test-results-*.md
cat test-results-*.md
```

## 🎯 预期效果

理想的RAG服务应该能够：
- ✅ 准确回答Kubernetes基础概念
- ✅ 提供完整的配置指南
- ✅ 给出实际的故障排查建议
- ✅ 引用具体的文档来源
- ✅ 在不确定时给出低置信度提示

## 💡 快速测试示例

如果想快速测试一个问题：

```bash
curl -X POST http://127.0.0.1:8080/rag/query \
  -H 'Content-Type: application/json' \
  -d '{
    "question": "Pod是什么？",
    "top_k": 3,
    "access_scopes": ["public"]
  }'
```

或打开浏览器访问：http://127.0.0.1:8080/qa

## 📊 测试覆盖

- 7个技术领域：基础概念、配置管理、网络、存储、调度、故障排查、安全
- 3个难度级别：基础、中等、高级
- 15个代表性问题

## 🔧 自定义测试

如需添加更多测试用例，编辑：
```bash
vim run-k8s-tests.sh
```

在QUESTIONS数组中添加新问题即可
