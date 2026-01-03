# venus-server

[![Crates.io](https://img.shields.io/crates/v/venus-server.svg)](https://crates.io/crates/venus-server)
[![Documentation](https://docs.rs/venus-server/badge.svg)](https://docs.rs/venus-server)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://github.com/ml-rust/venus/blob/main/LICENSE)

WebSocket server for Venus interactive notebooks.

## Overview

This crate provides the web server backend for Venus's interactive notebook interface:

- **Axum-based WebSocket server** - Real-time bidirectional communication
- **File watching** - Automatic reload on source changes
- **LSP integration** - Download and manage rust-analyzer
- **Embedded frontend** - Serves the web UI (optional)

This is an internal implementation crate used by `venus-cli`. Most users should use the CLI instead.

## Features

- `embedded-frontend` (default) - Embed the web UI in the binary

## Documentation

For complete documentation, visit:
- [Venus Repository](https://github.com/ml-rust/venus)
- [API Documentation](https://docs.rs/venus-server)

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](https://github.com/ml-rust/venus/blob/main/LICENSE) for details.
