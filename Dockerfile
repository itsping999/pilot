# ---- Build stage ----
FROM rust:1.85-slim AS builder

RUN apt-get update && apt-get install -y pkg-config && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
COPY static/ static/

RUN cargo build --release --locked

# ---- Runtime stage ----
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*

RUN useradd -r -s /bin/false knowledgepilot
RUN mkdir -p /data && chown knowledgepilot:knowledgepilot /data

COPY --from=builder /app/target/release/knowledge-pilot /usr/local/bin/knowledge-pilot

USER knowledgepilot
WORKDIR /data

ENV KNOWLEDGE_PILOT_ADDR=0.0.0.0:8080
ENV KNOWLEDGE_PILOT_DB_PATH=/data/knowledge-pilot.db
ENV KNOWLEDGE_PILOT_UI_ENABLED=true
ENV RUST_LOG=info

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=3s --retries=3 \
    CMD curl -fsS http://localhost:8080/health || exit 1

ENTRYPOINT ["knowledge-pilot"]
