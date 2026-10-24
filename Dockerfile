FROM node:26-bookworm-slim AS builder
WORKDIR /app
COPY package.json package-lock.json tsconfig.json ./
RUN npm ci --ignore-scripts --no-audit --no-fund
COPY src/ src/
RUN npm run build && npm prune --omit=dev --ignore-scripts --no-audit --no-fund

FROM node:26-bookworm-slim
ENV NODE_ENV=production
WORKDIR /app
COPY --from=builder /app/package.json /app/package-lock.json ./
COPY --from=builder /app/node_modules/ node_modules/
COPY --from=builder /app/dist/src/ dist/src/
COPY admin/ admin/
COPY skills/ skills/
RUN mkdir -p /app/data/agent /app/config && chown -R node:node /app/data /app/config
USER node
ENV AGENT_ADDR=0.0.0.0:8080
ENV AGENT_DATA_DIR=/app/data/agent
ENV AGENT_SKILLS_DIR=/app/skills
ENV AGENT_MCP_CONFIG_PATH=/app/config/mcp.json
ENV AGENT_ADMIN_ADDR=0.0.0.0:8082
ENV AGENT_LLM_PROVIDER=none
EXPOSE 8080
EXPOSE 8082
HEALTHCHECK --interval=30s --timeout=3s --retries=3 \
  CMD node -e 'fetch("http://127.0.0.1:8080/health").then(r=>{if(!r.ok)process.exit(1)}).catch(()=>process.exit(1))'
CMD ["node", "dist/src/main.js"]
