# Smart Relay (Rust) - Architecture Blueprint

## Goals
- AI-assisted proxy/VPN orchestration with minimal manual tuning.
- Native Rust core for safety and portability (Windows/macOS/Linux/BSD).
- Extensible adapters for xray/sing-box/other cores and future WASM plugins.
- Control plane with HTTP+gRPC friendly API for GUI and automation.

## High-Level Components
- **CLI / Daemon** (`src/main.rs`): starts control plane or runs one-shot AI proposals.
- **Config Store** (`src/config.rs`): persistent TOML config with endpoints and routing rules.
- **AI Service** (`src/ai.rs`): talks to local/remote Ollama to propose profiles, synthesize rules, and explain decisions.
- **Adapters** (`src/proxy/mod.rs`): abstraction over proxy engines. Current placeholder; future concrete adapters for xray and sing-box.
- **Control Plane API** (`src/api.rs`): Axum HTTP surface for health, config management, AI suggestions.
- **Telemetry** (`src/telemetry.rs`): tracing-based logging with env-driven filters.

## Data Flow
1. User or GUI calls `/ai/propose` with a goal (e.g., "streaming, low latency EU").
2. `AiClient` builds prompt with current config snapshot and hits Ollama `/api/generate`.
3. Plan is returned and, after user approval, translated into `SmartConfig` updates.
4. Config is serialized to TOML and fed into concrete proxy adapter (xray/sing-box).
5. Probes feed metrics back for AI refinement (closed-loop planned).

## Roadmap Suggestions
- Implement xray and sing-box adapters with hot-reload.
- Add traffic probes (QUIC/TCP ping) and health scoring.
- Add rules optimizer that uses AI to merge/simplify rule sets.
- Expose gRPC+OpenAPI for GUI clients.
- Package installers and system service units for all major OS targets.

## Ollama Expectations
- Env vars: `OLLAMA_HOST` (default `http://127.0.0.1:11434`), `OLLAMA_MODEL` (default `llama3`).
- Prompts stay concise; responses are consumed as text plans today, structured JSON planned later.

