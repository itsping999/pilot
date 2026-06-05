# API 设计规范指南

## 设计原则

### 一致性
- 命名风格统一：URL 路径使用 kebab-case，JSON 字段使用 snake_case
- 响应格式统一：成功响应使用 `{code, message, data}` 信封
- 错误格式统一：错误响应使用 `{code, message, details}` 结构
- 分页格式统一：使用 `{items, total, limit, offset}` 结构

### 可演进
- 版本管理：URL 路径版本（/v1/）或请求头版本
- 向后兼容：新增字段不破坏现有客户端
- 废弃策略：至少保留两个大版本的兼容性
- 文档先行：API 变更先更新文档再实现

## RESTful 设计

### URL 设计
- 资源使用名词复数：`/users`、`/orders`、`/documents`
- 层级关系使用嵌套：`/users/{id}/orders`
- 操作使用 HTTP 方法：GET（查询）、POST（创建）、PUT（更新）、DELETE（删除）
- 避免动词：不使用 `/getUsers`、`/createOrder`

### 查询参数
- 过滤：`?status=active&category=tech`
- 排序：`?sort=created_at:desc`
- 分页：`?limit=20&offset=0` 或 `?page=1&per_page=20`
- 搜索：`?q=keyword` 或 `?search=keyword`
- 字段选择：`?fields=id,name,email`

### 状态码使用
- 200：成功获取或更新
- 201：成功创建
- 204：成功删除（无返回体）
- 400：请求参数错误
- 401：未认证
- 403：无权限
- 404：资源不存在
- 409：资源冲突
- 429：请求频率超限
- 500：服务器内部错误

## 认证与授权

### 认证方式
- Bearer Token：`Authorization: Bearer <token>`
- API Key：`X-API-Key: <key>`（适合服务间调用）
- OAuth 2.0：适合第三方应用授权

### 权限控制
- RBAC（基于角色的访问控制）：管理员、普通用户、只读用户
- 资源级权限：用户只能访问自己的数据
- 操作级权限：区分读取、创建、修改、删除权限

## 限流策略

### 限流算法
- 令牌桶：允许突发流量，平滑长期速率
- 滑动窗口：精确控制时间窗口内的请求数
- 固定窗口：简单但存在窗口边界突发问题

### 限流响应
- 返回 429 状态码
- 设置 `Retry-After` 响应头
- 在响应体中包含限流信息：剩余请求数、重置时间

## 文档规范

### OpenAPI 3.0
- 使用 YAML 格式编写 API 规范
- 每个接口包含：描述、参数、请求体、响应、示例
- 自动生成交互式文档（Swagger UI / Redoc）
- 文档与代码同步更新，CI 中验证文档正确性
