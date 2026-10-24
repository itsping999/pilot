import { loadConfig } from "./config.js";
import { publicError } from "./errors.js";
import { Store } from "./store.js";
import { CapabilityRegistry } from "./capabilities.js";
import { PiFactory } from "./pi.js";
import { AgentService } from "./service.js";
import { createHttpServer } from "./http.js";

async function main() {
  const config = loadConfig();
  const store = new Store(config.dataDir);
  let capabilities: CapabilityRegistry;
  try {
    capabilities = new CapabilityRegistry(config);
  } catch (error) {
    store.close();
    throw error;
  }
  const service = new AgentService(
    config,
    store,
    new PiFactory(config),
    capabilities,
  );
  const http = createHttpServer(service);
  try {
    await new Promise<void>((resolve, reject) => {
      http.server.once("error", reject);
      http.server.listen(config.port, config.host, () => {
        http.server.off("error", reject);
        resolve();
      });
    });
  } catch (error) {
    if (http.server.listening) await http.close();
    else await service.close();
    throw error;
  }
  const address = http.server.address();
  console.log(
    JSON.stringify({
      event: "listening",
      host: config.host,
      port: typeof address === "object" ? address?.port : config.port,
    }),
  );
  let closing = false;
  const shutdown = () => {
    if (closing) return;
    closing = true;
    http
      .close()
      .then(() => console.log(JSON.stringify({ event: "stopped" })))
      .catch(() => {
        process.exitCode = 1;
      });
  };
  process.on("SIGTERM", shutdown);
  process.on("SIGINT", shutdown);
  process.on("SIGHUP", () => {
    if (closing) return;
    try {
      capabilities.reload();
      console.log(
        JSON.stringify({
          event: "capabilities_reloaded",
          generation: capabilities.snapshot.generation,
        }),
      );
    } catch (error) {
      console.error(
        JSON.stringify({
          event: "capability_reload_failed",
          code: publicError(error).code,
        }),
      );
    }
  });
}

void main().catch((error: unknown) => {
  const safe = publicError(error);
  console.error(
    JSON.stringify({
      event: "startup_failed",
      code: safe.code,
      message: safe.message,
    }),
  );
  process.exitCode = 1;
});
