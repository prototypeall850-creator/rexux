use crate::bespoke_event_handling::apply_bespoke_event_handling;
use crate::command_exec::CommandExecManager;
use crate::command_exec::StartCommandExecParams;
use crate::config_manager::ConfigManager;
use crate::error_code::INPUT_TOO_LARGE_ERROR_CODE;
use crate::error_code::invalid_params;
use crate::models::supported_models;
use crate::outgoing_message::ConnectionId;
use crate::outgoing_message::ConnectionRequestId;
use crate::outgoing_message::OutgoingMessageSender;
use crate::outgoing_message::RequestContext;
use crate::outgoing_message::ThreadScopedOutgoingMessageSender;
use crate::skills_watcher::SkillsWatcher;
use crate::thread_status::ThreadWatchManager;
use crate::thread_status::resolve_thread_status;
use chrono::Duration as ChronoDuration;
use chrono::SecondsFormat;
use rexux_analytics::AnalyticsEventsClient;
use rexux_analytics::AnalyticsJsonRpcError;
use rexux_analytics::InputError;
use rexux_analytics::TurnSteerRequestError;
use rexux_app_server_protocol::Account;
use rexux_app_server_protocol::AccountLoginCompletedNotification;
use rexux_app_server_protocol::AccountTokenUsageDailyBucket;
use rexux_app_server_protocol::AccountTokenUsageSummary;
use rexux_app_server_protocol::AccountUpdatedNotification;
use rexux_app_server_protocol::AddCreditsNudgeCreditType;
use rexux_app_server_protocol::AddCreditsNudgeEmailStatus;
use rexux_app_server_protocol::AdditionalContextEntry;
use rexux_app_server_protocol::AdditionalContextKind;
use rexux_app_server_protocol::AppListUpdatedNotification;
use rexux_app_server_protocol::AppSummary;
use rexux_app_server_protocol::AppTemplateSummary;
use rexux_app_server_protocol::AppTemplateUnavailableReason;
use rexux_app_server_protocol::AppsInstalledParams;
use rexux_app_server_protocol::AppsInstalledResponse;
use rexux_app_server_protocol::AppsListParams;
use rexux_app_server_protocol::AppsListResponse;
use rexux_app_server_protocol::AppsReadParams;
use rexux_app_server_protocol::AppsReadResponse;
use rexux_app_server_protocol::AskForApproval;
use rexux_app_server_protocol::AuthMode;
use rexux_app_server_protocol::CancelLoginAccountParams;
use rexux_app_server_protocol::CancelLoginAccountResponse;
use rexux_app_server_protocol::CancelLoginAccountStatus;
use rexux_app_server_protocol::ClientInfo;
use rexux_app_server_protocol::ClientRequest;
use rexux_app_server_protocol::ClientResponsePayload;
use rexux_app_server_protocol::RexuxErrorInfo;
use rexux_app_server_protocol::CollaborationModeListParams;
use rexux_app_server_protocol::CollaborationModeListResponse;
use rexux_app_server_protocol::CommandExecParams;
use rexux_app_server_protocol::CommandExecResizeParams;
use rexux_app_server_protocol::CommandExecTerminateParams;
use rexux_app_server_protocol::CommandExecWriteParams;
use rexux_app_server_protocol::ConfigWarningNotification;
use rexux_app_server_protocol::ConsumeAccountRateLimitResetCreditOutcome;
use rexux_app_server_protocol::ConsumeAccountRateLimitResetCreditParams;
use rexux_app_server_protocol::ConsumeAccountRateLimitResetCreditResponse;
use rexux_app_server_protocol::ConversationGitInfo;
use rexux_app_server_protocol::ConversationSummary;
use rexux_app_server_protocol::DeprecationNoticeNotification;
use rexux_app_server_protocol::DynamicToolFunctionSpec;
use rexux_app_server_protocol::DynamicToolNamespaceTool;
use rexux_app_server_protocol::DynamicToolSpec;
use rexux_app_server_protocol::EnvironmentAddParams;
use rexux_app_server_protocol::EnvironmentAddResponse;
use rexux_app_server_protocol::EnvironmentInfoParams;
use rexux_app_server_protocol::EnvironmentInfoResponse;
use rexux_app_server_protocol::EnvironmentShellInfo;
use rexux_app_server_protocol::EnvironmentStatusKind;
use rexux_app_server_protocol::EnvironmentStatusParams;
use rexux_app_server_protocol::EnvironmentStatusResponse;
use rexux_app_server_protocol::ExperimentalFeature as ApiExperimentalFeature;
use rexux_app_server_protocol::ExperimentalFeatureListParams;
use rexux_app_server_protocol::ExperimentalFeatureListResponse;
use rexux_app_server_protocol::ExperimentalFeatureStage as ApiExperimentalFeatureStage;
use rexux_app_server_protocol::FeedbackUploadParams;
use rexux_app_server_protocol::FeedbackUploadResponse;
use rexux_app_server_protocol::GetAccountParams;
use rexux_app_server_protocol::GetAccountRateLimitsResponse;
use rexux_app_server_protocol::GetAccountResponse;
use rexux_app_server_protocol::GetAccountTokenUsageParams;
use rexux_app_server_protocol::GetAccountTokenUsageResponse;
use rexux_app_server_protocol::GetAuthStatusParams;
use rexux_app_server_protocol::GetAuthStatusResponse;
use rexux_app_server_protocol::GetConversationSummaryParams;
use rexux_app_server_protocol::GetConversationSummaryResponse;
use rexux_app_server_protocol::GetWorkspaceMessagesResponse;
use rexux_app_server_protocol::GitDiffToRemoteParams;
use rexux_app_server_protocol::GitDiffToRemoteResponse;
use rexux_app_server_protocol::GitInfo as ApiGitInfo;
use rexux_app_server_protocol::HookHandlerMetadata;
use rexux_app_server_protocol::HookMetadata;
use rexux_app_server_protocol::HooksListParams;
use rexux_app_server_protocol::HooksListResponse;
use rexux_app_server_protocol::InitializeParams;
use rexux_app_server_protocol::InitializeResponse;
use rexux_app_server_protocol::InstalledApp;
use rexux_app_server_protocol::JSONRPCErrorError;
use rexux_app_server_protocol::ListMcpServerStatusParams;
use rexux_app_server_protocol::ListMcpServerStatusResponse;
use rexux_app_server_protocol::LoginAccountParams;
use rexux_app_server_protocol::LoginAccountResponse;
use rexux_app_server_protocol::LoginApiKeyParams;
use rexux_app_server_protocol::LogoutAccountResponse;
use rexux_app_server_protocol::MarketplaceAddParams;
use rexux_app_server_protocol::MarketplaceAddResponse;
use rexux_app_server_protocol::MarketplaceInterface;
use rexux_app_server_protocol::MarketplaceRemoveParams;
use rexux_app_server_protocol::MarketplaceRemoveResponse;
use rexux_app_server_protocol::MarketplaceUpgradeErrorInfo;
use rexux_app_server_protocol::MarketplaceUpgradeParams;
use rexux_app_server_protocol::MarketplaceUpgradeResponse;
use rexux_app_server_protocol::McpResourceReadParams;
use rexux_app_server_protocol::McpResourceReadResponse;
use rexux_app_server_protocol::McpServerOauthClientRegistration;
use rexux_app_server_protocol::McpServerOauthLoginCompletedNotification;
use rexux_app_server_protocol::McpServerOauthLoginParams;
use rexux_app_server_protocol::McpServerOauthLoginResponse;
use rexux_app_server_protocol::McpServerRefreshResponse;
use rexux_app_server_protocol::McpServerStatus;
use rexux_app_server_protocol::McpServerStatusDetail;
use rexux_app_server_protocol::McpServerToolCallParams;
use rexux_app_server_protocol::McpServerToolCallResponse;
use rexux_app_server_protocol::MemoryResetResponse;
use rexux_app_server_protocol::MockExperimentalMethodParams;
use rexux_app_server_protocol::MockExperimentalMethodResponse;
use rexux_app_server_protocol::ModelListParams;
use rexux_app_server_protocol::ModelListResponse;
use rexux_app_server_protocol::PermissionProfileListParams;
use rexux_app_server_protocol::PermissionProfileListResponse;
use rexux_app_server_protocol::PermissionProfileSummary;
use rexux_app_server_protocol::PluginDetail;
use rexux_app_server_protocol::PluginInstallParams;
use rexux_app_server_protocol::PluginInstallResponse;
use rexux_app_server_protocol::PluginInstalledParams;
use rexux_app_server_protocol::PluginInstalledResponse;
use rexux_app_server_protocol::PluginInterface;
use rexux_app_server_protocol::PluginListMarketplaceKind;
use rexux_app_server_protocol::PluginListParams;
use rexux_app_server_protocol::PluginListResponse;
use rexux_app_server_protocol::PluginMarketplaceEntry;
use rexux_app_server_protocol::PluginReadParams;
use rexux_app_server_protocol::PluginReadResponse;
use rexux_app_server_protocol::PluginShareCheckoutParams;
use rexux_app_server_protocol::PluginShareCheckoutResponse;
use rexux_app_server_protocol::PluginShareContext;
use rexux_app_server_protocol::PluginShareDeleteParams;
use rexux_app_server_protocol::PluginShareDeleteResponse;
use rexux_app_server_protocol::PluginShareDiscoverability;
use rexux_app_server_protocol::PluginShareListItem;
use rexux_app_server_protocol::PluginShareListParams;
use rexux_app_server_protocol::PluginShareListResponse;
use rexux_app_server_protocol::PluginSharePrincipal;
use rexux_app_server_protocol::PluginSharePrincipalType;
use rexux_app_server_protocol::PluginShareSaveParams;
use rexux_app_server_protocol::PluginShareSaveResponse;
use rexux_app_server_protocol::PluginShareTarget;
use rexux_app_server_protocol::PluginShareUpdateDiscoverability;
use rexux_app_server_protocol::PluginShareUpdateTargetsParams;
use rexux_app_server_protocol::PluginShareUpdateTargetsResponse;
use rexux_app_server_protocol::PluginSkillReadParams;
use rexux_app_server_protocol::PluginSkillReadResponse;
use rexux_app_server_protocol::PluginSource;
use rexux_app_server_protocol::PluginSummary;
use rexux_app_server_protocol::PluginUninstallParams;
use rexux_app_server_protocol::PluginUninstallResponse;
use rexux_app_server_protocol::RateLimitResetCredit;
use rexux_app_server_protocol::RateLimitResetCreditStatus;
use rexux_app_server_protocol::RateLimitResetCreditsSummary;
use rexux_app_server_protocol::RateLimitResetType;
use rexux_app_server_protocol::RequestId;
use rexux_app_server_protocol::ReviewDelivery as ApiReviewDelivery;
use rexux_app_server_protocol::ReviewStartParams;
use rexux_app_server_protocol::ReviewStartResponse;
use rexux_app_server_protocol::ReviewTarget as ApiReviewTarget;
use rexux_app_server_protocol::SandboxMode;
use rexux_app_server_protocol::SendAddCreditsNudgeEmailParams;
use rexux_app_server_protocol::SendAddCreditsNudgeEmailResponse;
use rexux_app_server_protocol::ServerNotification;
use rexux_app_server_protocol::ServerRequestResolvedNotification;
use rexux_app_server_protocol::SkillSummary;
use rexux_app_server_protocol::SkillsConfigWriteParams;
use rexux_app_server_protocol::SkillsConfigWriteResponse;
use rexux_app_server_protocol::SkillsExtraRootsSetParams;
use rexux_app_server_protocol::SkillsExtraRootsSetResponse;
use rexux_app_server_protocol::SkillsListParams;
use rexux_app_server_protocol::SkillsListResponse;
use rexux_app_server_protocol::SortDirection;
use rexux_app_server_protocol::Thread;
use rexux_app_server_protocol::ThreadApproveGuardianDeniedActionParams;
use rexux_app_server_protocol::ThreadApproveGuardianDeniedActionResponse;
use rexux_app_server_protocol::ThreadArchiveParams;
use rexux_app_server_protocol::ThreadArchiveResponse;
use rexux_app_server_protocol::ThreadArchivedNotification;
use rexux_app_server_protocol::ThreadBackgroundTerminal;
use rexux_app_server_protocol::ThreadBackgroundTerminalsCleanParams;
use rexux_app_server_protocol::ThreadBackgroundTerminalsCleanResponse;
use rexux_app_server_protocol::ThreadBackgroundTerminalsListParams;
use rexux_app_server_protocol::ThreadBackgroundTerminalsListResponse;
use rexux_app_server_protocol::ThreadBackgroundTerminalsTerminateParams;
use rexux_app_server_protocol::ThreadBackgroundTerminalsTerminateResponse;
use rexux_app_server_protocol::ThreadClosedNotification;
use rexux_app_server_protocol::ThreadCompactStartParams;
use rexux_app_server_protocol::ThreadCompactStartResponse;
use rexux_app_server_protocol::ThreadDecrementElicitationParams;
use rexux_app_server_protocol::ThreadDecrementElicitationResponse;
use rexux_app_server_protocol::ThreadDeleteParams;
use rexux_app_server_protocol::ThreadDeleteResponse;
use rexux_app_server_protocol::ThreadDeletedNotification;
use rexux_app_server_protocol::ThreadForkParams;
use rexux_app_server_protocol::ThreadForkResponse;
use rexux_app_server_protocol::ThreadGoal;
use rexux_app_server_protocol::ThreadGoalClearParams;
use rexux_app_server_protocol::ThreadGoalClearResponse;
use rexux_app_server_protocol::ThreadGoalClearedNotification;
use rexux_app_server_protocol::ThreadGoalGetParams;
use rexux_app_server_protocol::ThreadGoalGetResponse;
use rexux_app_server_protocol::ThreadGoalSetParams;
use rexux_app_server_protocol::ThreadGoalSetResponse;
use rexux_app_server_protocol::ThreadGoalStatus;
use rexux_app_server_protocol::ThreadGoalUpdatedNotification;
use rexux_app_server_protocol::ThreadHistoryBuilder;
#[cfg(test)]
use rexux_app_server_protocol::ThreadHistoryMode;
use rexux_app_server_protocol::ThreadIncrementElicitationParams;
use rexux_app_server_protocol::ThreadIncrementElicitationResponse;
use rexux_app_server_protocol::ThreadInjectItemsParams;
use rexux_app_server_protocol::ThreadInjectItemsResponse;
use rexux_app_server_protocol::ThreadItem;
use rexux_app_server_protocol::ThreadItemEntry;
use rexux_app_server_protocol::ThreadItemsListParams;
use rexux_app_server_protocol::ThreadItemsListResponse;
use rexux_app_server_protocol::ThreadListCwdFilter;
use rexux_app_server_protocol::ThreadListParams;
use rexux_app_server_protocol::ThreadListResponse;
use rexux_app_server_protocol::ThreadLoadedListParams;
use rexux_app_server_protocol::ThreadLoadedListResponse;
use rexux_app_server_protocol::ThreadMemoryModeSetParams;
use rexux_app_server_protocol::ThreadMemoryModeSetResponse;
use rexux_app_server_protocol::ThreadMetadataGitInfoUpdateParams;
use rexux_app_server_protocol::ThreadMetadataUpdateParams;
use rexux_app_server_protocol::ThreadMetadataUpdateResponse;
use rexux_app_server_protocol::ThreadNameUpdatedNotification;
use rexux_app_server_protocol::ThreadProjectUpdatedNotification;
use rexux_app_server_protocol::ThreadReadParams;
use rexux_app_server_protocol::ThreadReadResponse;
use rexux_app_server_protocol::ThreadRealtimeAppendAudioParams;
use rexux_app_server_protocol::ThreadRealtimeAppendAudioResponse;
use rexux_app_server_protocol::ThreadRealtimeAppendSpeechParams;
use rexux_app_server_protocol::ThreadRealtimeAppendSpeechResponse;
use rexux_app_server_protocol::ThreadRealtimeAppendTextParams;
use rexux_app_server_protocol::ThreadRealtimeAppendTextResponse;
use rexux_app_server_protocol::ThreadRealtimeListVoicesResponse;
use rexux_app_server_protocol::ThreadRealtimeStartParams;
use rexux_app_server_protocol::ThreadRealtimeStartResponse;
use rexux_app_server_protocol::ThreadRealtimeStartTransport;
use rexux_app_server_protocol::ThreadRealtimeStopParams;
use rexux_app_server_protocol::ThreadRealtimeStopResponse;
use rexux_app_server_protocol::ThreadResumeInitialTurnsPageParams;
use rexux_app_server_protocol::ThreadResumeParams;
use rexux_app_server_protocol::ThreadResumeResponse;
use rexux_app_server_protocol::ThreadRollbackParams;
use rexux_app_server_protocol::ThreadSearchOccurrence;
use rexux_app_server_protocol::ThreadSearchOccurrencesParams;
use rexux_app_server_protocol::ThreadSearchOccurrencesResponse;
use rexux_app_server_protocol::ThreadSearchParams;
use rexux_app_server_protocol::ThreadSearchResponse;
use rexux_app_server_protocol::ThreadSearchResult;
use rexux_app_server_protocol::ThreadSearchSortKey;
use rexux_app_server_protocol::ThreadSearchTextRange;
use rexux_app_server_protocol::ThreadSetNameParams;
use rexux_app_server_protocol::ThreadSetNameResponse;
use rexux_app_server_protocol::ThreadSettings;
use rexux_app_server_protocol::ThreadSettingsUpdateParams;
use rexux_app_server_protocol::ThreadSettingsUpdateResponse;
use rexux_app_server_protocol::ThreadShellCommandParams;
use rexux_app_server_protocol::ThreadShellCommandResponse;
use rexux_app_server_protocol::ThreadSortKey;
use rexux_app_server_protocol::ThreadSourceKind;
use rexux_app_server_protocol::ThreadStartParams;
use rexux_app_server_protocol::ThreadStartResponse;
use rexux_app_server_protocol::ThreadStartedNotification;
use rexux_app_server_protocol::ThreadStatus;
use rexux_app_server_protocol::ThreadTimelineListParams;
use rexux_app_server_protocol::ThreadTimelineListResponse;
use rexux_app_server_protocol::ThreadTurnsListParams;
use rexux_app_server_protocol::ThreadTurnsListResponse;
use rexux_app_server_protocol::ThreadUnarchiveParams;
use rexux_app_server_protocol::ThreadUnarchiveResponse;
use rexux_app_server_protocol::ThreadUnarchivedNotification;
use rexux_app_server_protocol::ThreadUnsubscribeParams;
use rexux_app_server_protocol::ThreadUnsubscribeResponse;
use rexux_app_server_protocol::ThreadUnsubscribeStatus;
use rexux_app_server_protocol::Turn;
use rexux_app_server_protocol::TurnEnvironmentParams;
use rexux_app_server_protocol::TurnError;
use rexux_app_server_protocol::TurnInterruptParams;
use rexux_app_server_protocol::TurnInterruptResponse;
use rexux_app_server_protocol::TurnItemsView;
use rexux_app_server_protocol::TurnSettingsUpdateParams;
use rexux_app_server_protocol::TurnSettingsUpdateResponse;
use rexux_app_server_protocol::TurnSettingsUpdateStatus;
use rexux_app_server_protocol::TurnStartParams;
use rexux_app_server_protocol::TurnStartResponse;
use rexux_app_server_protocol::TurnStatus;
use rexux_app_server_protocol::TurnSteerParams;
use rexux_app_server_protocol::TurnSteerResponse;
use rexux_app_server_protocol::UserInput as V2UserInput;
use rexux_app_server_protocol::WindowsSandboxReadiness;
use rexux_app_server_protocol::WindowsSandboxReadinessResponse;
use rexux_app_server_protocol::WindowsSandboxSetupCompletedNotification;
use rexux_app_server_protocol::WindowsSandboxSetupMode;
use rexux_app_server_protocol::WindowsSandboxSetupStartParams;
use rexux_app_server_protocol::WindowsSandboxSetupStartResponse;
use rexux_app_server_protocol::WorkspaceMessage;
use rexux_app_server_protocol::WorkspaceMessageType;
use rexux_arg0::Arg0DispatchPaths;
use rexux_backend_client::AddCreditsNudgeCreditType as BackendAddCreditsNudgeCreditType;
use rexux_backend_client::Client as BackendClient;
use rexux_backend_client::RexuxWorkspaceMessage as BackendWorkspaceMessage;
use rexux_backend_client::RexuxWorkspaceMessageType as BackendWorkspaceMessageType;
use rexux_backend_client::RexuxWorkspaceMessagesResponse as BackendWorkspaceMessagesResponse;
use rexux_backend_client::ConsumeRateLimitResetCreditCode as BackendConsumeRateLimitResetCreditCode;
use rexux_backend_client::RateLimitResetCreditDetails as BackendRateLimitResetCreditDetails;
use rexux_backend_client::RateLimitResetCreditsDetails as BackendRateLimitResetCreditsDetails;
use rexux_backend_client::RequestError as BackendRequestError;
use rexux_backend_client::TokenUsageProfile;
use rexux_chatgpt::connectors;
use rexux_config::CloudConfigBundleLoadError;
use rexux_config::CloudConfigBundleLoadErrorCode;
use rexux_config::ConfigLayerStack;
use rexux_config::loader::project_trust_key;
use rexux_config::types::McpServerTransportConfig;
use rexux_connectors::AppInfo;
use rexux_core::RexuxThread;
use rexux_core::RexuxThreadSettingsOverrides;
use rexux_core::ForkSnapshot;
use rexux_core::McpManager;
use rexux_core::NewThread;
use rexux_core::NotSubmittedReason;
#[cfg(test)]
use rexux_core::SessionMeta;
use rexux_core::StartThreadOptions;
use rexux_core::SteerSubmission;
use rexux_core::ThreadConfigSnapshot;
use rexux_core::ThreadManager;
use rexux_core::TurnInput;
use rexux_core::TurnInputRequest;
use rexux_core::TurnInputSubmission;
use rexux_core::TurnStartOptions;
use rexux_core::config::Config;
use rexux_core::config::ConfigOverrides;
use rexux_core::config::NetworkProxyAuditMetadata;
use rexux_core::config::edit::ConfigEdit;
use rexux_core::config::edit::ConfigEditsBuilder;
use rexux_core::connectors::AccessibleConnectorsStatus;
use rexux_core::exec::ExecCapturePolicy;
use rexux_core::exec::ExecExpiration;
use rexux_core::exec::ExecParams;
use rexux_core::exec_env::create_env;
use rexux_core::path_utils;
#[cfg(test)]
use rexux_core::read_head_for_summary;
use rexux_core::sandboxing::SandboxPermissions;
use rexux_core::truncate_rollout_after_turn_id;
use rexux_core::truncate_rollout_before_turn_id;
use rexux_core::windows_sandbox::WindowsSandboxLevelExt;
use rexux_core::windows_sandbox::WindowsSandboxSetupMode as CoreWindowsSandboxSetupMode;
use rexux_core::windows_sandbox::WindowsSandboxSetupRequest;
use rexux_core::windows_sandbox::sandbox_setup_is_complete;
use rexux_core_plugins::PluginInstallError as CorePluginInstallError;
use rexux_core_plugins::PluginInstallRequest;
use rexux_core_plugins::PluginReadRequest;
use rexux_core_plugins::PluginUninstallError as CorePluginUninstallError;
use rexux_core_plugins::PluginsManager;
use rexux_core_plugins::loader::load_plugin_apps;
use rexux_core_plugins::manifest::PluginManifestInterface;
use rexux_core_plugins::marketplace::MarketplaceError;
use rexux_core_plugins::marketplace::MarketplacePluginSource;
use rexux_core_plugins::marketplace_add::MarketplaceAddError;
use rexux_core_plugins::marketplace_add::MarketplaceAddRequest;
use rexux_core_plugins::marketplace_add::add_marketplace as add_marketplace_to_rexux_home;
use rexux_core_plugins::marketplace_remove::MarketplaceRemoveError;
use rexux_core_plugins::marketplace_remove::MarketplaceRemoveRequest as CoreMarketplaceRemoveRequest;
use rexux_core_plugins::marketplace_remove::remove_marketplace;
use rexux_core_plugins::remote::RemoteMarketplace;
use rexux_core_plugins::remote::RemoteMarketplaceSource;
use rexux_core_plugins::remote::RemotePluginCatalogError;
use rexux_core_plugins::remote::RemotePluginDetail as RemoteCatalogPluginDetail;
use rexux_core_plugins::remote::RemotePluginServiceConfig;
use rexux_core_plugins::remote::RemotePluginShareContext as RemoteCatalogPluginShareContext;
use rexux_core_plugins::remote::RemotePluginShareSummary as RemoteCatalogPluginShareSummary;
use rexux_core_plugins::remote::RemotePluginSummary as RemoteCatalogPluginSummary;
use rexux_exec_server::EnvironmentManager;
use rexux_exec_server::EnvironmentObservedStatus;
use rexux_exec_server::LOCAL_ENVIRONMENT_ID;
use rexux_exec_server::LOCAL_FS;
use rexux_features::FEATURES;
use rexux_features::Feature;
use rexux_features::Stage;
use rexux_feedback::RexuxFeedback;
use rexux_feedback::FeedbackAttachmentPath;
use rexux_feedback::FeedbackUploadOptions;
use rexux_git_utils::git_diff_to_remote;
use rexux_git_utils::resolve_root_git_project_for_trust;
use rexux_login::AuthManager;
use rexux_login::RexuxAuth;
use rexux_login::login_with_api_key;
use rexux_login::login_with_bedrock_api_key;
use rexux_mcp::McpRuntimeContext;
use rexux_mcp::McpServerStatusSnapshot;
use rexux_mcp::McpSnapshotDetail;
use rexux_mcp::collect_mcp_server_status_snapshot_with_detail;
use rexux_mcp::discover_supported_scopes;
use rexux_mcp::read_mcp_resource as read_mcp_resource_without_thread;
use rexux_mcp::resolve_oauth_scopes;
use rexux_memories_write::clear_memory_roots_contents;
use rexux_model_provider::create_model_provider;
use rexux_models_manager::collaboration_mode_presets::builtin_collaboration_mode_presets;
use rexux_protocol::ThreadId;
use rexux_protocol::config_types::CollaborationMode;
use rexux_protocol::config_types::ForcedLoginMethod;
use rexux_protocol::config_types::Personality;
use rexux_protocol::config_types::ReasoningSummary;
use rexux_protocol::config_types::TrustLevel;
use rexux_protocol::config_types::WindowsSandboxLevel;
use rexux_protocol::error::RexuxErr;
use rexux_protocol::error::Result as RexuxResult;
#[cfg(test)]
use rexux_protocol::items::TurnItem;
use rexux_protocol::models::ResponseItem;
use rexux_protocol::openai_models::ReasoningEffort;
use rexux_protocol::protocol::AgentStatus;
use rexux_protocol::protocol::ConversationAudioParams;
use rexux_protocol::protocol::ConversationSpeechParams;
use rexux_protocol::protocol::ConversationStartParams;
use rexux_protocol::protocol::ConversationStartTransport;
use rexux_protocol::protocol::ConversationTextParams;
use rexux_protocol::protocol::EnvironmentConfigState;
use rexux_protocol::protocol::EventMsg;
#[cfg(test)]
use rexux_protocol::protocol::GitInfo as CoreGitInfo;
use rexux_protocol::protocol::McpAuthStatus as CoreMcpAuthStatus;
use rexux_protocol::protocol::Op;
use rexux_protocol::protocol::RealtimeVoicesList;
use rexux_protocol::protocol::ReviewDelivery as CoreReviewDelivery;
use rexux_protocol::protocol::ReviewRequest;
use rexux_protocol::protocol::ReviewTarget as CoreReviewTarget;
use rexux_protocol::protocol::SessionConfiguredEvent;
#[cfg(test)]
use rexux_protocol::protocol::SessionMetaLine;
use rexux_protocol::protocol::TurnEnvironmentSelection;
use rexux_protocol::protocol::TurnEnvironmentSelections;
use rexux_protocol::protocol::W3cTraceContext;
use rexux_protocol::protocol::strip_user_message_prefix;
use rexux_protocol::user_input::MAX_USER_INPUT_TEXT_CHARS;
use rexux_protocol::user_input::UserInput as CoreInputItem;
use rexux_rmcp_client::McpOAuthClientRegistration;
use rexux_rmcp_client::StreamableHttpRedirectMode;
use rexux_rmcp_client::perform_oauth_login_return_url;
use rexux_rollout::InitialHistory;
use rexux_rollout::ResumedHistory;
use rexux_rollout::RolloutItem;
use rexux_rollout::is_persisted_rollout_item;
use rexux_rollout::state_db::StateDbHandle;
use rexux_rollout::state_db::reconcile_rollout;
use rexux_state::ThreadMetadata;
use rexux_state::log_db::LogDbLayer;
use rexux_thread_store::ArchiveThreadParams as StoreArchiveThreadParams;
use rexux_thread_store::ArchiveThreadsParams as StoreArchiveThreadsParams;
use rexux_thread_store::ClearableField as StoreClearableField;
use rexux_thread_store::DeleteThreadsParams as StoreDeleteThreadsParams;
use rexux_thread_store::GitInfoPatch as StoreGitInfoPatch;
use rexux_thread_store::ItemSortKey as StoreItemSortKey;
use rexux_thread_store::ListItemsParams as StoreListItemsParams;
use rexux_thread_store::ListThreadsParams as StoreListThreadsParams;
use rexux_thread_store::ListTimelineParams as StoreListTimelineParams;
use rexux_thread_store::ListTurnsParams as StoreListTurnsParams;
use rexux_thread_store::LoadThreadHistoryParams as StoreLoadThreadHistoryParams;
use rexux_thread_store::LocalThreadStore;
use rexux_thread_store::ReadThreadByRolloutPathParams as StoreReadThreadByRolloutPathParams;
use rexux_thread_store::ReadThreadParams as StoreReadThreadParams;
use rexux_thread_store::SearchThreadOccurrencesParams as StoreSearchThreadOccurrencesParams;
use rexux_thread_store::SearchThreadsParams as StoreSearchThreadsParams;
use rexux_thread_store::SortDirection as StoreSortDirection;
use rexux_thread_store::StoredThread;
use rexux_thread_store::StoredTurn;
use rexux_thread_store::StoredTurnItemsView;
use rexux_thread_store::StoredTurnStatus;
use rexux_thread_store::ThreadMetadataPatch as StoreThreadMetadataPatch;
use rexux_thread_store::ThreadRelationFilter as StoreThreadRelationFilter;
use rexux_thread_store::ThreadSortKey as StoreThreadSortKey;
use rexux_thread_store::ThreadStore;
use rexux_thread_store::ThreadStoreError;
use rexux_utils_absolute_path::AbsolutePathBuf;
use rexux_utils_pty::DEFAULT_OUTPUT_BYTES_CAP;
use std::io::Error as IoError;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::result::Result;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::Mutex;
use tokio::sync::Semaphore;
use tokio::sync::SemaphorePermit;
use tokio::sync::broadcast;
use tokio::sync::oneshot;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tokio_util::sync::DropGuard;
use tokio_util::task::TaskTracker;
use toml::Value as TomlValue;
use tracing::Instrument;
use tracing::error;
use tracing::info;
use tracing::warn;
use uuid::Uuid;

