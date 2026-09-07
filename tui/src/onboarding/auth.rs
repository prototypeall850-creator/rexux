//! Authentication step UI and state transitions used by onboarding.
//!
//! This module owns the auth-step state machine (BYOK provider wizard /
//! OpenAI API key / Bedrock), renders the corresponding UI, and handles
//! auth-scoped keyboard input. It intentionally does not decide onboarding
//! flow completion; the enclosing onboarding screen coordinates step
//! progression.
//!
//! BYOK flow: enter a provider name, its base URL, and an API key. Rexux
//! fetches the models the provider exposes (`GET {base_url}/models`) and the
//! user picks one. The provider (with its bearer token) is persisted to the
//! user `config.toml` through the app-server `config/batchWrite` API.

#![allow(clippy::unwrap_used)]

use rexux_app_server_client::AppServerRequestHandle;
use rexux_app_server_protocol::AccountUpdatedNotification;
use rexux_app_server_protocol::AuthMode as ApiAuthMode;
use rexux_app_server_protocol::ClientRequest;
use rexux_app_server_protocol::ConfigBatchWriteParams;
use rexux_app_server_protocol::ConfigWriteResponse;
use rexux_app_server_protocol::LoginAccountParams;
use rexux_app_server_protocol::LoginAccountResponse;
use rexux_login::AuthConfig;
use rexux_login::read_openai_api_key_from_env;
use rexux_protocol::auth::AuthMode;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::prelude::Widget;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Block;
use ratatui::widgets::BorderType;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use ratatui::widgets::WidgetRef;
use ratatui::widgets::Wrap;

use rexux_protocol::config_types::ForcedLoginMethod;
use rexux_http_client::ClientRouteClass;
use rexux_http_client::HttpClientFactory;
use rexux_http_client::RouteAwareClientPool;
use serde::Deserialize;
use std::cell::Cell;
use std::sync::Arc;
use std::sync::RwLock;
use uuid::Uuid;

use crate::LoginStatus;
use crate::config_update::replace_config_value;
use crate::key_hint::KeyBinding;
use crate::key_hint::KeyBindingListExt;
use crate::onboarding::bedrock::BedrockState;
use crate::onboarding::keys;
use crate::onboarding::onboarding_screen::KeyboardHandler;
use crate::onboarding::onboarding_screen::StepStateProvider;
use crate::tui::FrameRequester;

/// Marks buffer cells that have cyan+underlined style as an OSC 8 hyperlink.
///
/// Terminal emulators recognise the OSC 8 escape sequence and treat the entire
/// marked region as a single clickable link, regardless of row wrapping.  This
/// is necessary because ratatui's cell-based rendering emits `MoveTo` at every
/// row boundary, which breaks normal terminal URL detection for long URLs that
/// wrap across multiple rows.
pub(crate) fn mark_url_hyperlink(buf: &mut Buffer, area: Rect, url: &str) {
    crate::terminal_hyperlinks::mark_url_hyperlink(buf, area, url);
}

/// Marks any underlined buffer cells as an OSC 8 hyperlink.
pub(crate) fn mark_underlined_hyperlink(buf: &mut Buffer, area: Rect, url: &str) {
    crate::terminal_hyperlinks::mark_underlined_hyperlink(buf, area, url);
}

use super::onboarding_screen::StepState;


