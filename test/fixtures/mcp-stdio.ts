import { createInterface } from "node:readline";

const lines = createInterface({ input: process.stdin });
lines.on("line", (line) => {
  const message = JSON.parse(line) as {
    id?: string | number;
    method: string;
    params?: Record<string, unknown>;
  };
  if (message.id === undefined) return;
  let result: unknown = {};
  if (message.method === "initialize")
    result = {
      protocolVersion: message.params?.protocolVersion,
      capabilities: { tools: {} },
      serverInfo: { name: "stdio-fixture", version: "1.0.0" },
    };
  else if (message.method === "tools/list")
    result = {
      tools: [
        {
          name: "echo",
          description: "Echo scoped fixture state.",
          inputSchema: {
            type: "object",
            properties: { value: { type: "string" } },
            required: ["value"],
          },
        },
      ],
    };
  else if (message.method === "tools/call")
    result = {
      content: [
        {
          type: "text",
          text: JSON.stringify({
            user: process.env.USER_ID,
            tenant: process.env.TENANT_ID,
            session: process.env.SESSION_ID,
            cwd: process.cwd(),
            inherited_service_token: !!process.env.AGENT_API_TOKEN,
            value: (message.params?.arguments as { value?: string })?.value,
          }),
        },
      ],
    };
  process.stdout.write(
    JSON.stringify({ jsonrpc: "2.0", id: message.id, result }) + "\n",
  );
});
