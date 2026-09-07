# Codex CLI — Port Termux (BYOK Edition)

Port dari Codex CLI Rust v0.153.4 (`codex-rs`) untuk di-build & jalan di Termux/Android,
direbrand penuh menjadi **rexux** (`~/.rexux`, binary `rexux`),
dengan sistem autentikasi **BYOK (Bring Your Own Key)** — tanpa login ChatGPT, tanpa OS keyring.

Tampilan, TUI, slash command, dan seluruh UX **identik dengan Codex CLI asli** — yang berubah
hanya target build (Android) dan lapisan auth.

---

## 1. Build di Termux

```bash
pkg update
pkg install rust git clang binutils pkg-config openssl

# clone / copy folder repo ini ke HP, lalu:
cargo build --release -p rexux-cli

# binary hasil build:
install -Dm755 target/release/rexux $PREFIX/bin/rexux
rexux --version
```

Catatan:
- Build native di Termux otomatis memakai host triple `aarch64-unknown-linux-android`,
  sehingga semua `cfg(target_os = "android")` bawaan upstream aktif (clipboard dinonaktifkan,
  native certs Android, dsb).
- Build butuh RAM besar & waktu lama (puluhan menit). Kalau sering OOM, build di PC lalu
  copy binary, atau tambah swap.
- Binary `rexux` tunggal ini mencakup semua subcommand asli: `rexux`, `rexux exec`,
  `rexux login`, `rexux logout`, `rexux resume`, `rexux mcp`, `rexux apply`, `rexux doctor`, dst.

### Alternatif: cross-compile dari PC

```bash
# di PC (Arch/Manjaro)
rustup target add aarch64-linux-android
# pakai NDK + cargo-ndk, lalu:
cargo ndk -t arm64-v8a build --release -p rexux-cli
# copy target/aarch64-linux-android/release/rexux ke HP
```

---

## 2. Konfigurasi BYOK

Semua kredensial dijalankan lewat API key. Tiga cara:

### a. Env var (paling gampang)

```bash
export OPENAI_API_KEY=sk-...        # provider openai bawaan
rexux
```

### b. `rexux login --with-api-key` (tersimpan di auth.json)

```bash
printenv OPENAI_API_KEY | rexux login --with-api-key
rexux login status   # cek status
rexux logout         # hapus
```

### c. Provider custom di `~/.rexux/config.toml`

```toml
model = "deepseek/deepseek-chat-v3"
model_provider = "openrouter"

[model_providers.openrouter]
name = "OpenRouter"
base_url = "https://openrouter.ai/api/v1"
env_key = "OPENROUTER_API_KEY"
wire_api = "responses"

[model_providers.litellm]
name = "LiteLLM Proxy"
base_url = "http://localhost:4000/v1"
wire_api = "responses"

[model_providers.groq]
name = "Groq (via LiteLLM)"
base_url = "http://localhost:4000/groq/v1"
env_key = "GROQ_API_KEY"
wire_api = "responses"
```

Lalu set env key-nya: `export OPENROUTER_API_KEY=...`

### Provider bawaan yang tersedia