#[cfg(test)]
use rexux_app_server_protocol::ServerRequest;

mod account_processor;
mod apps_processor;
mod bedrock_auth;
mod catalog_processor;
mod command_exec_processor;
mod config_processor;
mod diagnostics;
mod environment_processor;
mod feedback_doctor_report;
mod feedback_processor;
mod feedback_thread_index;
mod fs_processor;
mod git_processor;
mod initialize_processor;
mod marketplace_processor;
mod mcp_event_stream;
mod mcp_processor;
mod persisted_resume_settings;
mod plugins;
mod process_exec_processor;
mod projects;
mod remote_control_processor;
mod search;
mod thread_enrichment;
mod thread_fork_goal;
mod thread_input;
mod thread_processor;
mod thread_queue_processor;
mod thread_sections;
mod token_usage_replay;
mod turn_processor;
mod windows_sandbox_processor;

pub(crate) use account_processor::AccountRequestProcessor;
pub(crate) use apps_processor::AppsRequestProcessor;
pub(crate) use catalog_processor::CatalogRequestProcessor;
pub(crate) use command_exec_processor::CommandExecRequestProcessor;
pub(crate) use config_processor::ConfigRequestProcessor;
pub(crate) use diagnostics::read_server_diagnostics;
pub(crate) use environment_processor::EnvironmentRequestProcessor;
pub(crate) use feedback_processor::FeedbackRequestProcessor;
pub(crate) use fs_processor::FsRequestProcessor;
pub(crate) use git_processor::GitRequestProcessor;
pub(crate) use initialize_processor::InitializeRequestProcessor;
pub(crate) use marketplace_processor::MarketplaceRequestProcessor;
pub(crate) use mcp_event_stream::McpEventStreamReady;
pub(crate) use mcp_event_stream::McpEventStreams;
pub(crate) use mcp_processor::McpRequestProcessor;
pub(crate) use plugins::PluginRequestProcessor;
pub(crate) use process_exec_processor::ProcessExecRequestProcessor;
pub(crate) use projects::ProjectRequestProcessor;
pub(crate) use remote_control_processor::RemoteControlRequestProcessor;
pub(crate) use search::SearchRequestProcessor;
pub(crate) use thread_goal_processor::ThreadGoalRequestProcessor;
pub(crate) use thread_processor::ThreadRequestProcessor;
pub(crate) use thread_queue_processor::ThreadQueueRequestProcessor;
pub(crate) use turn_processor::TurnRequestProcessor;
pub(crate) use windows_sandbox_processor::WindowsSandboxRequestProcessor;

