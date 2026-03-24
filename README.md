# 🏰 Bastion

> High-Performance API Gateway written in Rust

![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Rust Version](https://img.shields.io/badge/rustc-1.75+-lightgray.svg)
![Build](https://img.shields.io/badge/build-passing-brightgreen)

<div align="center">
  <img src="assets/logo.png" alt="Bastion Logo" width="200"/>
</div>

Bastion is an ultra-fast, safe, and modular API Gateway / Reverse Proxy designed for modern microservice architectures. It provides dynamic routing, load balancing, multi-layer caching, rate limiting, and extensive observability through a built-in real-time dashboard and Telegram bot.

## ✨ Key Features

- **⚡ Blazing Fast**: Zero-cost abstractions, lock-free LRU cache, and zero-copy request streaming.
- **🛡️ Secure by Default**: JWT & API Key authentication, IP filtering, and automatic CORS management.
- **🚦 Intelligent Routing**: Radix-trie based O(k) path matching with dynamic parameters.
- **⚖️ Advanced Load Balancing**: Round Robin, Weighted, Least Connections, and Consistent Hashing.
- **🛡️ Resilience Patterns**: Circuit Breakers, Retries with Backoff, and Active/Passive Health Checks.
- **🤖 Built-in ChatOps**: Manage and monitor your gateway directly from Telegram.
- **📊 Real-Time Dashboard**: Beautiful, responsive SPA dashboard powered by WebSockets.
- **🔄 Hot-Reload**: Update routing and middleware configuration without dropping connections.

## 🏗️ Architecture

Bastion is built as a highly modular workspace with cleanly separated components:

- `bastion-core`: The high-performance HTTP proxy engine and middleware pipeline
- `bastion-cache`: Concurrent, lock-free sharded LRU response cache
- `bastion-config`: TOML-based configuration manager with hot-reload capabilities
- `bastion-metrics`: In-memory time-series store and Prometheus exporter
- `bastion-admin`: REST API for programmatic control
- `bastion-telegram`: Integrated alerting and command-line ChatOps
- `bastion-dashboard`: Real-time WebSocket-powered observability UI

## 🚀 Quick Start

### Prerequisites
- Rust 1.75+
- (Optional) Docker

### Building from Source

```bash
git clone https://github.com/Niainarisoa01/Bastion.git
cd Bastion
cargo build --release
```

### Running Bastion

1. Create a minimal configuration file:
   ```bash
   cp config/bastion.toml.example config/bastion.toml
   ```

2. Start the gateway:
   ```bash
   cargo run --release -- --config config/bastion.toml
   ```

3. Access the dashboard:
   Navigate to `http://localhost:8080/dashboard` (if enabled in config).

## 📖 Configuration

Bastion uses a TOML configuration file that supports hot-reloading for most sections.

```toml
[server]
listen = "0.0.0.0:8080"
admin_listen = "127.0.0.1:9090"

[[routes]]
path = "/api/users/*"
methods = ["GET", "POST"]
upstream = "user-service"
middlewares = ["rate_limit", "auth_jwt"]

[[upstreams]]
name = "user-service"
strategy = "round_robin"

[[upstreams.backends]]
url = "http://10.0.1.1:3001"
weight = 5
```

For full configuration details, see the [Documentation](docs/).

## 🤖 Telegram ChatOps

Bastion can be fully monitored and managed via Telegram. Simply configure your bot token and admin chat IDs in `bastion.toml`:

Commands available:
- `/status` - Global gateway health
- `/toproutes` - Most active routes
- `/toggle <backend>` - Safely drain or enable a backend
- And many more...

## 📈 Benchmarks

*(Benchmarks will be published here upon completion of Phase 6)*

## 🤝 Contributing

Contributions are welcome! Please read [CONTRIBUTING.md](CONTRIBUTING.md) for details on our code of conduct and the process for submitting pull requests.

## 📝 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
