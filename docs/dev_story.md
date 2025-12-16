# Smart Relay Development Story (Living Document)

This note tracks key decisions and lessons as the project grows. Keep it brief and append-only.

## Recent Steps
- Bootstrapped Rust CLI/daemon with Axum control plane and Ollama client.
- Added AI-driven proposal endpoint plus config validation.
- Enriched book with Rust primer and deployment/ops guidance.
- Added AI health endpoint, router extractor for tests, and integration tests for health/config.

## Lessons Learned
- Keep prompts concise and structured; avoid unbounded AI output.
- Validate configs before applying to cores; surface clear HTTP errors.
- Cross-platform concerns: prefer rustls, avoid platform-specific syscalls in core.

## Near-Term Plans
- Implement real adapters for xray/sing-box with hot-reload hooks.
- Add health probes and scoring loop, expose metrics.
- Write integration tests for API routes and schema validation of AI output.