use crate::error_code::internal_error;
use crate::error_code::invalid_request;
use crate::filters::compute_source_filters;
use crate::filters::source_kind_matches;
use crate::thread_state::ConnectionCapabilities;
use crate::thread_state::ThreadListenerCommand;
use crate::thread_state::ThreadState;
use crate::thread_state::ThreadStateManager;
use token_usage_replay::restored_token_usage_turn_id;
use token_usage_replay::send_thread_token_usage_update_to_connection;

pub(crate) fn apply_live_model_settings(
    thread: &mut Thread,
    config_snapshot: &ThreadConfigSnapshot,
) {
    thread.model = Some(config_snapshot.model.clone());
    thread.reasoning_effort = config_snapshot.reasoning_effort.clone();
}

fn resolve_request_cwd(cwd: Option<PathBuf>) -> Result<Option<AbsolutePathBuf>, JSONRPCErrorError> {
    cwd.map(|cwd| {
        AbsolutePathBuf::relative_to_current_dir(path_utils::normalize_for_native_workdir(cwd))
            .map_err(|err| invalid_request(format!("invalid cwd: {err}")))
    })
    .transpose()
}

fn resolve_turn_environment_selections(
    thread_manager: &ThreadManager,
    environments: Option<Vec<TurnEnvironmentParams>>,
) -> Result<Option<Vec<TurnEnvironmentSelection>>, JSONRPCErrorError> {
    let Some(environments) = environments else {
        return Ok(None);
    };
    let mut selections = Vec::with_capacity(environments.len());
    for environment in environments {
        let environment_id = environment.environment_id;
        let cwd = environment
            .cwd
            .to_inferred_path_uri()
            .ok_or_else(|| {
                invalid_request(format!(
                    "invalid cwd for environment `{environment_id}`: path `{}` does not use absolute POSIX or Windows path syntax",
                    environment.cwd
                ))
            })?;
        let workspace_roots = environment
            .runtime_workspace_roots
            .map(|roots| {
                let mut resolved_roots = Vec::new();
                for root in roots {
                    let root = root.to_inferred_path_uri().ok_or_else(|| {
                        invalid_request(format!(
                            "invalid runtime workspace root for environment `{environment_id}`: path `{root}` does not use absolute POSIX or Windows path syntax"
                        ))
                    })?;
                    if !resolved_roots.contains(&root) {
                        resolved_roots.push(root);
                    }
                }
                Ok::<_, JSONRPCErrorError>(resolved_roots)
            })
            .transpose()?
            .unwrap_or_else(|| vec![cwd.clone()]);
        selections.push(TurnEnvironmentSelection {
            environment_id,
            cwd,
            workspace_roots,
            config: EnvironmentConfigState::FromThread,
        });
    }
    thread_manager
        .validate_environment_selections(&selections)
        .map_err(environment_selection_error)?;
    Ok(Some(selections))
}

