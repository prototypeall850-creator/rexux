//! CLI login commands and their direct-user observability surfaces.
//!
//! The TUI path already installs a broader tracing stack with feedback, OpenTelemetry, and other
//! interactive-session layers. Direct `codex login` intentionally does less: it preserves the
//! existing stderr UX and adds only a small file-backed tracing layer for login-specific
//! targets. Keeping that setup local avoids pulling the TUI's session-oriented logging machinery
//! into a one-shot CLI command while still producing a durable `rexux-login.log` artifact that
//! support can request from users.
//!
//! This is a BYOK (bring your own key) build: the ChatGPT OAuth browser/device-code flows are
//! removed. Credentials are supplied as API keys (stdin pipe or provider `env_key`) and stored
//! in the file-backed credential store.

use rexux_config::types::AuthCredentialsStoreMode;
use rexux_core::config::Config;
use rexux_core::config::edit::ConfigEdit;
use rexux_core::config::edit::ConfigEditsBuilder;
use rexux_login::AuthKeyringBackendKind;
use rexux_login::AuthManager;
use rexux_login::AuthRouteConfig;
use rexux_login::is_workload_identity_selected;
use rexux_login::login_with_api_key;
use rexux_login::logout_with_revoke;
use rexux_protocol::auth::AuthMode;
use rexux_protocol::config_types::ForcedLoginMethod;
use rexux_utils_cli::CliConfigOverrides;
use std::fs::OpenOptions;
use std::io::IsTerminal;
use std::io::Read;
use std::path::Path;
use tracing_appender::non_blocking;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

const API_KEY_LOGIN_DISABLED_MESSAGE: &str =
    "API key login is disabled. Configure a different login method in your config.";
const LOGIN_SUCCESS_MESSAGE: &str = "Successfully logged in";
pub const NO_CHATGPT_LOGIN_MESSAGE: &str = "ChatGPT sign-in is not available in this BYOK build.\n\
Provide credentials with `codex login --with-api-key` (pipe the key to stdin) or set the API key\n\
env var of your provider (e.g. `export OPENAI_API_KEY=...`).\n\
To use another provider, add a [model_providers.<id>] block with `env_key` to ~/.rexux/config.toml\n\
and select it via `model_provider`.";

/// Installs a small file-backed tracing layer for direct `codex login` flows.
///
/// This deliberately duplicates a narrow slice of the TUI logging setup instead of reusing it
/// wholesale. The TUI stack includes session-oriented layers that are valuable for interactive
/// runs but unnecessary for a one-shot login command. Keeping the direct CLI path local lets this
/// command produce a durable `rexux-login.log` artifact without coupling it to the TUI's broader
/// telemetry and feedback initialization.
fn init_login_file_logging(config: &Config) -> Option<WorkerGuard> {
    let log_dir = match rexux_core::config::log_dir(config) {
        Ok(log_dir) => log_dir,
        Err(err) => {
            eprintln!("Warning: failed to resolve login log directory: {err}");
            return None;
        }
    };

    if let Err(err) = std::fs::create_dir_all(&log_dir) {
        eprintln!(
            "Warning: failed to create login log directory {}: {err}",
            log_dir.display()
        );
        return None;
    }

    let mut log_file_opts = OpenOptions::new();
    log_file_opts.create(true).append(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        log_file_opts.mode(0o600);
    }

    let log_path = log_dir.join("rexux-login.log");
    let log_file = match log_file_opts.open(&log_path) {
        Ok(log_file) => log_file,
        Err(err) => {
            eprintln!(
                "Warning: failed to open login log file {}: {err}",
                log_path.display()
            );
            return None;
        }
    };

    let (non_blocking, guard) = non_blocking(log_file);
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("rexux_cli=info,rexux_core=info,rexux_login=info"));
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .with_target(true)
        .with_ansi(false)
        .with_filter(env_filter);

    // Direct `codex login` otherwise relies on ephemeral stderr output.
    // Persist the same login targets to a file so failures can be inspected
    // without reproducing them through TUI or app-server.
    if let Err(err) = tracing_subscriber::registry().with(file_layer).try_init() {
        eprintln!(
            "Warning: failed to initialize login log file {}: {err}",
            log_path.display()
        );
        return None;
    }

    Some(guard)
}

