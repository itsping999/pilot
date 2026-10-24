import { createHash, timingSafeEqual } from "node:crypto";
import type { IncomingHttpHeaders } from "node:http";
import type { Config } from "./config.js";
import { ApiError } from "./errors.js";

export interface Principal {
  tenantId: string;
  userId: string;
  key: string;
}

export function principal(tenantId: string, userId: string): Principal {
  return {
    tenantId,
    userId,
    key: createHash("sha256")
      .update(JSON.stringify([tenantId, userId]))
      .digest("hex"),
  };
}

export function authenticate(
  headers: IncomingHttpHeaders,
  config: Config,
): Principal {
  if (!config.apiToken)
    throw new ApiError(503, "auth_not_configured", "服务认证尚未配置。");
  const raw = headers.authorization;
  const provided =
    typeof raw === "string" && raw.startsWith("Bearer ") ? raw.slice(7) : "";
  const expected = Buffer.from(config.apiToken);
  const candidate = Buffer.from(provided);
  if (
    candidate.length !== expected.length ||
    !timingSafeEqual(candidate, expected)
  )
    throw new ApiError(401, "unauthorized", "调用方认证失败。");
  const valid = (value: string | string[] | undefined): value is string =>
    typeof value === "string" &&
    /^[A-Za-z0-9][A-Za-z0-9._:@+\-]{0,127}$/.test(value);
  const user = headers["x-user-id"];
  const tenant = headers["x-tenant-id"] ?? "default";
  if (!valid(user) || !valid(tenant))
    throw new ApiError(
      400,
      "principal_required",
      "需要有效的受信用户和企业标识。",
    );
  return principal(tenant, user);
}
