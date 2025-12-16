# Smart Relay (Rust)

AI-assisted proxy/VPN orchestrator written in Rust with optional Ollama-powered configuration hints. Features a modern cyberpunk-themed GUI built with `egui` and cross-platform packaging support. The `software-example/` folder contains the legacy reference (v2rayN) for comparison.

## Quick Start

### CLI Mode

1. Install Rust (stable) and ensure Ollama is running locally or remotely.
2. Set environment variables as needed:
   - `OLLAMA_HOST` (default `http://127.0.0.1:11434`)
   - `OLLAMA_MODEL` (default `llama3`)
3. Run the control plane:
   ```bash
   cargo run --bin smart-relay -- serve --listen 127.0.0.1:8080
   ```
4. Ask the AI for a plan:
   ```bash
   cargo run --bin smart-relay -- propose --goal "low latency gaming to JP/KR"
   ```

### GUI Mode

1. Start the control plane (see CLI mode above)
2. Launch the GUI:
   ```bash
   cargo run --bin smart-relay-gui
   ```
3. The GUI connects to `http://127.0.0.1:8080` by default

## Building and Packaging

### Development Build
```bash
cargo build --release
```

### Multi-Platform Packaging

**Windows:**
```powershell
.\scripts\package-windows.ps1
```
Creates: `smart-relay-windows-x86_64-v0.1.0.zip`

**Linux:**
```bash
bash scripts/package-linux.sh
```
Creates: `smart-relay-linux-x86_64-v0.1.0.tar.gz`

**macOS:**
```bash
bash scripts/package-macos.sh
```
Creates: `Smart Relay.app` and optionally `.dmg`

## Layout
- `src/` — Rust sources (API, AI client, config store, adapters, GUI)
- `src/gui/` — egui-based cyberpunk GUI implementation
- `scripts/` — Multi-platform packaging scripts
- `docs/architecture.md` — architecture blueprint and roadmap
- `docs/book/` — LaTeX book covering networking to Smart Relay usage with TikZ diagrams
- `docs/dev_story.md` — Development history and lessons learned
- `software-example/` — upstream v2rayN reference implementation (C#)

## Features

- ✅ HTTP control plane API
- ✅ AI-powered configuration suggestions (Ollama)
- ✅ Cyberpunk-themed GUI with egui
- ✅ Cross-platform support (Windows, Linux, macOS)
- ✅ Multi-platform packaging scripts
- 🚧 Proxy adapter implementations (xray/sing-box planned)
- 🚧 Real-time health monitoring
- 🚧 Advanced routing rules

## Status
Core functionality implemented. GUI available with basic features. Adapters are placeholders; planned next steps include xray/sing-box integration, probe-based health scoring, and enhanced GUI features.