async fn clear_existing_auth_before_login(
    rexux_home: &Path,
    auth_credentials_store_mode: AuthCredentialsStoreMode,
    auth_keyring_backend_kind: AuthKeyringBackendKind,
    auth_route_config: &AuthRouteConfig,
) {
    if let Err(err) = logout_with_revoke(
        rexux_home,
        auth_credentials_store_mode,
        auth_keyring_backend_kind,
        auth_route_config,
    )
    .await
    {
        tracing::warn!("failed to clear existing auth before login: {err}");
    }
}

pub async fn run_login_with_api_key(
    cli_config_overrides: CliConfigOverrides,
    api_key: String,
) -> ! {
    let config = load_config_or_exit(cli_config_overrides).await;
    let _login_log_guard = init_login_file_logging(&config);
    tracing::info!("starting api key login flow");

    if !config
        .auth_config()
        .is_login_method_allowed(ForcedLoginMethod::Api)
    {
        eprintln!("{API_KEY_LOGIN_DISABLED_MESSAGE}");
        std::process::exit(1);
    }

    match login_with_api_key(
        &config.rexux_home,
        &api_key,
        config.cli_auth_credentials_store_mode,
        config.auth_keyring_backend_kind(),
    ) {
        Ok(_) => {
            eprintln!("{LOGIN_SUCCESS_MESSAGE}");
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("Error logging in: {e}");
            std::process::exit(1);
        }
    }
}

pub fn read_api_key_from_stdin() -> String {
    read_stdin_secret(
        "--with-api-key expects the API key on stdin. Try piping it, e.g. `printenv OPENAI_API_KEY | codex login --with-api-key`.",
        "Reading API key from stdin...",
        "No API key provided via stdin.",
    )
}

fn read_stdin_secret(terminal_message: &str, reading_message: &str, empty_message: &str) -> String {
    let mut stdin = std::io::stdin();

    if stdin.is_terminal() {
        eprintln!("{terminal_message}");
        std::process::exit(1);
    }

    eprintln!("{reading_message}");

    let mut buffer = String::new();
    if let Err(err) = stdin.read_to_string(&mut buffer) {
        eprintln!("Failed to read stdin: {err}");
        std::process::exit(1);
    }

    let secret = buffer.trim().to_string();
    if secret.is_empty() {
        eprintln!("{empty_message}");
        std::process::exit(1);
    }

    secret
}

pub async fn run_login_status(cli_config_overrides: CliConfigOverrides) -> ! {
    let config = load_config_or_exit(cli_config_overrides).await;

    if is_workload_identity_selected() {
        match AuthManager::shared_from_config(&config, /*enable_rexux_api_key_env*/ false).await {
            Ok(_) => {
                eprintln!("Logged in using workload identity");
                std::process::exit(0);
            }
            Err(err) => {
                eprintln!("Error checking login status: {err}");
                std::process::exit(1);
            }
        }
    }

    let auth_config = config.auth_config();
    match auth_config
        .load_auth(/*enable_rexux_api_key_env*/ false)
        .await
    {
        Ok(Some(auth)) => match auth.auth_mode() {
            AuthMode::ApiKey => match auth.get_token() {
                Ok(api_key) => {
                    eprintln!("Logged in using an API key - {}", safe_format_key(&api_key));
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("Unexpected error retrieving API key: {e}");
                    std::process::exit(1);
                }
            },
            AuthMode::Chatgpt | AuthMode::ChatgptAuthTokens => {
                eprintln!("Logged in using ChatGPT");
                std::process::exit(0);
            }
            AuthMode::Headers => {
                unreachable!("header auth cannot be loaded from auth storage")
            }
            AuthMode::AgentIdentity => {
                eprintln!("Logged in using access token");
                std::process::exit(0);
            }
            AuthMode::PersonalAccessToken => {
                eprintln!("Logged in using personal access token");
                std::process::exit(0);
            }
            AuthMode::BedrockApiKey => {
                eprintln!("Logged in using Amazon Bedrock API key");
                std::process::exit(0);
            }
            AuthMode::BedrockAccessKeys => {
                eprintln!("Logged in using Amazon Bedrock AWS access keys");
                std::process::exit(0);
            }
        },
        Ok(None) => {
            eprintln!("Not logged in");
            std::process::exit(1);
        }
        Err(err) => {
            eprintln!("Error checking login status: {err}");
            std::process::exit(1);
        }
    }
}

