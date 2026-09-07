use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use rexux_core::StartThreadOptions;
use rexux_core::config::Config;
use rexux_core::windows_sandbox::WindowsSandboxLevelExt;
use rexux_features::Feature;
use rexux_login::AuthManager;
use rexux_login::RexuxAuth;
use rexux_models_manager::bundled_models_response;
use rexux_models_manager::manager::StaticModelsManager;
use rexux_protocol::config_types::WindowsSandboxLevel;
use rexux_protocol::models::PermissionProfile;
use rexux_protocol::models::PermissionProfileSnapshot;
use rexux_protocol::openai_models::ModelsResponse;
use rexux_protocol::openai_models::ReasoningEffort;
use rexux_protocol::permissions::FileSystemAccessMode;
use rexux_protocol::permissions::FileSystemPath;
use rexux_protocol::permissions::FileSystemSandboxEntry;
use rexux_protocol::permissions::FileSystemSandboxPolicy;
use rexux_protocol::permissions::FileSystemSpecialPath;
use rexux_protocol::permissions::NetworkSandboxPolicy;
use rexux_protocol::protocol::AskForApproval;
use rexux_protocol::protocol::EnvironmentConfig;
use rexux_protocol::protocol::EnvironmentConfigState;
use rexux_protocol::protocol::InternalSessionSource;
use rexux_protocol::protocol::SessionSource;
use rexux_protocol::protocol::ThreadSource;
use core_test_support::responses;
use core_test_support::test_rexux::TestRexux;
use core_test_support::test_rexux::test_rexux;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::super::GuardianExtension;

async fn reviewer_test_codex() -> Result<TestRexux> {
    let server = responses::start_mock_server().await;
    test_rexux()
        .with_auth(RexuxAuth::create_dummy_chatgpt_auth_for_testing())
        .with_model_info_override("rexux-auto-review", |_| {})
        .with_model("gpt-5.5")
        .build_with_auto_env(&server)
        .await
}

async fn prepare_options(test: &TestRexux, parent_config: &Config) -> Result<StartThreadOptions> {
    let extension = GuardianExtension::new(Arc::downgrade(&test.thread_manager), ());
    Ok(extension
        .prepare_reviewer_options(
            parent_config,
            &test.codex.environment_selections().await,
            "gpt-5.5",
            /*parent_reasoning_effort*/ None,
            /*live_network_config*/ None,
        )
        .await?)
}

#[tokio::test]
async fn prepares_isolated_guardian_internal_session() -> Result<()> {
    let test = reviewer_test_codex().await?;
    let mut parent_config = test.config.clone();
    parent_config.developer_instructions = Some("parent developer instructions".to_string());
    parent_config.notify = Some(vec!["notify-parent".to_string()]);
    parent_config.include_apps_instructions = true;
    let isolated_features = [
        Feature::Apps,
        Feature::RexuxHooks,
        Feature::Plugins,
        Feature::RecommendedPlugins,
        Feature::TokenBudget,
        Feature::ToolSuggest,
    ];
    for feature in isolated_features {
        parent_config.features.enable(feature)?;
    }
    parent_config.mcp_servers.set(HashMap::from([(
        "parent-server".to_string(),
        serde_json::from_value(json!({ "command": "parent-mcp" }))?,
    )]))?;

    let options = prepare_options(&test, &parent_config).await?;

    assert_eq!(
        (options.session_source, options.thread_source),
        (
            Some(SessionSource::Internal(InternalSessionSource::Guardian)),
            Some(ThreadSource::GuardianReview),
        )
    );
    assert_eq!(options.config.developer_instructions, None);
    assert_eq!(options.config.notify, None);
    assert!(!options.config.include_apps_instructions);
    assert_eq!(options.config.mcp_servers.get(), &HashMap::new());
    for feature in isolated_features {
        assert!(
            !options.config.features.enabled(feature),
            "{}",
            feature.key()
        );
    }
    assert_eq!(options.config.ephemeral, parent_config.ephemeral);
    assert_eq!(
        (options.config.model, options.config.model_reasoning_effort),
        (
            Some("rexux-auto-review".to_string()),
            Some(ReasoningEffort::Low)
        )
    );
    assert_eq!(
        options.config.permissions.approval_policy.value(),
        AskForApproval::Never
    );
    Ok(())
}

