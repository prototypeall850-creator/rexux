# rexux — AI coding agent for Termux/Android

`rexux` is a terminal-based AI coding agent that runs natively on Android via
Termux. It brings its own key (BYOK): point it at any Responses-API provider
and start coding from your phone.

> Ported from Codex CLI (Rust) v0.153.4 and fully rebranded. All behavior,
> TUI, and commands mirror the original — only the name, home directory
> (`~/.rexux`), and auth model changed.

## Install (one command, in Termux)

```sh
curl -fsSL https://raw.githubusercontent.com/prototypeall850-creator/rexux/main/install.sh | sh
```

This installs the `rexux` binary to `$PREFIX/bin`. To pin a version:

```sh
REXUX_VERSION=v0.1.0 curl -fsSL https://raw.githubusercontent.com/prototypeall850-creator/rexux/main/install.sh | sh
```

### Build from source (Termux)

```sh
pkg install rust git clang binutils pkg-config openssl
git clone https://github.com/prototypeall850-creator/rexux.git
cd rexux
cargo build --release -p rexux-cli
install -Dm755 target/release/rexux $PREFIX/bin/rexux
```

## Quick start

```sh
# 1. Provide an API key (OpenAI by default)
export OPENAI_API_KEY=sk-...

# ...or save it to ~/.rexux/auth.json
printenv OPENAI_API_KEY | rexux login --with-api-key

# 2. Run it
rexux                        # interactive TUI
rexux exec "explain this repo"   # non-interactive
rexux login status           # check auth
rexux logout                 # clear auth
```

## Use more providers (BYOK)

Add any Responses-API provider to `~/.rexux/config.toml`:

```toml
model = "deepseek/deepseek-chat-v3"
model_provider = "openrouter"

[model_providers.openrouter]
name = "OpenRouter"
base_url = "https://openrouter.ai/api/v1"
env_key = "OPENROUTER_API_KEY"
wire_api = "responses"
```

```sh
export OPENROUTER_API_KEY=...
rexux
```

Built-in providers: `openai`, `openrouter`, `litellm` (local gateway to 100+
Chat-Completions providers), `ollama`, `lmstudio`, `amazon-bedrock`.

> Providers that only expose Chat Completions (Groq, DeepSeek, Mistral, …)
> can be reached through a local [LiteLLM proxy](https://docs.litellm.ai/),
> which translates to the Responses API this build speaks.

## Commands

| Command | Description |
|---|---|
| `rexux` | Interactive TUI (chat, `/init`, `/model`, `/diff`, `/compact`, …) |
| `rexux exec "<prompt>"` | Non-interactive run, great for scripts |
| `rexux login --with-api-key` | Save API key from stdin |
| `rexux login status` / `rexux logout` | Check / clear auth |
| `rexux resume` | Resume a previous session |
| `rexux mcp` | Manage MCP servers |
| `rexux apply` | Apply a patch |
| `rexux doctor` | Diagnose environment issues |

No ChatGPT sign-in in this build — API keys only, by design.

## Layout

`~/.rexux/` holds everything, same structure as the original:

```
~/.rexux/
├── config.toml        # your settings + providers
├── auth.json          # saved API key (from `rexux login`)
├── keyring-store/     # file-backed credential store (0600 JSON)
├── sessions/          # session rollouts
├── history.jsonl      # command history
└── log/               # logs (rexux.log, rexux-login.log)
```

## Releases

Pushing a tag `v*` builds Android binaries (`aarch64` + `x86_64`) via GitHub
Actions and attaches them to the GitHub Release. The installer above picks the
matching asset automatically, and falls back to a source build when no asset
fits.

## License

See [LICENSE](./LICENSE) (inherited from the upstream project).