pub async fn run_logout(cli_config_overrides: CliConfigOverrides) -> ! {
    let config = load_config_or_exit(cli_config_overrides).await;
    let auth_route_config = config.auth_route_config();

    let logged_out = match logout_with_revoke(
        &config.rexux_home,
        config.cli_auth_credentials_store_mode,
        config.auth_keyring_backend_kind(),
        &auth_route_config,
    )
    .await
    {
        Ok(logged_out) => logged_out,
        Err(err) => {
            eprintln!("Error logging out: {err}");
            std::process::exit(1);
        }
    };

    let cleared_bedrock_config =
        if let Some(paths) = ConfigEditsBuilder::bedrock_provider_config_paths_to_clear(&config) {
            let edits = paths
                .into_iter()
                .map(|segments| ConfigEdit::ClearPath { segments });
            if let Err(err) = ConfigEditsBuilder::for_config(&config)
                .with_edits(edits)
                .apply()
                .await
            {
                eprintln!("Error clearing Amazon Bedrock configuration after logout: {err}");
                std::process::exit(1);
            }
            true
        } else {
            false
        };

    if logged_out || cleared_bedrock_config {
        eprintln!("Successfully logged out");
    } else {
        eprintln!("Not logged in");
    }
    std::process::exit(0);
}

async fn load_config_or_exit(cli_config_overrides: CliConfigOverrides) -> Config {
    let cli_overrides = match cli_config_overrides.parse_overrides() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error parsing -c overrides: {e}");
            std::process::exit(1);
        }
    };

    match Config::load_with_cli_overrides(cli_overrides).await {
        Ok(config) => match config.auth_config().validate() {
            Ok(()) => config,
            Err(e) => {
                eprintln!("Error loading configuration: {e}");
                std::process::exit(1);
            }
        },
        Err(e) => {
            eprintln!("Error loading configuration: {e}");
            std::process::exit(1);
        }
    }
}

fn safe_format_key(key: &str) -> String {
    if key.len() <= 13 {
        return "***".to_string();
    }
    let prefix = &key[..8];
    let suffix = &key[key.len() - 5..];
    format!("{prefix}***{suffix}")
}

#[cfg(test)]
mod tests {
    use rexux_config::types::AuthCredentialsStoreMode;
    use rexux_login::AuthKeyringBackendKind;
    use rexux_login::load_auth_dot_json;
    use rexux_login::login_with_api_key;
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    use super::clear_existing_auth_before_login;
    use super::safe_format_key;

    #[tokio::test]
    async fn clears_existing_auth_before_login() {
        let rexux_home = tempdir().expect("create temporary Rexux home");
        login_with_api_key(
            rexux_home.path(),
            "sk-existing",
            AuthCredentialsStoreMode::File,
            AuthKeyringBackendKind::default(),
        )
        .expect("save existing auth");

        clear_existing_auth_before_login(
            rexux_home.path(),
            AuthCredentialsStoreMode::File,
            AuthKeyringBackendKind::default(),
            &rexux_login::test_support::transport_default_auth_route_config(),
        )
        .await;

        let auth = load_auth_dot_json(
            rexux_home.path(),
            AuthCredentialsStoreMode::File,
            AuthKeyringBackendKind::default(),
        )
        .expect("load auth after cleanup");
        assert_eq!(auth, None);
    }

    #[test]
    fn formats_long_key() {
        let key = "sk-proj-1234567890ABCDE";
        assert_eq!(safe_format_key(key), "sk-proj-***ABCDE");
    }

    #[test]
    fn short_key_returns_stars() {
        let key = "sk-proj-12345";
        assert_eq!(safe_format_key(key), "***");
    }
}
