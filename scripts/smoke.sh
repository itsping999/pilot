#!/usr/bin/env bash
set -euo pipefail
AGENT_PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$AGENT_PROJECT_ROOT"
npm run smoke
