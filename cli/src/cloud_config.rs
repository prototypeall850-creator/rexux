use anyhow::Context;
use anyhow::Result;
use rexux_cloud_config::cloud_config_bundle_loader_for_storage;
use rexux_config::CloudConfigBundleLoader;
use rexux_config::ConfigLoadOptions;
use rexux_core::config::Config;
use rexux_core::config::ConfigBuilder;
use rexux_core::config::LoaderOverrides;
use rexux_core::config::bootstrap_auth_config;
use rexux_core::config::find_rexux_home;
use rexux_core::config::load_config_toml_with_layer_stack;
use rexux_utils_absolute_path::AbsolutePathBuf;
use rexux_utils_cli::CliConfigOverrides;

pub(crate) async fn load_config(
    config_overrides: &CliConfigOverrides,
    loader_overrides: LoaderOverrides,
) -> Result<Config> {
    config_builder(config_overrides, loader_overrides)
        .await?
        .build()
        .await
        .context("failed to load configuration")
}

pub(crate) async fn config_builder(
    config_overrides: &CliConfigOverrides,
    loader_overrides: LoaderOverrides,
) -> Result<ConfigBuilder> {
    let cli_overrides = config_overrides
        .parse_overrides()
        .map_err(anyhow::Error::msg)?;
    let rexux_home = find_rexux_home().context("failed to resolve REXUX_HOME")?;
    let cwd = AbsolutePathBuf::current_dir().context("failed to resolve current directory")?;
    let bootstrap_config = load_config_toml_with_layer_stack(
        rexux_home.as_path(),
        Some(&cwd),
        cli_overrides.clone(),
        ConfigLoadOptions {
            loader_overrides: loader_overrides.clone(),
            strict_config: false,
            cloud_config_bundle: CloudConfigBundleLoader::default(),
        },
    )
    .await
    .context("failed to load bootstrap configuration")?;
    let cloud_config_bundle = cloud_config_bundle_loader_for_storage(
        bootstrap_auth_config(rexux_home.as_path(), &bootstrap_config)
            .context("failed to resolve cloud configuration authentication")?,
        /*enable_rexux_api_key_env*/ false,
    )
    .await
    .context("failed to initialize cloud configuration authentication")?;

    Ok(ConfigBuilder::default()
        .rexux_home(rexux_home.to_path_buf())
        .cli_overrides(cli_overrides)
        .loader_overrides(loader_overrides)
        .cloud_config_bundle(cloud_config_bundle)
        .fallback_cwd(Some(cwd.to_path_buf())))
}