| ID | Keterangan | Env key |
|---|---|---|
| `openai` | OpenAI (default) | `OPENAI_API_KEY` |
| `openrouter` | OpenRouter (ratusan model) | `OPENROUTER_API_KEY` |
| `litellm` | LiteLLM proxy lokal (gateway ke 100+ provider Chat-Completions) | — |
| `ollama` | Ollama lokal (http://localhost:11434/v1) | — |
| `lmstudio` | LM Studio lokal (http://localhost:1234/v1) | — |
| `amazon-bedrock` | AWS Bedrock (kredensial AWS) | AWS |

> **Penting**: v0.153.4 upstream hanya mendukung wire API **Responses** (`wire_api = "responses"`);
> wire Chat Completions sudah dihapus upstream (lihat `OLLAMA_CHAT_PROVIDER_REMOVED_ERROR`).
> Provider yang hanya punya endpoint Chat Completions (Groq, DeepSeek, Mistral, dsb.) dapat
> dipakai dengan menengankan lewat **LiteLLM proxy** yang menerjemahkan ke Responses API.

---

## 3. Apa yang diubah dari upstream

### Auth: BYOK murni
- **Crate `login`**: seluruh mesin OAuth ChatGPT dibuang — `server.rs` (local OAuth callback
  server), `pkce.rs`, `device_code_auth.rs`, `success_page.rs` + test-nya. Yang dipertahankan:
  `AuthManager`, `CodexAuth`, `login_with_api_key`, `login_with_access_token`, storage auth.json,
  Bedrock login, workload identity.
- **`rexux login` CLI**: kini hanya `--with-api-key`, `status`, dan `logout`. `rexux login`
  tanpa argumen menampilkan panduan BYOK.
- **TUI onboarding**: opsi "Sign in with ChatGPT" dan "Device Code" dihilangkan dari UI;
  layar pick-mode kini langsung menawarkan entry API key (+ Bedrock). State ChatGPT dipertahankan
  di kode agar auth.json lama (dari desktop) tetap terbaca.
- **`rexux-keyring-store`**: di-rewrite **file-backed** — kredensial disimpan sebagai JSON
  0600 di `$CODEX_HOME/keyring-store/`, bukan OS keyring (dbus/secret-service/kernel-keyutils
  tidak tersedia/diandalkan di Android). Trait & mock-nya dipertahankan sehingga 30+ pemakai
  (secrets, rmcp-client, rexux-mcp, login) tidak berubah. Crate `keyring` dihapus total dari
  dependency tree.

### Sandbox (Android)
- `rexux-linux-sandbox` (Landlock/seccomp) memang tidak dikompilasi untuk target Android
  (cfg `target_os = "linux"` tidak match). Konfigurasikan `approval_policy = "on-request"`
  dan biarkan sandbox tidak ditegakkan; untuk perilaku paling mirip desktop di HP, tambahkan
  `sandbox_mode = "danger-full-access"` di `~/.rexux/config.toml` bila perlu.

### Trim workspace (crate dibuang dari members)
- `realtime-webrtc`, `voice-host` — voice mode (tidak dirujuk binary utama)
- `v8-poc`, `code-mode-runtime`, `code-mode-host` — runtime JS eksperimental (paling berat;
  tidak dirujuk binary utama)
- Folder non-Rust dari upstream tidak dibawa: `codex-cli/` (npm wrapper), `sdk/`, `bazel/`,
  `patches/`, `tools/`, `scripts/`, `third_party/`

### Yang sengaja TIDAK diubah
- TUI (`tui/`) — seluruh look & feel, chat widget, slash command, diff viewer, keybinding.
- `core/`, `protocol/`, `config/` — agent loop, tools (`shell`, `apply_patch`, web-search),
  schema `~/.rexux/config.toml`, session rollout (format sama dengan desktop).
- MCP (`mcp-server`, `rmcp-client`, `rexux-mcp`) — termasuk OAuth MCP server yang berjalan
  mandiri via tiny_http.
- `~/.rexux/` di HP: `config.toml`, `auth.json`, `sessions/`, `history.jsonl`, `log/`,
  `keyring-store/` — struktur sama dengan desktop.

---

## 4. Struktur repo ini

```
repo ini                 # seluruh workspace codex-rs (di-trim dari members, direbrand rexux)
├── Cargo.toml           # workspace manifest (members di-trim)
├── cli/                 # binary `rexux` (entry utama)
├── tui/                 # UI interaktif (identik asli)
├── core/                # agent loop + tools
├── login/               # auth API-key (OAuth dibuang)
├── keyring-store/       # credential store file-backed 0600 (rewrite)
├── model-provider-info/ # + provider openrouter & litellm
└── ...                  # crate pendukung lainnya
```

## 5. Status validasi (PC x86_64, Rust 1.98.1)

- `cargo check --workspace --tests` → **exit 0, tanpa error** (per 2026-09-07, setelah rebrand rexux).
- Binary `rexux` (`-p rexux-cli --tests`): bersih.
- `rexux-login`, `rexux-app-server`, `rexux-tui --lib`: bersih.
- Warning tersisa (~36, mayoritas pre-existing di upstream): hanya `unused import` /
  dead-code hasil trim + `future-incompat proc-macro-error2` dari upstream.
- Target Android (`aarch64-linux-android`) belum di-check di sini — butuh NDK di HP/CI;
  semua `cfg(target_os = "android")` upstream dibiarkan utuh agar otomatis aktif.

## 6. Build flags yang berguna

```bash
cargo build --release -p rexux-cli            # binary utama
cargo build --release -p rexux-tui            # (opsional) binary rexux-tui terpisah
cargo build --release -p rexux-exec           # (opsional) binary exec terpisah
```