#[derive(Clone)]
pub(crate) enum SignInState {
    PickMode,
    ByokEntry(ByokEntryState),
    ByokModelSelect(ByokModelSelectState),
    ByokSaving(ByokProviderConfig),
    ByokConfigured(ByokProviderConfig),
    ApiKeyEntry(ApiKeyInputState),
    ApiKeyConfigured,
    Bedrock(BedrockState),
    BedrockConfigured,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SignInOption {
    Byok,
    ApiKey,
    Bedrock,
}

const API_KEY_DISABLED_MESSAGE: &str = "API key login is disabled.";
pub(super) fn onboarding_request_id() -> rexux_app_server_protocol::RequestId {
    rexux_app_server_protocol::RequestId::String(Uuid::new_v4().to_string())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ByokField {
    #[default]
    Provider,
    BaseUrl,
    ApiKey,
}

impl ByokField {
    fn next(self) -> Option<Self> {
        match self {
            Self::Provider => Some(Self::BaseUrl),
            Self::BaseUrl => Some(Self::ApiKey),
            Self::ApiKey => None,
        }
    }

    fn previous(self) -> Option<Self> {
        match self {
            Self::Provider => None,
            Self::BaseUrl => Some(Self::Provider),
            Self::ApiKey => Some(Self::BaseUrl),
        }
    }
}

/// BYOK wizard text-entry state: three fields completed in order.
#[derive(Clone, Debug, Default)]
pub(crate) struct ByokEntryState {
    pub(crate) field: ByokField,
    pub(crate) provider: String,
    pub(crate) base_url: String,
    pub(crate) api_key: String,
    pub(crate) fetching: bool,
}

/// Provider details collected by the wizard; reused by save/configured states.
#[derive(Clone, Debug)]
pub(crate) struct ByokProviderConfig {
    pub(crate) provider: String,
    pub(crate) base_url: String,
    pub(crate) api_key: String,
}

/// Model picker shown after a successful `/models` fetch.
#[derive(Clone, Debug)]
pub(crate) struct ByokModelSelectState {
    pub(crate) config: ByokProviderConfig,
    pub(crate) models: Vec<String>,
    pub(crate) highlighted: usize,
}

#[derive(Clone, Default)]
pub(crate) struct ApiKeyInputState {
    value: String,
    prepopulated_from_env: bool,
}

impl KeyboardHandler for AuthModeWidget {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        if self.handle_bedrock_key_event(&key_event) {
            return;
        }
        if self.handle_byok_key_event(&key_event) {
            return;
        }
        if self.handle_api_key_entry_key_event(&key_event) {
            return;
        }

        if keys::MOVE_UP.is_pressed(key_event) {
            self.move_highlight(/*delta*/ -1);
            return;
        }
        if keys::MOVE_DOWN.is_pressed(key_event) {
            self.move_highlight(/*delta*/ 1);
            return;
        }
        if keys::SELECT_FIRST.is_pressed(key_event) {
            self.select_option_by_index(/*index*/ 0);
            return;
        }
        if keys::SELECT_SECOND.is_pressed(key_event) {
            self.select_option_by_index(/*index*/ 1);
            return;
        }
        if keys::SELECT_THIRD.is_pressed(key_event) {
            self.select_option_by_index(/*index*/ 2);
            return;
        }
        if keys::SELECT_FOURTH.is_pressed(key_event) {
            self.select_option_by_index(/*index*/ 3);
            return;
        }
        if keys::CONFIRM.is_pressed(key_event) {
            let sign_in_state = { (*self.sign_in_state.read().unwrap()).clone() };
            match sign_in_state {
                SignInState::PickMode => {
                    self.handle_sign_in_option(self.highlighted_mode);
                }
                _ => {}
            }
            return;
        }
        if keys::CANCEL.is_pressed(key_event) {
            tracing::info!("Cancel onboarding auth step");
            self.cancel_active_attempt();
        }
    }

    fn handle_paste(&mut self, pasted: String) {
        let sign_in_state = self.sign_in_state.read().unwrap();
        match &*sign_in_state {
            SignInState::Bedrock(_) => {
                drop(sign_in_state);
                let _ = self.handle_bedrock_paste(&pasted);
            }
            SignInState::ByokEntry(_) => {
                drop(sign_in_state);
                let _ = self.handle_byok_paste(pasted);
            }
            SignInState::ApiKeyEntry(_) => {
                drop(sign_in_state);
                let _ = self.handle_api_key_entry_paste(pasted);
            }
            _ => {}
        }
    }
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct AuthModeWidget {
    pub request_frame: FrameRequester,
    pub highlighted_mode: SignInOption,
    pub error: Arc<RwLock<Option<String>>>,
    pub sign_in_state: Arc<RwLock<SignInState>>,
    pub login_status: LoginStatus,
    pub app_server_request_handle: AppServerRequestHandle,
    pub auth_config: AuthConfig,
    pub bedrock_setup_enabled: bool,
    pub animations_enabled: bool,
    pub animations_suppressed: Cell<bool>,
    pub http_client_factory: HttpClientFactory,
}

impl AuthModeWidget {
    pub(crate) fn set_animations_suppressed(&self, suppressed: bool) {
        self.animations_suppressed.set(suppressed);
    }

    pub(crate) fn should_suppress_animations(&self) -> bool {
        false
    }

    pub(crate) fn cancel_active_attempt(&self) {
        let next = self.default_sign_in_state();
        *self.sign_in_state.write().unwrap() = next;
        self.set_error(/*message*/ None);
        self.request_frame.schedule_frame();
    }

    /// The state the auth step falls back to when nothing is in progress.
    fn default_sign_in_state(&self) -> SignInState {
        if self.is_api_login_allowed() && !self.bedrock_setup_enabled {
            SignInState::ByokEntry(ByokEntryState::default())
        } else {
            SignInState::PickMode
        }
    }

    fn set_error(&self, message: Option<String>) {
        *self.error.write().unwrap() = message;
    }

    fn error_message(&self) -> Option<String> {
        self.error.read().unwrap().clone()
    }

    /// Returns whether the auth flow is currently accepting text input.
    pub(crate) fn is_text_entry_active(&self) -> bool {
        self.sign_in_state.read().is_ok_and(|guard| match &*guard {
            SignInState::ByokEntry(_) | SignInState::ApiKeyEntry(_) => true,
            SignInState::Bedrock(state) => state.is_text_entry_active(),
            _ => false,
        })
    }

    /// Returns whether printable quit shortcuts must be treated as text input.
    pub(crate) fn should_suppress_printable_quit(&self) -> bool {
        self.sign_in_state.read().is_ok_and(|guard| match &*guard {
            SignInState::ByokEntry(_) => true,
            SignInState::ApiKeyEntry(state) => !state.value.is_empty(),
            SignInState::Bedrock(state) => state.is_text_entry_active(),
            _ => false,
        })
    }

    fn confirm_binding(&self) -> KeyBinding {
        keys::CONFIRM[0]
    }

    fn cancel_binding(&self) -> KeyBinding {
        keys::CANCEL[0]
    }

    fn is_api_login_allowed(&self) -> bool {
        self.auth_config
            .is_login_method_allowed(ForcedLoginMethod::Api)
    }

    fn displayed_sign_in_options(&self) -> Vec<SignInOption> {
        let mut options = vec![SignInOption::Byok];
        if self.is_api_login_allowed() {
            options.push(SignInOption::ApiKey);
            if self.bedrock_setup_enabled {
                options.push(SignInOption::Bedrock);
            }
        }
        options
    }

    fn selectable_sign_in_options(&self) -> Vec<SignInOption> {
        self.displayed_sign_in_options()
    }

    fn move_highlight(&mut self, delta: isize) {
        let options = self.selectable_sign_in_options();
        if options.is_empty() {
            return;
        }

        let current_index = options
            .iter()
            .position(|option| *option == self.highlighted_mode)
            .unwrap_or(0);
        let next_index =
            (current_index as isize + delta).rem_euclid(options.len() as isize) as usize;
        self.highlighted_mode = options[next_index];
    }

    fn select_option_by_index(&mut self, index: usize) {
        let options = self.displayed_sign_in_options();
        if let Some(option) = options.get(index).copied() {
            self.handle_sign_in_option(option);
        }
    }

    fn handle_sign_in_option(&mut self, option: SignInOption) {
        match option {
            SignInOption::Byok => {
                if self.is_api_login_allowed() {
                    self.start_byok_entry();
                } else {
                    self.disallow_api_login();
                }
            }
            SignInOption::ApiKey => {
                if self.is_api_login_allowed() {
                    self.start_api_key_entry();
                } else {
                    self.disallow_api_login();
                }
            }
            SignInOption::Bedrock => {
                if self.bedrock_setup_enabled && self.is_api_login_allowed() {
                    self.start_bedrock_discovery();
                } else if !self.is_api_login_allowed() {
                    self.disallow_api_login();
                }
            }
        }
    }

    fn disallow_api_login(&mut self) {
        self.highlighted_mode = SignInOption::Byok;
        self.set_error(Some(API_KEY_DISABLED_MESSAGE.to_string()));
        *self.sign_in_state.write().unwrap() = SignInState::PickMode;
        self.request_frame.schedule_frame();
    }

    fn render_pick_mode(&self, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<Line> = vec![
            Line::from(vec![
                "  ".into(),
                "Connect a provider to use Rexux".into(),
            ]),
            Line::from(vec![
                "  ".into(),
                "bring your own API key (BYOK)".dim(),
            ]),
            "".into(),
        ];

        let create_mode_item = |idx: usize,
                                selected_mode: SignInOption,
                                text: &str,
                                description: &str|
         -> Vec<Line<'static>> {
            let is_selected = self.highlighted_mode == selected_mode;
            let caret = if is_selected { ">" } else { " " };

            let line1 = if is_selected {
                Line::from(vec![
                    format!("{caret} {index}. ", index = idx + 1).cyan().dim(),
                    text.to_string().cyan(),
                ])
            } else {
                format!("  {index}. {text}", index = idx + 1).into()
            };

            let line2 = if is_selected {
                Line::from(format!("     {description}"))
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::DIM)
            } else {
                Line::from(format!("     {description}"))
                    .style(Style::default().add_modifier(Modifier::DIM))
            };

            vec![line1, line2]
        };

        for (idx, option) in self.displayed_sign_in_options().into_iter().enumerate() {
            match option {
                SignInOption::Byok => {
                    lines.extend(create_mode_item(
                        idx,
                        option,
                        "Connect a custom provider",
                        "Enter a base URL and API key",
                    ));
                }
                SignInOption::ApiKey => {
                    lines.extend(create_mode_item(
                        idx,
                        option,
                        "Use an OpenAI API key",
                        "Pay for what you use",
                    ));
                }
                SignInOption::Bedrock => {
                    lines.extend(create_mode_item(
                        idx,
                        option,
                        "Use Amazon Bedrock",
                        "Connect using your AWS credentials",
                    ));
                }
            }
            lines.push("".into());
        }

        if !self.is_api_login_allowed() {
            lines.push(
                "  API key login is disabled by this workspace.".dim().into(),
            );
            lines.push("".into());
        }
        lines.push(Line::from(vec![
            "  Press ".dim(),
            self.confirm_binding().into(),
            " to continue".dim(),
        ]));
        if let Some(err) = self.error_message() {
            lines.push("".into());
            lines.push(err.red().into());
        }

        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }

