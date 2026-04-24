//! Source-facing helpers that centralize the derivation of the agent-icon shape
//! ([`IconWithStatusVariant`]) from the underlying state models.
//!
//! Each helper is a thin adapter over one data source (a live [`TerminalView`], or a
//! [`ConversationOrTask`] card row). Surfaces call the helper for whichever source they hold
//! and feed the resulting variant into [`render_icon_with_status`]. See
//! `specs/REMOTE-1458/TECH.md` for the cross-surface consistency contract.
//!
//! The public helpers are thin adapters that gather primitive inputs from their respective
//! sources and delegate to the pure inner functions
//! ([`agent_icon_variant_from_terminal_inputs`], [`agent_icon_variant_for_task`]).
//! Keeping the decision logic pure lets the cross-surface consistency tests in
//! `agent_icon_tests.rs` exercise every canonical run state without a live `AppContext`.
use warp_cli::agent::Harness;
use warpui::AppContext;
use warpui::SingletonEntity;

use crate::ai::agent::conversation::ConversationStatus;
use crate::ai::agent_conversations_model::ConversationOrTask;
use crate::terminal::cli_agent_sessions::listener::agent_supports_rich_status;
use crate::terminal::cli_agent_sessions::CLIAgentSessionsModel;
use crate::terminal::view::TerminalView;
use crate::terminal::CLIAgent;
use crate::ui_components::icon_with_status::IconWithStatusVariant;

/// Primitive inputs to the terminal-view waterfall, gathered once from the live
/// [`TerminalView`] / [`AppContext`]. Keeping the decision logic in terms of these
/// primitives makes it testable without a live app.
pub(crate) struct TerminalIconInputs {
    pub is_ambient: bool,
    pub cli_session: Option<CLISessionInputs>,
    /// The CLI agent corresponding to the currently selected cloud harness, when the selection
    /// is a third-party (non-Oz) harness. `None` for Oz or when no harness is selected.
    pub ambient_selected_third_party_cli_agent: Option<CLIAgent>,
    /// The conversation status that the terminal view would surface in its status-icon slot.
    pub selected_conversation_status: Option<ConversationStatus>,
    /// Whether the terminal view currently has a selected conversation (ambient or local).
    pub has_selected_conversation: bool,
}

/// CLI-session-derived inputs for the terminal waterfall.
pub(crate) struct CLISessionInputs {
    pub agent: CLIAgent,
    /// Whether the session is backed by a plugin listener (plugin-backed wins step 1 of the
    /// waterfall; command-detected falls through to step 2).
    pub has_listener: bool,
    pub status: ConversationStatus,
    /// Whether the agent's session handler exposes rich status (plugin-backed handlers report
    /// rich status; Codex's OSC 9 handler does not).
    pub supports_rich_status: bool,
}

/// Pure waterfall from primitive inputs to an [`IconWithStatusVariant`]. Mirrors the
/// resolution order documented on [`terminal_view_agent_icon_variant`].
pub(crate) fn agent_icon_variant_from_terminal_inputs(
    inputs: &TerminalIconInputs,
) -> Option<IconWithStatusVariant> {
    let is_plugin_backed = inputs.cli_session.as_ref().is_some_and(|s| s.has_listener);

    // 1. Plugin-backed CLI session with a known agent: observed reality wins.
    if let Some(session) = inputs
        .cli_session
        .as_ref()
        .filter(|s| s.has_listener)
        .filter(|s| !matches!(s.agent, CLIAgent::Unknown))
    {
        return Some(IconWithStatusVariant::CLIAgent {
            agent: session.agent,
            status: if session.supports_rich_status {
                Some(session.status.clone())
            } else {
                None
            },
            is_ambient: inputs.is_ambient,
        });
    }

    // 2. Non-plugin-backed, command-detected CLI session.
    if let Some(session) = inputs
        .cli_session
        .as_ref()
        .filter(|_| !is_plugin_backed)
        .filter(|s| !matches!(s.agent, CLIAgent::Unknown))
    {
        return Some(IconWithStatusVariant::CLIAgent {
            agent: session.agent,
            status: None,
            is_ambient: inputs.is_ambient,
        });
    }

    // 3. Ambient agent with a selected third-party harness (Claude / Gemini). Render the
    //    harness's brand circle immediately once the user commits, even before the harness
    //    CLI is running in the sandbox.
    if inputs.is_ambient {
        if let Some(agent) = inputs.ambient_selected_third_party_cli_agent {
            return Some(IconWithStatusVariant::CLIAgent {
                agent,
                status: inputs.selected_conversation_status.clone(),
                is_ambient: true,
            });
        }
    }

    // 4. Selected conversation OR ambient (Oz) terminal: Oz agent variant.
    if inputs.has_selected_conversation || inputs.is_ambient {
        return Some(IconWithStatusVariant::OzAgent {
            status: inputs.selected_conversation_status.clone(),
            is_ambient: inputs.is_ambient,
        });
    }

    None
}

