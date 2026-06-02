# Kubernetes RAG 测试计划

## 测试目标
评估KnowledgePilot RAG服务对中文技术文档的检索和生成能力

## 文档来源
Kubernetes 官方中文文档 (kubernetes.io/zh-cn)

## 测试层次

### 1. 基础概念
- Pod是什么？
- Service的作用？
- Deployment和ReplicaSet的区别？

### 2. 配置管理
- 如何配置ConfigMap？
- Secret的使用方法？
- 环境变量注入

### 3. 网络和服务
- ClusterIP、NodePort、LoadBalancer的区别？
- Ingress的配置？
- Service Discovery如何工作？

### 4. 存储管理
- PersistentVolume的使用？
- StorageClass的配置？

### 5. 故障排查
- Pod无法启动的原因？
- Service无法访问的问题？
- 性能调优建议？

## 评估指标
1. **相关性**：检索到的文档片段是否与问题相关
2. **完整性**：答案是否涵盖问题的关键要点
3. **准确性**：信息是否准确无误
4. **可用性**：答案是否易于理解和应用

## 测试数量
计划测试 20-30 个问题，覆盖各个层次