    fn render_api_key_configured(&self, area: Rect, buf: &mut Buffer) {
        let lines = vec![
            "✓ API key configured".fg(Color::Green).into(),
            "".into(),
            "  Rexux will use usage-based billing with your API key.".into(),
        ];

        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }

    fn render_byok_entry(&self, area: Rect, buf: &mut Buffer, state: &ByokEntryState) {
        let [intro_area, fields_area, footer_area] = Layout::vertical([
            Constraint::Min(4),
            Constraint::Length(16),
            Constraint::Min(2),
        ])
        .areas(area);

        let intro_lines: Vec<Line> = vec![
            Line::from(vec![
                "> ".into(),
                "Connect a custom provider".bold(),
            ]),
            "".into(),
            "  Enter your provider name, its base URL, and your API key.".into(),
            "  Rexux fetches the available models automatically.".into(),
            "".into(),
        ];
        Paragraph::new(intro_lines)
            .wrap(Wrap { trim: false })
            .render(intro_area, buf);

        let fields = [
            (ByokField::Provider, "Provider", &state.provider, "e.g. openrouter"),
            (
                ByokField::BaseUrl,
                "Base URL",
                &state.base_url,
                "https://openrouter.ai/api/v1",
            ),
            (ByokField::ApiKey, "API key", &state.api_key, "sk-..."),
        ];

        let [provider_area, base_url_area, api_key_area] = Layout::vertical([
            Constraint::Length(5),
            Constraint::Length(5),
            Constraint::Length(5),
        ])
        .areas(fields_area);
        for ((field, label, value, placeholder), field_area) in fields
            .iter()
            .zip([provider_area, base_url_area, api_key_area])
        {
            self.render_byok_field(field_area, buf, *field, label, value, placeholder);
        }

        let mut footer_lines: Vec<Line> = vec![Line::from(vec![
            "  Press ".dim(),
            self.confirm_binding().into(),
            " to continue, ".dim(),
            self.cancel_binding().into(),
            " to go back".dim(),
        ])];
        if state.fetching {
            footer_lines.push("".into());
            footer_lines.push("  Fetching available models…".into());
        }
        if let Some(error) = self.error_message() {
            footer_lines.push("".into());
            footer_lines.push(error.red().into());
        }
        Paragraph::new(footer_lines)
            .wrap(Wrap { trim: false })
            .render(footer_area, buf);
    }

    fn render_byok_field(
        &self,
        area: Rect,
        buf: &mut Buffer,
        field: ByokField,
        label: &str,
        value: &str,
        placeholder: &str,
    ) {
        let [label_area, input_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Length(3)]).areas(area);

        let is_active = self
            .sign_in_state
            .read()
            .is_ok_and(|guard| matches!(&*guard, SignInState::ByokEntry(state) if state.field == field));

        let label_line: Line = if is_active {
            Line::from(format!("  {label}")).cyan()
        } else {
            Line::from(format!("  {label}")).dim()
        };
        Paragraph::new(label_line).render(label_area, buf);