fn resolve_runtime_workspace_roots(workspace_roots: Vec<AbsolutePathBuf>) -> Vec<AbsolutePathBuf> {
    let mut resolved_roots = Vec::new();
    for root in workspace_roots {
        if !resolved_roots.iter().any(|existing| existing == &root) {
            resolved_roots.push(root);
        }
    }
    resolved_roots
}

mod config_errors;
mod request_errors;
mod thread_delete;
mod thread_goal_processor;
mod thread_lifecycle;
mod thread_resume_redaction;
mod thread_summary;

use self::config_errors::*;
use self::request_errors::*;
use self::thread_goal_processor::api_thread_goal_from_state;
use self::thread_lifecycle::*;
use self::thread_resume_redaction::*;
use self::thread_summary::*;

pub(crate) use self::thread_lifecycle::populate_thread_turns_from_history;
pub(crate) use self::thread_processor::thread_from_stored_thread;
#[cfg(test)]
pub(crate) use self::thread_summary::read_summary_from_rollout;
#[cfg(test)]
pub(crate) use self::thread_summary::summary_to_thread;
pub(crate) use self::thread_summary::thread_settings_from_config_snapshot;

pub(crate) fn build_legacy_api_turns_from_rollout_items(items: &[RolloutItem]) -> Vec<Turn> {
    let mut builder = ThreadHistoryBuilder::new();
    for item in items {
        if is_persisted_rollout_item(item, rexux_protocol::protocol::ThreadHistoryMode::Legacy) {
            builder.handle_rollout_item(item);
        }
    }
    builder.finish()
}