#[tokio::test]
async fn read_only_permissions_preserve_parent_environments_and_denied_reads() -> Result<()> {
    let test = reviewer_test_codex().await?;
    let mut parent_config = test.config.clone();
    let parent_profile = PermissionProfile::from_runtime_permissions(
        &FileSystemSandboxPolicy::restricted(vec![
            FileSystemSandboxEntry::new(
                FileSystemPath::Special {
                    value: FileSystemSpecialPath::Root,
                },
                FileSystemAccessMode::Read,
            ),
            FileSystemSandboxEntry::new(
                FileSystemPath::GlobPattern {
                    pattern: "**/*.secret".to_string(),
                },
                FileSystemAccessMode::Deny,
            ),
            FileSystemSandboxEntry::new(
                FileSystemPath::Special {
                    value: FileSystemSpecialPath::project_roots(/*subpath*/ None),
                },
                FileSystemAccessMode::Write,
            ),
        ]),
        NetworkSandboxPolicy::Enabled,
    );
    parent_config
        .permissions
        .set_permission_profile(parent_profile.clone())?;

    let mut parent_environments = test.codex.environment_selections().await;
    let environment = parent_environments
        .first_mut()
        .expect("parent should have an execution environment");
    environment.config = EnvironmentConfigState::Ready(EnvironmentConfig {
        allow_login_shell: true,
        workspace_roots: environment.workspace_roots.clone(),
        permission_profile: PermissionProfileSnapshot::legacy(parent_profile.clone()),
        shell_environment_policy: Default::default(),
        windows_sandbox_level: WindowsSandboxLevel::from_config(&parent_config),
        windows_sandbox_private_desktop: parent_config.permissions.windows_sandbox_private_desktop,
        use_legacy_landlock: parent_config.features.use_legacy_landlock(),
        exec_policy: None,
        mcp_policy: None,
        network_policy: None,
        selected_capability_roots: Vec::new(),
    });
    let mut secondary = environment.clone();
    secondary.environment_id = "secondary".to_string();
    if let EnvironmentConfigState::Ready(config) = &mut secondary.config {
        config.allow_login_shell = false;
        config.permission_profile =
            PermissionProfileSnapshot::legacy(PermissionProfile::workspace_write());
    }
    parent_environments.push(secondary);
    let mut expected_environments = parent_environments.clone();
    for (environment, parent_profile) in expected_environments
        .iter_mut()
        .zip([parent_profile.clone(), PermissionProfile::workspace_write()])
    {
        if let EnvironmentConfigState::Ready(config) = &mut environment.config {
            config.permission_profile = PermissionProfileSnapshot::legacy(
                parent_profile
                    .intersect_with_read_only()
                    .expect("managed permissions should support read-only intersection"),
            );
        }
    }

    let extension = GuardianExtension::new(Arc::downgrade(&test.thread_manager), ());
    let options = extension
        .prepare_reviewer_options(
            &parent_config,
            &parent_environments,
            "gpt-5.5",
            /*parent_reasoning_effort*/ None,
            /*live_network_config*/ None,
        )
        .await?;

    assert_eq!(
        options.config.permissions.permission_profile(),
        &parent_profile
            .intersect_with_read_only()
            .expect("managed permissions should support read-only intersection")
    );
    assert_eq!(options.environments, Some(expected_environments));
    Ok(())
}

#[tokio::test]
async fn honors_parent_models_auto_review_override() -> Result<()> {
    let server = responses::start_mock_server().await;
    let test = test_rexux()
        .with_auth(RexuxAuth::create_dummy_chatgpt_auth_for_testing())
        .with_model_info_override("gpt-5.5", |model| {
            model.auto_review_model_override = Some("gpt-5.2".to_string());
        })
        .build_with_auto_env(&server)
        .await?;

    let options = prepare_options(&test, &test.config).await?;

    assert_eq!(options.config.model, Some("gpt-5.2".to_string()));
    Ok(())
}

#[tokio::test]
async fn falls_back_to_parent_model_and_effective_reasoning() -> Result<()> {
    let server = responses::start_mock_server().await;
    let mut parent_model = bundled_models_response()?
        .models
        .into_iter()
        .find(|model| model.slug == "gpt-5.5")
        .expect("bundled parent model should exist");
    parent_model
        .supported_reasoning_levels
        .retain(|effort| effort.effort != ReasoningEffort::Low);
    parent_model.default_reasoning_level = Some(ReasoningEffort::Medium);
    let auth = RexuxAuth::create_dummy_chatgpt_auth_for_testing();
    let auth_manager = AuthManager::from_auth_for_testing(auth.clone());
    let models_manager = StaticModelsManager::new(
        Some(auth_manager),
        ModelsResponse {
            models: vec![parent_model],
        },
    );
    let test = test_rexux()
        .with_auth(auth)
        .with_models_manager(Arc::new(models_manager))
        .with_model("gpt-5.5")
        .build_with_auto_env(&server)
        .await?;

    let extension = GuardianExtension::new(Arc::downgrade(&test.thread_manager), ());
    let options = extension
        .prepare_reviewer_options(
            &test.config,
            &test.codex.environment_selections().await,
            "gpt-5.5",
            Some(ReasoningEffort::XHigh),
            /*live_network_config*/ None,
        )
        .await?;

    assert_eq!(
        (options.config.model, options.config.model_reasoning_effort),
        (Some("gpt-5.5".to_string()), Some(ReasoningEffort::XHigh))
    );
    Ok(())
}