        let content_line: Line = if value.is_empty() {
            Line::from(format!("  {placeholder}")).dim()
        } else {
            Line::from(format!("  {value}"))
        };
        Paragraph::new(content_line)
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(if is_active {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default().add_modifier(Modifier::DIM)
                    }),
            )
            .render(input_area, buf);
    }

    fn render_byok_model_select(
        &self,
        area: Rect,
        buf: &mut Buffer,
        state: &ByokModelSelectState,
    ) {
        let [intro_area, list_area, footer_area] = Layout::vertical([
            Constraint::Length(5),
            Constraint::Min(4),
            Constraint::Min(2),
        ])
        .areas(area);

        let intro_lines: Vec<Line> = vec![
            Line::from(vec!["> ".into(), "Select a model".bold()]),
            "".into(),
            Line::from(format!(
                "  {} models available from {}.",
                state.models.len(),
                state.config.provider
            )),
            "".into(),
        ];
        Paragraph::new(intro_lines)
            .wrap(Wrap { trim: false })
            .render(intro_area, buf);

        const VISIBLE_ROWS: usize = 12;
        let total = state.models.len();
        let start = state
            .highlighted
            .saturating_sub(VISIBLE_ROWS.saturating_sub(1) / 2)
            .min(total.saturating_sub(VISIBLE_ROWS.min(total)));
        let end = (start + VISIBLE_ROWS).min(total);

        let mut list_lines: Vec<Line> = Vec::new();
        for (idx, model) in state.models[start..end].iter().enumerate() {
            let idx = start + idx;
            if idx == state.highlighted {
                list_lines.push(Line::from(vec![
                    "  > ".cyan(),
                    model.clone().cyan(),
                ]));
            } else {
                list_lines.push(Line::from(vec!["    ".into(), model.clone().into()]));
            }
        }
        if end < total {
            list_lines.push(
                Line::from(format!("    … {} more", total - end))
                    .style(Style::default().add_modifier(Modifier::DIM)),
            );
        }
        Paragraph::new(list_lines)
            .wrap(Wrap { trim: false })
            .render(list_area, buf);

        let mut footer_lines: Vec<Line> = vec![Line::from(vec![
            "  Press ".dim(),
            self.confirm_binding().into(),
            " to choose, ".dim(),
            self.cancel_binding().into(),
            " to go back".dim(),
        ])];
        if let Some(error) = self.error_message() {
            footer_lines.push("".into());
            footer_lines.push(error.red().into());
        }
        Paragraph::new(footer_lines)
            .wrap(Wrap { trim: false })
            .render(footer_area, buf);
    }

    fn render_byok_saving(&self, area: Rect, buf: &mut Buffer, config: &ByokProviderConfig) {
        let lines = vec![
            Line::from(format!("  Connecting to {}…", config.provider)),
            "".into(),
        ];
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }

    fn render_byok_configured(&self, area: Rect, buf: &mut Buffer, config: &ByokProviderConfig) {
        let lines = vec![
            format!("✓ Connected to {}", config.provider)
                .fg(Color::Green)
                .into(),
            "".into(),
            "  Rexux is ready. Run /model to switch models anytime.".into(),
        ];

        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }

    // ------------------------------------------------------------------
    // BYOK wizard input handling
    // ------------------------------------------------------------------

    fn handle_byok_key_event(&mut self, key_event: &KeyEvent) -> bool {
        let sign_in_state = { (*self.sign_in_state.read().unwrap()).clone() };
        match sign_in_state {
            SignInState::ByokEntry(state) => {
                self.handle_byok_entry_key_event(state, key_event)
            }
            SignInState::ByokModelSelect(state) => {
                self.handle_byok_model_select_key_event(state, key_event)
            }
            SignInState::ByokSaving(_) => {
                // Save is in flight; only allow going back to the model list on Esc.
                if keys::CANCEL.is_pressed(*key_event) {
                    // Keep waiting; a save result will arrive shortly.
                    return true;
                }
                true
            }
            SignInState::ByokConfigured(_) => true,
            _ => false,
        }
    }

    fn handle_byok_entry_key_event(
        &mut self,
        mut state: ByokEntryState,
        key_event: &KeyEvent,
    ) -> bool {
        if state.fetching {
            // A fetch is in flight; swallow keys until it resolves.
            return true;
        }

        if keys::CANCEL.is_pressed(*key_event) {
            match state.field.previous() {
                Some(previous_field) => {
                    state.field = previous_field;
                    self.set_error(/*message*/ None);
                    self.commit_byok_entry(state);
                }
                None => {
                    *self.sign_in_state.write().unwrap() = SignInState::PickMode;
                    self.set_error(/*message*/ None);
                    self.request_frame.schedule_frame();
                }
            }
            return true;
        }

        if keys::CONFIRM.is_pressed(*key_event) {
            match state.field {
                ByokField::Provider => {
                    if state.provider.trim().is_empty() {
                        self.set_error(Some("Provider name cannot be empty".to_string()));
                    } else {
                        if state.base_url.is_empty()
                            && let Some(suggestion) = suggested_base_url(&state.provider)
                        {
                            state.base_url = suggestion.to_string();
                        }
                        state.field = ByokField::BaseUrl;
                        self.set_error(/*message*/ None);
                    }
                    self.commit_byok_entry(state);
                }
                ByokField::BaseUrl => {
                    let trimmed = state.base_url.trim();
                    if trimmed.is_empty() {
                        self.set_error(Some("Base URL cannot be empty".to_string()));
                    } else if !(trimmed.starts_with("http://") || trimmed.starts_with("https://"))
                    {
                        self.set_error(Some(
                            "Base URL must start with http:// or https://".to_string(),
                        ));
                    } else {
                        state.field = ByokField::ApiKey;
                        self.set_error(/*message*/ None);
                    }
                    self.commit_byok_entry(state);
                }
                ByokField::ApiKey => {
                    if state.api_key.trim().is_empty() {
                        self.set_error(Some("API key cannot be empty".to_string()));
                        self.commit_byok_entry(state);
                    } else {
                        state.fetching = true;
                        self.commit_byok_entry(state);
                        self.start_byok_model_fetch();
                    }
                }
            }
            return true;
        }

        match key_event.code {
            KeyCode::Backspace => {
                self.byok_field_mut(&mut state).pop();
                self.set_error(/*message*/ None);
            }
            KeyCode::Char(c)
                if key_event.kind == KeyEventKind::Press
                    && !key_event.modifiers.contains(KeyModifiers::SUPER)
                    && !key_event.modifiers.contains(KeyModifiers::CONTROL)
                    && !key_event.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.byok_field_mut(&mut state).push(c);
                self.set_error(/*message*/ None);
            }
            _ => {}
        }
        self.commit_byok_entry(state);
        true
    }

    fn byok_field_mut<'a>(&self, state: &'a mut ByokEntryState) -> &'a mut String {
        match state.field {
            ByokField::Provider => &mut state.provider,
            ByokField::BaseUrl => &mut state.base_url,
            ByokField::ApiKey => &mut state.api_key,
        }
    }

    fn commit_byok_entry(&mut self, state: ByokEntryState) {
        *self.sign_in_state.write().unwrap() = SignInState::ByokEntry(state);
        self.request_frame.schedule_frame();
    }

    fn handle_byok_model_select_key_event(
        &mut self,
        mut state: ByokModelSelectState,
        key_event: &KeyEvent,
    ) -> bool {
        if keys::MOVE_UP.is_pressed(*key_event) {
            state.highlighted = state.highlighted.saturating_sub(1);
        } else if keys::MOVE_DOWN.is_pressed(*key_event) {
            if state.highlighted + 1 < state.models.len() {
                state.highlighted += 1;
            }
        } else if keys::CONFIRM.is_pressed(*key_event) {
            if let Some(model) = state.models.get(state.highlighted).cloned() {
                let config = state.config.clone();
                *self.sign_in_state.write().unwrap() = SignInState::ByokSaving(config.clone());
                self.request_frame.schedule_frame();
                self.start_byok_save(config, model);
            }
            return true;
        } else if keys::CANCEL.is_pressed(*key_event) {
            self.set_error(/*message*/ None);
            *self.sign_in_state.write().unwrap() =
                SignInState::ByokEntry(ByokEntryState {
                    field: ByokField::ApiKey,
                    provider: state.config.provider,
                    base_url: state.config.base_url,
                    api_key: state.config.api_key,
                    fetching: false,
                });
            self.request_frame.schedule_frame();
            return true;
        } else {
            return true;
        }
        *self.sign_in_state.write().unwrap() = SignInState::ByokModelSelect(state);
        self.request_frame.schedule_frame();
        true
    }

    fn handle_byok_paste(&mut self, pasted: String) -> bool {
        let trimmed = pasted.trim();
        if trimmed.is_empty() {
            return false;
        }

        let mut guard = self.sign_in_state.write().unwrap();
        if let SignInState::ByokEntry(state) = &mut *guard {
            if state.fetching {
                return false;
            }
            let field = match state.field {
                ByokField::Provider => &mut state.provider,
                ByokField::BaseUrl => &mut state.base_url,
                ByokField::ApiKey => &mut state.api_key,
            };
            field.push_str(trimmed);
            drop(guard);
            self.set_error(/*message*/ None);
            self.request_frame.schedule_frame();
            return true;
        }
        false
    }

    fn start_byok_entry(&mut self) {
        self.set_error(/*message*/ None);
        let mut guard = self.sign_in_state.write().unwrap();
        match &*guard {
            SignInState::ByokEntry(_) => {}
            _ => {
                *guard = SignInState::ByokEntry(ByokEntryState::default());
            }
        }
        drop(guard);
        self.request_frame.schedule_frame();
    }

    /// Fetches `GET {base_url}/models` with the entered API key and, on
    /// success, moves the wizard to the model-selection state.
    fn start_byok_model_fetch(&mut self) {
        let (config, base_url, api_key) = {
            let guard = self.sign_in_state.read().unwrap();
            match &*guard {
                SignInState::ByokEntry(state) => (
                    ByokProviderConfig {
                        provider: state.provider.trim().to_string(),
                        base_url: state.base_url.trim().to_string(),
                        api_key: state.api_key.trim().to_string(),
                    },
                    state.base_url.trim().to_string(),
                    state.api_key.trim().to_string(),
                ),
                _ => return,
            }
        };

        let sign_in_state = self.sign_in_state.clone();
        let error = self.error.clone();
        let request_frame = self.request_frame.clone();
        let http_client_factory = self.http_client_factory.clone();
        tokio::spawn(async move {
            let models_url = format!("{}/models", base_url.trim_end_matches('/'));
            let client_pool = RouteAwareClientPool::with_chatgpt_cloudflare_cookies(
                http_client_factory,
                ClientRouteClass::Other,
            )
            .with_legacy_custom_ca_fallback();
            let outcome = match client_pool
                .get(&models_url)
                .header("Authorization", format!("Bearer {api_key}"))
                .header("Accept", "application/json")
                .send()
                .await
            {
                Ok(response) => match response.error_for_status() {
                    Ok(response) => match response.text().await {
                        Ok(body) => parse_byok_models(&body),
                        Err(err) => Err(format!("Failed to read response: {err}")),
                    },
                    Err(err) => Err(format!("Provider rejected the request: {err}")),
                },
                Err(err) => Err(format!("Failed to fetch models: {err}")),
            };

            match outcome {
                Ok(mut models) => {
                    if models.is_empty() {
                        *error.write().unwrap() = Some(
                            "The provider returned no models. Check your API key and base URL."
                                .to_string(),
                        );
                        if let SignInState::ByokEntry(state) = &mut *sign_in_state.write().unwrap()
                        {
                            state.fetching = false;
                        }
                    } else {
                        models.sort();
                        models.dedup();
                        *sign_in_state.write().unwrap() = SignInState::ByokModelSelect(
                            ByokModelSelectState {
                                config,
                                models,
                                highlighted: 0,
                            },
                        );
                    }
                }
                Err(err) => {
                    *error.write().unwrap() = Some(err);
                    if let SignInState::ByokEntry(state) = &mut *sign_in_state.write().unwrap() {
                        state.fetching = false;
                    }
                }
            }
            request_frame.schedule_frame();
        });
    }

    /// Persists the selected provider + model to the user `config.toml` via
    /// the app-server `config/batchWrite` API, then completes the auth step.
    fn start_byok_save(&mut self, config: ByokProviderConfig, model: String) {
        let request_handle = self.app_server_request_handle.clone();
        let sign_in_state = self.sign_in_state.clone();
        let error = self.error.clone();
        let request_frame = self.request_frame.clone();
        tokio::spawn(async move {
            let provider_id = sanitize_provider_id(&config.provider);
            let edits = vec![
                replace_config_value(
                    format!("model_providers.\"{provider_id}\".name"),
                    serde_json::json!(config.provider),
                ),
                replace_config_value(
                    format!("model_providers.\"{provider_id}\".base_url"),
                    serde_json::json!(config.base_url),
                ),
                replace_config_value(
                    format!("model_providers.\"{provider_id}\".experimental_bearer_token"),
                    serde_json::json!(config.api_key),
                ),
                replace_config_value("model_provider", serde_json::json!(provider_id)),
                replace_config_value("model", serde_json::json!(model)),
            ];
            match request_handle
                .request_typed::<ConfigWriteResponse>(ClientRequest::ConfigBatchWrite {
                    request_id: onboarding_request_id(),
                    params: ConfigBatchWriteParams {
                        edits,
                        file_path: None,
                        expected_version: None,
                        reload_user_config: true,
                    },
                })
                .await
            {
                Ok(_) => {
                    *error.write().unwrap() = None;
                    *sign_in_state.write().unwrap() = SignInState::ByokConfigured(config);
                }
                Err(err) => {
                    *error.write().unwrap() =
                        Some(format!("Failed to save provider config: {err}"));
                    *sign_in_state.write().unwrap() = SignInState::ByokSaving(config);
                }
            }
            request_frame.schedule_frame();
        });
    }

    // ------------------------------------------------------------------
    // OpenAI API key entry (built-in provider via auth.json)
    // ------------------------------------------------------------------

    fn handle_api_key_entry_key_event(&mut self, key_event: &KeyEvent) -> bool {
        let mut should_save: Option<String> = None;
        let mut should_request_frame = false;

        {
            let mut guard = self.sign_in_state.write().unwrap();
            if let SignInState::ApiKeyEntry(state) = &mut *guard {
                if keys::CANCEL.is_pressed(*key_event) {
                    *guard = self.default_sign_in_state();
                    self.set_error(/*message*/ None);
                    should_request_frame = true;
                } else if keys::CONFIRM.is_pressed(*key_event) {
                    let trimmed = state.value.trim().to_string();
                    if trimmed.is_empty() {
                        self.set_error(Some("API key cannot be empty".to_string()));
                        should_request_frame = true;
                    } else {
                        should_save = Some(trimmed);
                    }
                } else {
                    match key_event.code {
                        KeyCode::Backspace => {
                            if state.prepopulated_from_env {
                                state.value.clear();
                                state.prepopulated_from_env = false;
                            } else {
                                state.value.pop();
                            }
                            self.set_error(/*message*/ None);
                            should_request_frame = true;
                        }
                        KeyCode::Char(c)
                            if key_event.kind == KeyEventKind::Press
                                && !key_event.modifiers.contains(KeyModifiers::SUPER)
                                && !key_event.modifiers.contains(KeyModifiers::CONTROL)
                                && !key_event.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            if state.prepopulated_from_env {
                                state.value.clear();
                                state.prepopulated_from_env = false;
                            }
                            state.value.push(c);
                            self.set_error(/*message*/ None);
                            should_request_frame = true;
                        }
                        _ => {}
                    }
                }
                // handled; let guard drop before potential save
            } else {
                return false;
            }
        }

        if let Some(api_key) = should_save {
            self.save_api_key(api_key);
        } else if should_request_frame {
            self.request_frame.schedule_frame();
        }
        true
    }

    fn handle_api_key_entry_paste(&mut self, pasted: String) -> bool {
        let trimmed = pasted.trim();
        if trimmed.is_empty() {
            return false;
        }

        let mut guard = self.sign_in_state.write().unwrap();
        if let SignInState::ApiKeyEntry(state) = &mut *guard {
            if state.prepopulated_from_env {
                state.value = trimmed.to_string();
                state.prepopulated_from_env = false;
            } else {
                state.value.push_str(trimmed);
            }
            self.set_error(/*message*/ None);
        } else {
            return false;
        }

        drop(guard);
        self.request_frame.schedule_frame();
        true
    }

    fn start_api_key_entry(&mut self) {
        if !self.is_api_login_allowed() {
            self.disallow_api_login();
            return;
        }
        self.set_error(/*message*/ None);
        let prefill_from_env = read_openai_api_key_from_env();
        let mut guard = self.sign_in_state.write().unwrap();
        match &mut *guard {
            SignInState::ApiKeyEntry(state) => {
                if state.value.is_empty() {
                    if let Some(prefill) = prefill_from_env {
                        state.value = prefill;
                        state.prepopulated_from_env = true;
                    } else {
                        state.prepopulated_from_env = false;
                    }
                }
            }
            _ => {
                *guard = SignInState::ApiKeyEntry(ApiKeyInputState {
                    value: prefill_from_env.clone().unwrap_or_default(),
                    prepopulated_from_env: prefill_from_env.is_some(),
                });
            }
        }
        drop(guard);
        self.request_frame.schedule_frame();
    }

    fn save_api_key(&mut self, api_key: String) {
        if !self.is_api_login_allowed() {
            self.disallow_api_login();
            return;
        }
        self.set_error(/*message*/ None);
        let request_handle = self.app_server_request_handle.clone();
        let sign_in_state = self.sign_in_state.clone();
        let error = self.error.clone();
        let request_frame = self.request_frame.clone();
        tokio::spawn(async move {
            match request_handle
                .request_typed::<LoginAccountResponse>(ClientRequest::LoginAccount {
                    request_id: onboarding_request_id(),
                    params: LoginAccountParams::ApiKey {
                        api_key: api_key.clone(),
                    },
                })
                .await
            {
                Ok(LoginAccountResponse::ApiKey {}) => {
                    *error.write().unwrap() = None;
                    *sign_in_state.write().unwrap() = SignInState::ApiKeyConfigured;
                }
                Ok(other) => {
                    *error.write().unwrap() = Some(format!(
                        "Unexpected account/login/start response: {other:?}"
                    ));
                    *sign_in_state.write().unwrap() = SignInState::ApiKeyEntry(ApiKeyInputState {
                        value: api_key,
                        prepopulated_from_env: false,
                    });
                }
                Err(err) => {
                    *error.write().unwrap() = Some(format!("Failed to save API key: {err}"));
                    *sign_in_state.write().unwrap() = SignInState::ApiKeyEntry(ApiKeyInputState {
                        value: api_key,
                        prepopulated_from_env: false,
                    });
                }
            }
            request_frame.schedule_frame();
        });
    }

    pub(crate) fn on_account_updated(&mut self, notification: AccountUpdatedNotification) {
        self.login_status = notification
            .auth_mode
            .map(|auth_mode| {
                LoginStatus::AuthMode(match auth_mode {
                    ApiAuthMode::ApiKey => AuthMode::ApiKey,
                    ApiAuthMode::Chatgpt => AuthMode::Chatgpt,
                    ApiAuthMode::ChatgptAuthTokens => AuthMode::ChatgptAuthTokens,
                    ApiAuthMode::Headers => AuthMode::Headers,
                    ApiAuthMode::AgentIdentity => AuthMode::AgentIdentity,
                    ApiAuthMode::PersonalAccessToken => AuthMode::PersonalAccessToken,
                    ApiAuthMode::BedrockApiKey => AuthMode::BedrockApiKey,
                    ApiAuthMode::BedrockAccessKeys => AuthMode::BedrockAccessKeys,
                })
            })
            .unwrap_or(LoginStatus::NotAuthenticated);
    }
}