/// Pure task-card logic: maps a harness config name (the lowercase string recorded in
/// `HarnessConfig::harness_type`, e.g. `"claude"`, `"gemini"`, `"oz"`) and the task's
/// current status into an [`IconWithStatusVariant`]. Task cards are always ambient.
pub(crate) fn agent_icon_variant_for_task(
    harness_config_name: Option<&str>,
    status: ConversationStatus,
) -> IconWithStatusVariant {
    let harness = harness_config_name
        .map(Harness::from_config_name)
        .unwrap_or(Harness::Oz);
    match CLIAgent::from_harness(harness) {
        Some(agent) => IconWithStatusVariant::CLIAgent {
            agent,
            status: Some(status),
            is_ambient: true,
        },
        None => IconWithStatusVariant::OzAgent {
            status: Some(status),
            is_ambient: true,
        },
    }
}

/// Returns the agent-icon variant for a live [`TerminalView`], or `None` when the terminal is
/// not an agent surface (plain terminal / shell / empty conversation).
///
/// Resolution order (see `specs/REMOTE-1458/TECH.md` §5):
/// 1. A plugin-backed [`CLIAgentSessionsModel`] session (observed reality) wins.
/// 2. A non-plugin-backed, command-detected CLI session wins next.
/// 3. An ambient agent with a selected third-party harness uses the harness's CLI brand even
///    before the harness CLI has started running in the sandbox. **This is the fix for the
///    pre-setup icon bug.**
/// 4. A selected conversation or ambient Oz run falls back to the Oz agent variant.
/// 5. Everything else returns `None` so the caller renders a plain-terminal indicator.
pub(crate) fn terminal_view_agent_icon_variant(
    terminal_view: &TerminalView,
    app: &AppContext,
) -> Option<IconWithStatusVariant> {
    let cli_agent_session = CLIAgentSessionsModel::as_ref(app).session(terminal_view.id());
    let inputs = TerminalIconInputs {
        is_ambient: terminal_view.is_ambient_agent_session(app),
        cli_session: cli_agent_session.map(|session| CLISessionInputs {
            agent: session.agent,
            has_listener: session.listener.is_some(),
            status: session.status.to_conversation_status(),
            supports_rich_status: agent_supports_rich_status(&session.agent),
        }),
        ambient_selected_third_party_cli_agent: terminal_view
            .ambient_agent_view_model()
            .as_ref(app)
            .selected_third_party_cli_agent(),
        selected_conversation_status: terminal_view.selected_conversation_status_for_display(app),
        has_selected_conversation: terminal_view
            .selected_conversation_display_title(app)
            .is_some(),
    };
    agent_icon_variant_from_terminal_inputs(&inputs)
}

/// Returns the agent-icon variant for a [`ConversationOrTask`] card row.
///
/// Task rows resolve their harness from `agent_config_snapshot.harness`; conversation rows
/// have no harness signal and always render as local Oz per the product spec.
pub(crate) fn conversation_or_task_agent_icon_variant(
    src: &ConversationOrTask<'_>,
    app: &AppContext,
) -> Option<IconWithStatusVariant> {
    match src {
        ConversationOrTask::Task(task) => {
            let harness_name = task
                .agent_config_snapshot
                .as_ref()
                .and_then(|s| s.harness.as_ref())
                .map(|h| h.harness_type.as_str());
            Some(agent_icon_variant_for_task(harness_name, src.status(app)))
        }
        ConversationOrTask::Conversation(_) => Some(IconWithStatusVariant::OzAgent {
            status: Some(src.status(app)),
            is_ambient: false,
        }),
    }
}

#[cfg(test)]
#[path = "agent_icon_tests.rs"]
mod tests;
