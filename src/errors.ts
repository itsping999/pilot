export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}

export const notFound = () =>
  new ApiError(404, "not_found", "会话或运行不存在。");
export const badRequest = (message: string) =>
  new ApiError(400, "bad_request", message);
export const modelNotConfigured = () =>
  new ApiError(
    503,
    "model_not_configured",
    "未配置可用模型，请设置 AGENT_LLM_PROVIDER、AGENT_LLM_MODEL 和服务地址或 API Key。",
  );

export function publicError(error: unknown): ApiError {
  return error instanceof ApiError
    ? error
    : new ApiError(500, "internal_error", "服务暂时无法完成请求。");
}