/// Maps a user-entered provider name to a suggested base URL, if known.
fn suggested_base_url(provider: &str) -> Option<&'static str> {
    match provider.trim().to_ascii_lowercase().as_str() {
        "openai" => Some("https://api.openai.com/v1"),
        "openrouter" => Some("https://openrouter.ai/api/v1"),
        "litellm" => Some("http://localhost:4000/v1"),
        "groq" => Some("https://api.groq.com/openai/v1"),
        "ollama" => Some("http://localhost:11434/v1"),
        "deepseek" => Some("https://api.deepseek.com/v1"),
        "mistral" => Some("https://api.mistral.ai/v1"),
        "xai" | "grok" => Some("https://api.x.ai/v1"),
        "together" => Some("https://api.together.xyz/v1"),
        "fireworks" => Some("https://api.fireworks.ai/inference/v1"),
        "gemini" => Some("https://generativelanguage.googleapis.com/v1beta/openai"),
        _ => None,
    }
}

/// Normalizes a provider name into a config key (lowercase, `[a-z0-9_-]`).
fn sanitize_provider_id(provider: &str) -> String {
    let mut id = String::new();
    let mut last_dash = true;
    for c in provider.trim().chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            id.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            id.push('-');
            last_dash = true;
        }
    }
    while id.ends_with('-') {
        id.pop();
    }
    if id.is_empty() {
        "custom".to_string()
    } else {
        id
    }
}

#[derive(Deserialize, Debug, Clone)]
struct ByokModelsResponse {
    #[serde(default)]
    data: Vec<ByokModelEntry>,
    #[serde(default)]
    models: Vec<ByokModelEntry>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
enum ByokModelEntry {
    Id(String),
    Object { id: String },
}

impl ByokModelEntry {
    fn id(&self) -> String {
        match self {
            Self::Id(id) => id.clone(),
            Self::Object { id } => id.clone(),
        }
    }
}

/// Parses an OpenAI-compatible `/models` response body into sorted model ids.
fn parse_byok_models(body: &str) -> Result<Vec<String>, String> {
    let parsed: ByokModelsResponse = serde_json::from_str(body)
        .map_err(|err| format!("Unexpected /models response: {err}"))?;
    let mut models: Vec<String> = parsed
        .data
        .into_iter()
        .chain(parsed.models)
        .map(|entry| entry.id())
        .filter(|id| !id.is_empty())
        .collect();
    models.sort();
    models.dedup();
    Ok(models)
}

impl StepStateProvider for AuthModeWidget {
    fn get_step_state(&self) -> StepState {
        let sign_in_state = self.sign_in_state.read().unwrap();
        match &*sign_in_state {
            SignInState::PickMode
            | SignInState::ByokEntry(_)
            | SignInState::ByokModelSelect(_)
            | SignInState::ByokSaving(_)
            | SignInState::ApiKeyEntry(_)
            | SignInState::Bedrock(_) => StepState::InProgress,
            SignInState::ByokConfigured(_)
            | SignInState::ApiKeyConfigured
            | SignInState::BedrockConfigured => StepState::Complete,
        }
    }
}

impl WidgetRef for AuthModeWidget {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let sign_in_state = self.sign_in_state.read().unwrap();
        match &*sign_in_state {
            SignInState::PickMode => {
                self.render_pick_mode(area, buf);
            }
            SignInState::ByokEntry(state) => {
                self.render_byok_entry(area, buf, state);
            }
            SignInState::ByokModelSelect(state) => {
                self.render_byok_model_select(area, buf, state);
            }
            SignInState::ByokSaving(config) => {
                self.render_byok_saving(area, buf, config);
            }
            SignInState::ByokConfigured(config) => {
                self.render_byok_configured(area, buf, config);
            }
            SignInState::ApiKeyEntry(state) => {
                self.render_api_key_entry(area, buf, state);
            }
            SignInState::ApiKeyConfigured => {
                self.render_api_key_configured(area, buf);
            }
            SignInState::Bedrock(state) => {
                state.render(area, buf, self.error_message());
            }
            SignInState::BedrockConfigured => {
                Paragraph::new("✓ Amazon Bedrock configured".green())
                    .wrap(Wrap { trim: false })
                    .render(area, buf);
            }
        }
    }
}

impl AuthModeWidget {
    fn render_api_key_entry(&self, area: Rect, buf: &mut Buffer, state: &ApiKeyInputState) {
        let [intro_area, input_area, footer_area] = Layout::vertical([
            Constraint::Min(4),
            Constraint::Length(3),
            Constraint::Min(2),
        ])
        .areas(area);

        let mut intro_lines: Vec<Line> = vec![
            Line::from(vec![
                "> ".into(),
                "Use your own OpenAI API key for usage-based billing".bold(),
            ]),
            "".into(),
            "  Paste or type your API key below. It will be stored locally in auth.json.".into(),
            "".into(),
        ];
        if state.prepopulated_from_env {
            intro_lines.push("  Detected OPENAI_API_KEY environment variable.".into());
            intro_lines.push(
                "  Paste a different key if you prefer to use another account."
                    .dim()
                    .into(),
            );
            intro_lines.push("".into());
        }
        Paragraph::new(intro_lines)
            .wrap(Wrap { trim: false })
            .render(intro_area, buf);

        let content_line: Line = if state.value.is_empty() {
            vec!["Paste or type your API key".dim()].into()
        } else {
            Line::from(state.value.clone())
        };
        Paragraph::new(content_line)
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .title("API key")
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .render(input_area, buf);

        let mut footer_lines: Vec<Line> = vec![
            Line::from(vec![
                "  Press ".dim(),
                self.confirm_binding().into(),
                " to save".dim(),
            ]),
            Line::from(vec![
                "  Press ".dim(),
                self.cancel_binding().into(),
                " to go back".dim(),
            ]),
        ];
        if let Some(error) = self.error_message() {
            footer_lines.push("".into());
            footer_lines.push(error.red().into());
        }
        Paragraph::new(footer_lines)
            .wrap(Wrap { trim: false })
            .render(footer_area, buf);
    }
}

// The visible-lines helper is re-exported for hyperlink-aware rendering; keep
// the import used so clippy does not flag it when other renders change.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::legacy_core::config::ConfigBuilder;
    use rexux_app_server_client::AppServerRequestHandle;
    use rexux_app_server_client::DEFAULT_IN_PROCESS_CHANNEL_CAPACITY;
    use rexux_app_server_client::InProcessAppServerClient;
    use rexux_app_server_client::InProcessClientStartArgs;
    use rexux_arg0::Arg0DispatchPaths;
    use rexux_cloud_config::cloud_config_bundle_loader_for_storage;
    use pretty_assertions::assert_eq;
    use tempfile::TempDir;

    async fn test_widget() -> (AuthModeWidget, TempDir) {
        let rexux_home = TempDir::new().unwrap();
        let rexux_home_path = rexux_home.path().to_path_buf();
        let config = ConfigBuilder::default()
            .rexux_home(rexux_home_path.clone())
            .build()
            .await
            .unwrap();
        let auth_config = config.auth_config();
        let http_client_factory = config.http_client_factory();
        let client = InProcessAppServerClient::start(InProcessClientStartArgs {
            arg0_paths: Arg0DispatchPaths::default(),
            config: Arc::new(config),
            cli_overrides: Vec::new(),
            loader_overrides: Default::default(),
            strict_config: false,
            cloud_config_bundle: cloud_config_bundle_loader_for_storage(
                auth_config.clone(),
                /*enable_rexux_api_key_env*/ false,
            )
            .await
            .expect("test cloud config loader"),
            feedback: rexux_feedback::RexuxFeedback::new(),
            log_db: None,
            state_db: None,
            environment_manager: Arc::new(
                rexux_app_server_client::EnvironmentManager::default_for_tests(),
            ),
            config_warnings: Vec::new(),
            session_source: serde_json::from_value(serde_json::json!("cli"))
                .expect("cli session source should deserialize"),
            enable_rexux_api_key_env: false,
            client_name: "test".to_string(),
            client_version: "test".to_string(),
            experimental_api: true,
            mcp_server_openai_form_elicitation: false,
            opt_out_notification_methods: Vec::new(),
            channel_capacity: DEFAULT_IN_PROCESS_CHANNEL_CAPACITY,
        })
        .await
        .unwrap();
        let widget = AuthModeWidget {
            request_frame: FrameRequester::test_dummy(),
            highlighted_mode: SignInOption::Byok,
            error: Arc::new(RwLock::new(None)),
            sign_in_state: Arc::new(RwLock::new(SignInState::PickMode)),
            login_status: LoginStatus::NotAuthenticated,
            app_server_request_handle: AppServerRequestHandle::InProcess(client.request_handle()),
            auth_config,
            bedrock_setup_enabled: false,
            animations_enabled: true,
            animations_suppressed: std::cell::Cell::new(false),
            http_client_factory,
        };
        (widget, rexux_home)
    }

    async fn widget_forced_chatgpt() -> (AuthModeWidget, TempDir) {
        let (mut widget, tmp) = test_widget().await;
        widget.auth_config.forced_login_method = Some(ForcedLoginMethod::Chatgpt);
        (widget, tmp)
    }

    #[tokio::test]
    async fn byok_entry_blocked_when_chatgpt_forced() {
        let (mut widget, _tmp) = widget_forced_chatgpt().await;

        widget.handle_sign_in_option(SignInOption::Byok);

        assert_eq!(
            widget.error_message().as_deref(),
            Some(API_KEY_DISABLED_MESSAGE)
        );
        assert!(matches!(
            &*widget.sign_in_state.read().unwrap(),
            SignInState::PickMode
        ));
    }

    #[tokio::test]
    async fn byok_wizard_advances_through_fields() {
        let (mut widget, _tmp) = test_widget().await;
        widget.handle_sign_in_option(SignInOption::Byok);

        assert!(matches!(
            &*widget.sign_in_state.read().unwrap(),
            SignInState::ByokEntry(state) if state.field == ByokField::Provider
        ));

        // Provider name → base URL (with suggestion for a known provider).
        widget.handle_byok_paste("openrouter".to_string());
        widget.handle_byok_key_event(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(
            &*widget.sign_in_state.read().unwrap(),
            SignInState::ByokEntry(state)
                if state.field == ByokField::BaseUrl
                    && state.base_url == "https://openrouter.ai/api/v1"
        ));

        // Esc walks back one field at a time.
        widget.handle_byok_key_event(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(matches!(
            &*widget.sign_in_state.read().unwrap(),
            SignInState::ByokEntry(state) if state.field == ByokField::Provider
        ));
        widget.handle_byok_key_event(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(matches!(
            &*widget.sign_in_state.read().unwrap(),
            SignInState::PickMode
        ));
    }

    #[tokio::test]
    async fn base_url_requires_scheme() {
        let (mut widget, _tmp) = test_widget().await;
        widget.handle_sign_in_option(SignInOption::Byok);
        widget.handle_byok_paste("myproxy".to_string());
        widget.handle_byok_key_event(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        widget.handle_byok_paste("myproxy.local".to_string());
        widget.handle_byok_key_event(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(
            widget.error_message().as_deref(),
            Some("Base URL must start with http:// or https://")
        );
        assert!(matches!(
            &*widget.sign_in_state.read().unwrap(),
            SignInState::ByokEntry(state) if state.field == ByokField::BaseUrl
        ));
    }

    #[tokio::test]
    async fn bedrock_option_requires_feature_and_api_login_permission() {
        let (mut widget, _tmp) = test_widget().await;
        assert_eq!(
            widget.displayed_sign_in_options(),
            vec![SignInOption::Byok, SignInOption::ApiKey]
        );

        widget.bedrock_setup_enabled = true;
        assert_eq!(
            widget.displayed_sign_in_options(),
            vec![
                SignInOption::Byok,
                SignInOption::ApiKey,
                SignInOption::Bedrock,
            ]
        );

        let area = Rect::new(0, 0, 76, 19);
        let mut buffer = Buffer::empty(area);
        widget.render_pick_mode(area, &mut buffer);
        let mut rows = (area.top()..area.bottom())
            .map(|row| {
                (area.left()..area.right())
                    .map(|column| buffer[(column, row)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>();
        while rows.last().is_some_and(String::is_empty) {
            rows.pop();
        }
        assert_eq!(
            rows.join("\n"),
            [
                "  Connect a provider to use Rexux",
                "  bring your own API key (BYOK)",
                "",
                "> 1. Connect a custom provider",
                "     Enter a base URL and API key",
                "",
                "  2. Use an OpenAI API key",
                "     Pay for what you use",
                "",
                "  3. Use Amazon Bedrock",
                "     Connect using your AWS credentials",
                "",
                "  Press enter to continue",
            ]
            .join("\n")
        );

        widget.auth_config.forced_login_method = Some(ForcedLoginMethod::Chatgpt);
        assert_eq!(
            widget.displayed_sign_in_options(),
            vec![SignInOption::Byok]
        );
    }

    #[tokio::test]
    async fn saving_api_key_is_blocked_when_chatgpt_forced() {
        let (mut widget, _tmp) = widget_forced_chatgpt().await;

        widget.save_api_key("sk-test".to_string());

        assert_eq!(
            widget.error_message().as_deref(),
            Some(API_KEY_DISABLED_MESSAGE)
        );
        assert!(matches!(
            &*widget.sign_in_state.read().unwrap(),
            SignInState::PickMode
        ));
        assert_eq!(widget.login_status, LoginStatus::NotAuthenticated);
    }

    #[test]
    fn sanitize_provider_id_normalizes_names() {
        assert_eq!(sanitize_provider_id("OpenRouter"), "openrouter");
        assert_eq!(sanitize_provider_id("My Cool Proxy!"), "my-cool-proxy");
        assert_eq!(sanitize_provider_id("  "), "custom");
        assert_eq!(sanitize_provider_id("---"), "custom");
    }

    #[test]
    fn parse_byok_models_handles_data_and_plain_lists() {
        let models = parse_byok_models(
            r#"{"data":[{"id":"b-model","other":1},{"id":"a-model"}],"extra":true}"#,
        )
        .expect("parse");
        assert_eq!(models, vec!["a-model", "b-model"]);

        let models = parse_byok_models(r#"{"models":["z-model"]}"#).expect("parse");
        assert_eq!(models, vec!["z-model"]);

        assert!(parse_byok_models("not json").is_err());
    }

    /// Collects all buffer cell symbols that contain the OSC 8 open sequence
    /// for the given URL.  Returns the concatenated "inner" characters.
    fn collect_osc8_chars(buf: &Buffer, area: Rect, url: &str) -> String {
        let open = format!("\x1B]8;;{url}\x07");
        let close = "\x1B]8;;\x07";
        let mut chars = String::new();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                let sym = buf[(x, y)].symbol();
                if let Some(rest) = sym.strip_prefix(open.as_str())
                    && let Some(ch) = rest.strip_suffix(close)
                {
                    chars.push_str(ch);
                }
            }
        }
        chars
    }

    #[test]
    fn mark_url_hyperlink_wraps_cyan_underlined_cells() {
        let url = "https://example.com";
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);

        // Manually write some cyan+underlined characters to simulate a rendered URL.
        for (i, ch) in "example".chars().enumerate() {
            let cell = &mut buf[(i as u16, 0)];
            cell.set_symbol(&ch.to_string());
            cell.fg = Color::Cyan;
            cell.modifier = Modifier::UNDERLINED;
        }
        // Leave a plain cell that should NOT be marked.
        buf[(7, 0)].set_symbol("X");

        mark_url_hyperlink(&mut buf, area, url);

        // Each cyan+underlined cell should now carry the OSC 8 wrapper.
        let found = collect_osc8_chars(&buf, area, url);
        assert_eq!(found, "example");

        // The plain "X" cell should be untouched.
        assert_eq!(buf[(7, 0)].symbol(), "X");
    }

    #[test]
    fn mark_url_hyperlink_sanitizes_control_chars() {
        let area = Rect::new(0, 0, 10, 1);
        let mut buf = Buffer::empty(area);

        // One cyan+underlined cell to mark.
        let cell = &mut buf[(0, 0)];
        cell.set_symbol("a");
        cell.fg = Color::Cyan;
        cell.modifier = Modifier::UNDERLINED;

        // URL contains ESC and BEL that could break the OSC 8 sequence.
        let malicious_url = "https://evil.com/\x1B]8;;\x07injected";
        mark_url_hyperlink(&mut buf, area, malicious_url);

        let sym = buf[(0, 0)].symbol().to_string();
        // The sanitized URL retains `]` (printable) but strips ESC and BEL.
        let sanitized = "https://evil.com/]8;;injected";
        assert!(
            sym.contains(sanitized),
            "symbol should contain sanitized URL, got: {sym:?}"
        );
        // The injected close-sequence must not survive: \x1B and \x07 are gone.
        assert!(
            !sym.contains("\x1B]8;;\x07injected"),
            "symbol must not contain raw control chars from URL"
        );
    }
}
