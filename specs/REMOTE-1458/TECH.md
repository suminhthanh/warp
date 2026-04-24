# REMOTE-1458: Tech Spec — Unified agent icon-with-status
## Problem
Agent-icon rendering is duplicated across four surfaces and each surface re-derives the underlying `{agent kind, status, is_ambient}` tuple from different primitives. Today's logic is also incomplete: the vertical tab icon doesn't flip to the selected third-party harness until the harness CLI is detected in `CLIAgentSessionsModel`, leaving a stale "Oz cloud" icon during the entire setup phase for Claude / Gemini cloud runs. See `specs/REMOTE-1458/PRODUCT.md` for desired behavior.
## Relevant code
- `app/src/ui_components/icon_with_status.rs:12-250` — `IconWithStatusVariant`, `IconWithStatusSizing`, `render_icon_with_status`, `render_with_cloud_status_badge`, `OZ_AMBIENT_BACKGROUND_COLOR`.
- `app/src/workspace/view/vertical_tabs.rs:2240-2347` — `resolve_icon_with_status_variant` (per-row live icon).
- `app/src/workspace/view/vertical_tabs.rs:2486-2527` — `TypedPane::summary_pane_kind` (aggregate-icon computation).
- `app/src/workspace/view/vertical_tabs.rs:111-147` — `VERTICAL_TABS_SIZING` and `VERTICAL_TABS_AGENT_SIZING`, cloud lobe constants.
- `app/src/workspace/view/vertical_tabs.rs:746-750` — `SummaryPaneKind::OzAgent`/`CLIAgent { is_ambient }`.
- `app/src/workspace/view/vertical_tabs.rs:3644-3722` — `ambient_agent_variant` helper used in the "summary mode" aggregate icon path.
- `app/src/terminal/view/pane_impl.rs:268-369` — `render_header_title`, which picks an indicator per terminal state.
- `app/src/terminal/view/pane_impl.rs:766-819` — `render_agent_indicator`.
- `app/src/terminal/view/pane_impl.rs:861-900` — `render_ambient_agent_indicator`.
- `app/src/terminal/view/pane_impl.rs:823-859` — `render_terminal_mode_indicator` (remains unchanged).
- `app/src/terminal/view/pane_impl.rs:978-1043` — `is_in_cloud_agent_setup_phase`, `selected_conversation_status`, `selected_conversation_status_for_display`.
- `app/src/terminal/view/ambient_agent/model.rs:245-278` — `selected_harness`, `is_third_party_harness`, `selected_third_party_cli_agent`.
- `app/src/terminal/view.rs:6573` — `TerminalView::ambient_agent_view_model` accessor.
- `app/src/terminal/cli_agent_sessions/mod.rs:275-301` — `CLIAgentSessionsModel::session`.
- `app/src/terminal/cli_agent.rs:107-260` — `CLIAgent` enum, brand colors, icons.
- `crates/warp_cli/src/agent.rs:118-131` — `Harness` enum.
- `app/src/workspace/view/conversation_list/item.rs:156-220` — `render_item` (inline conversation list row leading-slot icon).
- `app/src/ai/agent_conversations_model.rs:337-400` — `ConversationOrTask::title`, `status`, `display_status`.
- `app/src/ai/agent_conversations_model.rs:516-520` — `environment_id` on `ConversationOrTask::Task`.
- `app/src/ai/ambient_agents/task.rs` — `AmbientAgentTask`, `AgentConfigSnapshot.harness: Option<HarnessConfig>`.
- `app/src/ai/agent_management/notifications/item.rs:45-120` — `NotificationSourceAgent`, `NotificationItem`.
- `app/src/ai/agent_management/notifications/item_rendering.rs:402-441` — `render_agent_avatar`, `NOTIFICATION_AVATAR_SIZING`.
- `app/src/ai/agent_management/agent_management_model.rs:126-197,314-371,394-433,505-522` — `handle_cli_agent_session_event`, `handle_history_event_for_mailbox`, `add_notification`, `resolve_git_branch_for_terminal_view` (the lookup pattern we'll reuse for `is_ambient`).
## Current state
Vertical tabs contains the only surface that already uses the full `IconWithStatusVariant` shape; `resolve_icon_with_status_variant` walks this waterfall (paraphrased):
1. If `CLIAgentSessionsModel::session(terminal_view.id())` exists with a listener and a non-Unknown agent: `CLIAgent { agent, status: from session, is_ambient }`.
2. Else if a non-plugin-backed CLI session exists: `CLIAgent { agent, status: None, is_ambient }`.
3. Else if a conversation exists or the terminal is an ambient agent: `OzAgent { status: selected_conversation_status_for_display, is_ambient }`.
4. Else: `Neutral { Terminal }`.
`TypedPane::summary_pane_kind` does its own version of the same waterfall, but without status — producing `SummaryPaneKind::{OzAgent, CLIAgent, Terminal}`. The two waterfalls agree on the happy path but diverge on the ambient-but-no-session case (both return OzAgent, which is the bug driving this spec).
The pane header's indicator slot uses plain glyphs. `render_ambient_agent_indicator` renders `Icon::OzCloud` in a constrained box with no brand circle, no status, no harness specialization. `render_agent_indicator` renders either `WarpIcon::Oz` / `WarpIcon::OzCloud` OR a raw status element — never a combined circle+status.
The inline conversation list row's leading slot (in `conversation_list/item.rs::render_item`) switches between two plain icons based on `ConversationOrTask::is_ambient_agent_conversation()`: a raw `Icon::Cloud` glyph for ambient rows, or `render_status_element(&conversation.status(app), font_size, appearance)` for local rows. There is no brand color, harness identity, or cloud-lobe treatment in that slot at all.
The Agent Management View's card leading slot (`AgentManagementView::render_header_row` in `app/src/ai/agent_management/view.rs`) also calls `render_status_element` with the display status. That surface is out of scope for this pass — its richer card layout (action buttons, session status labels, creator avatars) owns its own status treatment and we leave it untouched.
Notifications call `render_agent_avatar(NotificationSourceAgent, NotificationCategory, theme)` which maps to `IconWithStatusVariant::{OzAgent, CLIAgent}` with `is_ambient: false` hardcoded and a category-derived status.
## Architecture: helpers, not a singleton
Before diving into the per-surface changes, one architectural decision: centralization happens via a small set of source-facing *helpers* that return `Option<IconWithStatusVariant>`, not via a new singleton model that caches computed descriptors.
A singleton was considered. It would offer "guaranteed centralization" and "one call site per surface" in exchange for keeping a cached `AgentIconDescriptor` keyed by some notion of run identity. The tradeoffs that kept us away from it:
- **Keying.** A logical run has multiple identities (`terminal_view_id` when open, `AmbientAgentTaskId` remotely, `AIConversationId` in history, plus the notification's own `terminal_view_id`). A singleton would need overlapping indices and a merge policy for the same run viewed via different keys. On-demand helpers sidestep this entirely because each surface already holds exactly one identity.
- **Update plumbing.** Every emit path that mutates the underlying state would have to push into the singleton: `CLIAgentSessionsModel` (6 event variants), `AmbientAgentViewModel` (11 event variants), `BlocklistAIHistoryModel` (conversation status updates), `AgentConversationsModel` (task state updates). ~25 new write sites, each a potential source of stale state.
- **Derivation cost.** The computation is a small `match` over a few field reads. It is cheaper to recompute at render time than to maintain a cache. Cached derived state is also not an idiomatic pattern in WarpUI.
- **Testability.** Source helpers with a source argument are trivially testable by constructing the source. Singleton state requires mocking.
The Option-3 trait path (see *Follow-ups*) is the natural escalation if we ever grow past two source types and want a shared descriptor contract without a cache. Do that later, not now.
## Proposed changes
### 1. Extend `IconWithStatusVariant` with `is_ambient` + cloud-lobe rendering
`app/src/ui_components/icon_with_status.rs`
Extend `IconWithStatusVariant::OzAgent` and `IconWithStatusVariant::CLIAgent` to carry `is_ambient: bool`. Add three fields to `IconWithStatusSizing` — `cloud_icon_size: f32`, `cloud_offset: (f32, f32)`, `status_in_cloud_icon_size: f32` — so each surface can tune the lobe independently. Introduce `render_with_cloud_status_badge` as a private helper that overlays a white `WarpIcon::CloudFilled` at the bottom-right of the brand circle with the status icon (if any) centered inside; invoke it from the agent variants when `is_ambient` is true in place of the normal status ring. For ambient Oz runs, swap the circle background to the brand purple `OZ_AMBIENT_BACKGROUND_COLOR = ColorU { r: 203, g: 176, b: 247, a: 255 }` and render `WarpIcon::OzCloud` instead of `WarpIcon::Oz`. Fill in the new sizing fields for every existing `IconWithStatusSizing` literal (notifications, vertical tabs, new pane header constant, new conversation-card constant); values can be small placeholders on surfaces that don't currently paint a lobe — threading `is_ambient: true` is what actually enables lobe rendering.
### 2. Centralize derivation via per-source helpers (no new types)
Keep `IconWithStatusVariant` as the canonical render-time shape. Introduce one pure helper per data source, each returning `Option<IconWithStatusVariant>` covering only the agent variants (`OzAgent`, `CLIAgent`). Non-agent variants (`Neutral`, `NeutralElement`) stay at the call site.
The helpers share three primitive mappers — each mapper lives in exactly one place and is invoked by every helper that needs it:
- `CLIAgent::from_harness(Harness) -> Option<CLIAgent>`, added on `CLIAgent` in `app/src/terminal/cli_agent.rs`. Delegate `AmbientAgentViewModel::selected_third_party_cli_agent` to it (keep the flag gate on the view-model method so its current call sites continue to behave the same):
  ```rust
  impl CLIAgent {
      pub fn from_harness(harness: Harness) -> Option<Self> {
          match harness {
              Harness::Oz => None,
              Harness::Claude => Some(CLIAgent::Claude),
              Harness::Gemini => Some(CLIAgent::Gemini),
          }
      }
  }
  ```
- `ConversationOrTask::status(app) -> ConversationStatus` (already exists at `app/src/ai/agent_conversations_model.rs:359`).
- `TerminalView::selected_conversation_status_for_display(ctx) -> Option<ConversationStatus>` (already exists at `app/src/terminal/view/pane_impl.rs:1031`).
### 3. `AmbientAgentViewModelEvent` re-emission through `TerminalViewStateChanged`
`app/src/terminal/view/ambient_agent/view_impl.rs::handle_ambient_agent_event`
The vertical tabs (and every other subscriber that renders from `terminal_view_agent_icon_variant`) re-render on `TerminalViewEvent::TerminalViewStateChanged`. Emit that event from the ambient-model event handlers for every state transition that can change the icon output: `EnteredSetupState`, `EnteredComposingState`, `DispatchedAgent`, `SessionReady`, `ProgressUpdated`, `Failed`, `Cancelled`, `NeedsGithubAuth`, `HarnessSelected`, `HarnessCommandStarted`. The event propagates pane-group → workspace → `ctx.notify()` via existing wiring in `app/src/pane_group/pane/terminal_pane.rs:720-722` and `app/src/workspace/view.rs:12920`.
### 4. `TerminalView::is_in_cloud_agent_setup_phase` + status override
`app/src/terminal/view/pane_impl.rs`
Add `is_in_cloud_agent_setup_phase(ctx) -> bool` that returns true while `ambient_agent_view_model.is_waiting_for_session()` OR `is_cloud_agent_pre_first_exchange` is true. Update `selected_conversation_status` (line 996) and `selected_conversation_status_for_display` (line 1031) to treat the setup phase as `ConversationStatus::InProgress`, matching how active long-running shell commands surface today. This is what makes the spinner show up inside the cloud lobe on every surface that consults the terminal view for status (vertical tabs, pane header). Remote-task surfaces (conversation list cards) already return InProgress during `AmbientAgentTaskState::{Queued, Pending, Claimed, InProgress}` via `ConversationOrTask::status`, so they inherit the same semantic for free.
### 5. `terminal_view_agent_icon_variant` helper
New free function in a new module `app/src/ui_components/agent_icon.rs`, re-exported via `app/src/ui_components/mod.rs`. Signature:
```rust
pub(crate) fn terminal_view_agent_icon_variant(
    terminal_view: &TerminalView,
    app: &AppContext,
) -> Option<IconWithStatusVariant>;
```
Encapsulates the waterfall currently split between `resolve_icon_with_status_variant` and `TypedPane::summary_pane_kind`:
1. Let `is_ambient = terminal_view.is_ambient_agent_session(app)`.
2. If `CLIAgentSessionsModel::as_ref(app).session(terminal_view.id())` is `Some(session)` with `listener.is_some()` and `agent != CLIAgent::Unknown`: return `Some(CLIAgent { agent: session.agent, status: if agent_supports_rich_status(&session.agent) { Some(session.status.to_conversation_status()) } else { None }, is_ambient })`.
3. Else if a non-plugin-backed session exists with a known agent: return `Some(CLIAgent { agent, status: None, is_ambient })`.
4. Else if the terminal is ambient with a selected third-party harness (via `ambient_agent_view_model.as_ref(app).selected_third_party_cli_agent()`): return `Some(CLIAgent { agent: harness_cli, status: terminal_view.selected_conversation_status_for_display(app), is_ambient: true })`. **This is the fix for the pre-setup bug.**
5. Else if the terminal has a selected conversation OR `is_ambient`: return `Some(OzAgent { status: terminal_view.selected_conversation_status_for_display(app), is_ambient })`.
6. Else: `None` (caller falls through to `Neutral { Terminal }` / `Neutral { Shell }` / error indicator).
### 6. `conversation_or_task_agent_icon_variant` helper
Same module as above. Signature:
```rust
pub(crate) fn conversation_or_task_agent_icon_variant(
    src: &ConversationOrTask<'_>,
    app: &AppContext,
) -> Option<IconWithStatusVariant>;
```
Rules:
- `ConversationOrTask::Task(task)`: ambient run. Read `task.agent_config_snapshot.as_ref().and_then(|s| s.harness.as_ref())`, parse to `Harness` via the same local parser used by `AmbientAgentViewModel::enter_viewing_existing_session` (the `parse_harness_config_name` helper at `app/src/terminal/view/ambient_agent/model.rs:981`, which should be lifted to `pub(crate)` in that module or moved to `crates/warp_cli` as `Harness::from_config_name`). Use `CLIAgent::from_harness` to map to `Option<CLIAgent>`; when `None` (Oz harness or no snapshot), emit `OzAgent { status: Some(src.status(app)), is_ambient: true }`; otherwise `CLIAgent { agent, status: Some(src.status(app)), is_ambient: true }`.
- `ConversationOrTask::Conversation(_)`: local Oz conversation. Emit `OzAgent { status: Some(src.status(app)), is_ambient: false }`. Local conversations have no `CLIAgent` signal on this surface (per product spec).
### 7. Wire vertical tabs through the helper
Replace the two inline derivations:
- `resolve_icon_with_status_variant` in `vertical_tabs.rs:2254-2299` for the Terminal branch becomes:
  ```rust
  TypedPane::Terminal(terminal_pane) => {
      let terminal_view = terminal_pane.terminal_view(app);
      let terminal_view = terminal_view.as_ref(app);
      if let Some(variant) = terminal_view_agent_icon_variant(terminal_view, app) {
          return variant;
      }
      IconWithStatusVariant::Neutral {
          icon: WarpIcon::Terminal,
          icon_color: main_text,
      }
  }
  ```
- `TypedPane::summary_pane_kind` in `vertical_tabs.rs:2488-2509` for the Terminal branch becomes a thin wrapper that maps the helper's variant into `SummaryPaneKind::OzAgent`/`CLIAgent` with matching `is_ambient`:
  ```rust
  if let Some(variant) = terminal_view_agent_icon_variant(terminal_view, app) {
      return match variant {
          IconWithStatusVariant::OzAgent { is_ambient, .. } => SummaryPaneKind::OzAgent { is_ambient },
          IconWithStatusVariant::CLIAgent { agent, is_ambient, .. } => SummaryPaneKind::CLIAgent { agent, is_ambient },
          _ => unreachable!("helper only returns agent variants"),
      };
  }
  // else: existing conversation/title fallback / Terminal
  ```
No other logic in these two functions changes — the centralization removes the duplicated waterfall without moving the call sites.
### 8. Wire pane header through the helper
Replace the agent-only indicator branches in `render_header_title` (`pane_impl.rs:268-369`):
- Today the branch chain is: shared ambient agent indicator → shared-session avatar / Sharing icon → conversation-selected `render_agent_indicator` → terminal-mode indicator.
- New: after the shared-session branch, if `terminal_view_agent_icon_variant(self, app)` returns `Some(variant)`, render via `render_icon_with_status(variant, &PANE_HEADER_AGENT_SIZING, theme, theme.background())` at 16px icon / 24px overall (matches `VERTICAL_TABS_AGENT_SIZING`). This replaces both `render_ambient_agent_indicator` and `render_agent_indicator`.
- Plain-terminal / shell / error indicators (`render_terminal_mode_indicator`) stay untouched.
Define `PANE_HEADER_AGENT_SIZING` in `pane_impl.rs` mirroring `VERTICAL_TABS_AGENT_SIZING` — we keep two constants rather than unifying them to minimize ripple, but the numeric values are identical.
Delete the now-unused `render_agent_indicator` and `render_ambient_agent_indicator` after the migration. Keep `mouse_states.ambient_agent_indicator_mouse_handle` only if the new surface still needs a tooltip hover — verify during implementation; likely we drop the tooltip and rely on the tab title instead.
### 9. Wire the inline conversation list menu through the helper
In `conversation_list/item.rs::render_item`, replace the two-branch icon derivation (`Icon::Cloud` for ambient rows, `render_status_element` for local rows) at the block around line 197:
```rust
let icon_element: Box<dyn Element> = if conversation.is_ambient_agent_conversation() {
    ConstrainedBox::new(Icon::Cloud.to_warpui_icon(theme.sub_text_color(theme.background())).finish())
        .with_width(status_element_size)
        .with_height(status_element_size)
        .finish()
} else {
    render_status_element(&conversation.status(app), font_size, appearance)
};
```
with:
```rust
let icon_element: Box<dyn Element> =
    match conversation_or_task_agent_icon_variant(conversation, app) {
        Some(variant) => render_icon_with_status(variant, &LIST_ITEM_AGENT_SIZING, theme, theme.background()),
        None => render_status_element(&conversation.status(app), font_size, appearance),
    };
```
`LIST_ITEM_AGENT_SIZING` is a new module-local `IconWithStatusSizing` whose overall overall size matches the existing `status_element_size = font_size + STATUS_ELEMENT_PADDING * 2.` footprint so row heights don't shift. Visual proportions (icon-inside-circle, cloud lobe, badge offsets) follow `VERTICAL_TABS_AGENT_SIZING` scaled down proportionally. The `None` branch keeps the surface future-proof; every current `ConversationOrTask` produces an agent variant today.
The Agent Management View (`AgentManagementView::render_header_row`) is explicitly out of scope in this pass — keep its existing `render_status_element` call untouched. See `PRODUCT.md` § "Inline conversation list menu" for rationale.
### 10. Thread `is_ambient` through `NotificationSourceAgent`
Expand the enum to carry the flag so downstream rendering can honor it:
```rust
// app/src/ai/agent_management/notifications/item.rs
pub enum NotificationSourceAgent {
    Oz { is_ambient: bool },
    CLI { agent: CLIAgent, is_ambient: bool },
}
```
All construction sites live in `app/src/ai/agent_management/agent_management_model.rs`:
- CLI sessions (`handle_cli_agent_session_event`, lines 172 and 189): for a CLI session firing a notification, the terminal view for `terminal_view_id` may be an ambient cloud-mode terminal running e.g. Claude. Resolve it by looking up the terminal view (reuse the `resolve_git_branch_for_terminal_view` lookup pattern at `agent_management_model.rs:505` and extract a `find_terminal_view_by_id(terminal_view_id, app) -> Option<ViewHandle<TerminalView>>` helper) and reading `terminal_view.as_ref(app).ambient_agent_view_model().as_ref(app).is_ambient_agent()`.
- History-model Oz notifications (`handle_history_event_for_mailbox`, lines 325, 338, 350, 363): same lookup applied to the notification's `terminal_view_id`.
Update `NotificationItem::new` and `AgentNotificationsModel::add_notification` signatures to keep the is_ambient flag on the `NotificationSourceAgent` rather than introducing a separate param — the flag is conceptually part of the source's identity.
Update `render_agent_avatar` in `item_rendering.rs:417-441` to read `is_ambient` from the enum variant rather than hardcoding `false`. When `is_ambient` is true, the existing `IconWithStatusVariant::{OzAgent, CLIAgent}` code path already produces the cloud-lobe rendering; no changes to `render_icon_with_status` itself.
`NOTIFICATION_AVATAR_SIZING` already has the `cloud_icon_size` / `cloud_offset` / `status_in_cloud_icon_size` fields filled in (lines 412-414); the values were set up speculatively and are reasonable as-is.
The telemetry event at `agent_management_model.rs:423` uses `TelemetryEvent::AgentNotificationShown { agent_variant: agent.into() }`. Update the `From<NotificationSourceAgent>` impl for the telemetry variant to drop the ambient flag or preserve it as a separate field — whichever matches existing telemetry schema. Defer to the existing schema; default to dropping the flag if it's not already in the event.
### 11. Telemetry / logs
No new telemetry events. Existing `AgentNotificationShown` keeps firing with the agent variant. If schema allows, extend it with `is_ambient: bool` to enable future cloud-vs-local notification analytics; otherwise defer.
### 12. Cross-surface equivalence tests
The suite's job is to lock the invariant *"same logical run → identical icon on every surface"* and catch drift when a new surface or state combo is added. Three essential pieces, all in a new `app/src/ui_components/agent_icon_tests.rs`.
#### 12.1 Canonical state fixture
A test-only enum `CanonicalRunState` enumerates every conceptually distinct run (plain terminal; local Oz conversation; local CLI agent; cloud Oz in setup/post-setup; cloud third-party pre-dispatch / waiting / harness-started / terminal state). It exposes:
```rust
impl CanonicalRunState {
    fn all() -> Vec<Self>;
    fn to_terminal_fixture(&self) -> Option<FakeTerminalSource>;
    fn to_task_fixture(&self) -> Option<FakeTaskOrConversationSource>;
    fn to_notification_fixture(&self) -> Option<FakeNotificationSource>;
    fn expected_variant(&self) -> Option<IconWithStatusVariant>; // single canonical answer
}
```
`expected_variant` is spec-in-code — editing it requires editing `PRODUCT.md`. The `Fake*Source` structs expose only the fields each helper reads; helpers accept a trait implemented by both the real source and the fake.
#### 12.2 Canonical-state equivalence table
The consistency enforcer — one parameterized test:
```rust
#[test]
fn every_canonical_state_produces_consistent_icon_across_surfaces() {
    for state in CanonicalRunState::all() {
        let expected = state.expected_variant();
        if let Some(t) = state.to_terminal_fixture() {
            assert_eq!(terminal_view_agent_icon_variant_fake(&t), expected, "terminal: {state:?}");
        }
        if let Some(t) = state.to_task_fixture() {
            assert_eq!(conversation_or_task_agent_icon_variant_fake(&t), expected, "task: {state:?}");
        }
        if let Some(n) = state.to_notification_fixture() {
            assert_eq!(notification_source_agent_variant_fake(&n), expected, "notification: {state:?}");
        }
    }
}
```
Fails loudly and names the offending state when surfaces drift. Adding a surface = one more `if let` branch; adding a state = one more enum variant + `expected_variant` arm. Bundle two structural invariants in the same loop: ambient states must produce `is_ambient: true`, local states must produce `is_ambient: false`.
#### 12.3 Event-driven re-render + integration tests
Using real models via the `workspace/view_test.rs` harness (not fakes):
- For each `AmbientAgentViewModelEvent` re-emitted in §3, fire it on a real `AmbientAgentViewModel` and assert the enclosing `TerminalView` emits `TerminalViewEvent::TerminalViewStateChanged`.
- `cloud_claude_setup_phase_renders_same_variant_on_every_surface`: build a live `TerminalView` (harness = Claude, `WaitingForSession`), a matching `AmbientAgentTask`, and a `NotificationSourceAgent` for the same terminal. Assert all three helpers return `CLIAgent { Claude, Some(InProgress), is_ambient: true }`.
- `local_claude_vs_cloud_claude_are_distinguishable`: a locally-registered Claude CLI session and an ambient Claude run must produce the same `CLIAgent { Claude, .. }` variant differing only in `is_ambient` — the one bit that answers the second product-spec ambiguity.
- `notification_is_ambient_flag_travels_end_to_end`: firing a `Blocked` event from an ambient Claude terminal produces `NotificationSourceAgent::CLI { Claude, is_ambient: true }`; the same from a local Claude terminal produces `is_ambient: false`.
`summary_pane_kind_icons_distinguish_ambient_claude_from_local_claude` in `vertical_tabs_tests.rs` stays as-is. Existing `notifications/item_tests.rs` tests are extended with `is_ambient: true` and `false` cases for each variant.
## End-to-end flow (Claude cloud run)
1. User selects Claude in harness selector. `AmbientAgentViewModel::set_harness(Harness::Claude, ctx)` fires `HarnessSelected`.
2. The `HarnessSelected` handler in `ambient_agent/view_impl.rs:262` emits `TerminalViewStateChanged` → pane group → workspace → `ctx.notify()`.
3. Vertical tabs re-render. `terminal_view_agent_icon_variant` walks its waterfall: no CLI session yet (step 1/2 skipped), `is_third_party_harness()` → true with `selected_third_party_cli_agent() → Some(CLIAgent::Claude)`, `selected_conversation_status_for_display` → `None` (nothing in progress yet since Composing), so variant is `CLIAgent { Claude, None, is_ambient: true }`. Tab shows Claude-orange circle, cloud lobe (no status icon inside).
4. User submits prompt. `DispatchedAgent` fires; view-model transitions to `WaitingForSession`; `is_in_cloud_agent_setup_phase` → true; `selected_conversation_status_for_display` → `Some(InProgress)`. Helper returns `CLIAgent { Claude, Some(InProgress), is_ambient: true }`. Tab shows Claude circle, cloud lobe with spinner inside.
5. Pane header renders the same helper output, same visual.
6. Conversation list card appears as soon as the `AmbientAgentTask` is registered. `conversation_or_task_agent_icon_variant` reads `agent_config_snapshot.harness` → "claude" → `Harness::Claude` → `CLIAgent::Claude`; `ConversationOrTask::status` → `InProgress`. Card shows same visual.
7. Session ready; harness block starts; `CLIAgentSessionsModel::set_session(CLIAgent::Claude)` fires. Helper's step 1 takes over (plugin-backed session). Same brand + status, so no visual flip.
8. Notification fires (e.g. blocked permission request). Emit path resolves `terminal_view.ambient_agent_view_model().is_ambient_agent() → true`; `NotificationSourceAgent::CLI { Claude, is_ambient: true }` goes into the notification. `render_agent_avatar` → `CLIAgent { Claude, Some(Blocked), is_ambient: true }`. Notification shows Claude cloud avatar.
9. Run completes. Status propagates from `AmbientAgentTask.state` → `ConversationOrTask::status` → Success; card icon updates. `AmbientAgentViewModel` transitions to terminal state; `HarnessCommandStarted` already fired earlier, so `is_cloud_agent_pre_first_exchange` is false and the tab/pane-header helpers pick up the real conversation status.
## Risks and mitigations
**`NotificationSourceAgent` schema change is a breaking API for existing call sites.** Every construction site is in `agent_management_model.rs` (8 call sites between CLI events and history events). We'll update them all in the same patch. The helper function `find_terminal_view_by_id` keeps the lookup cost low (already paid once per notification for branch resolution).
**Look-up races for is_ambient at notification emit time.** If the terminal view has been torn down before the notification fires (unlikely — notifications fire off in-flight events), the lookup returns `None` and we default `is_ambient: false`. This is safe; the notification still renders, just without the cloud lobe.
**Pane header sizing regression.** The current indicator uses `appearance.ui_font_size()` (~12-14px) as the icon size; the new circle is 24px overall. The header row height is fixed by other elements (title text at 13-14px), so a 24px circle is taller. We may need to adjust vertical alignment or cross-axis sizing. Validate during implementation; if the header grows, either reduce `PANE_HEADER_AGENT_SIZING` to match the font size or re-lay the header row to center the circle against the title.
**Removing `render_ambient_agent_indicator` drops its tooltip.** The indicator currently shows a "Cloud agent run" tooltip on hover (pane_impl.rs:881-892). If retaining the tooltip is important, wrap the new circle in the same `Hoverable` and tooltip. Otherwise drop; the cloud lobe makes the ambient nature visually obvious.
**Card icon replaces the plain status-only visual.** Cards that don't have an agent (none today, but defensively) fall through to the existing `render_status_element`. No visual regression expected.
**Helper takes a full `TerminalView` reference.** This creates a transitive dependency from `ui_components/agent_icon.rs` back to `terminal::view::TerminalView`. Acceptable because `ui_components` already has terminal-aware code (`icon_with_status.rs` imports `CLIAgent` from `terminal`). Alternative: invert the dependency by making the helper a method on `TerminalView` in `pane_impl.rs`. Pick based on where the compile dependencies look cleanest; prefer the free function in `ui_components/agent_icon.rs` for testability.
## Testing and validation
Unit coverage:
- Add `CLIAgent::from_harness` tests for Oz/Claude/Gemini inputs.
- Extend `vertical_tabs_tests.rs` (or add `ui_components/agent_icon_tests.rs`) with the cross-surface equivalence suite described in §12.
- Test `conversation_or_task_agent_icon_variant` with:
  - A `ConversationOrTask::Task` whose `agent_config_snapshot.harness` is `"claude"` → `CLIAgent::Claude` + `is_ambient: true`.
  - A `ConversationOrTask::Task` whose harness is `"oz"` or missing → `OzAgent` + `is_ambient: true`.
  - A `ConversationOrTask::Conversation` → `OzAgent` + `is_ambient: false`.
- Test that `terminal_view_agent_icon_variant` returns `CLIAgent { Claude, ... is_ambient: true }` when the view-model's selected harness is Claude and no CLIAgent session exists yet.
- Test idempotency of `NotificationSourceAgent` schema: old tests in `notifications/item_tests.rs` need updates for the new `is_ambient` field; add one new test case asserting the cloud-lobe path is taken when `is_ambient: true`.
Integration / manual validation:
- Full flows from the product spec's Validation section. Concretely: spawn a Claude cloud run; observe tab + pane header + card all render the same Claude circle + cloud lobe + status through Composing → Waiting → Running → Success.
- Start a local `claude` CLI session and confirm tab + pane header render the Claude circle with a bottom-right badge (no cloud lobe).
- Trigger a blocking permission request on a Claude cloud run; observe the notification in the mailbox shows the Claude cloud avatar.
- Re-run the REMOTE-1454 validation cases to confirm no regression in the cloud setup UX.
Invoke `verify-ui-change-in-cloud` after the implementation lands to spot-check the visual result across surfaces.
## Follow-ups
- Consider collapsing `NotificationSourceAgent` + `is_ambient` + status into a single `AgentIconDescriptor` struct shared by all four surfaces once a third or fourth call site appears (Option 3 in the design discussion). Not worth it at two call sites.
- If the helper function pattern proves brittle at the trait-boundary for test fakes, consider introducing an `AgentIconSource` trait and formalizing the descriptor in a follow-up PR.
- Move `parse_harness_config_name` out of `ambient_agent/model.rs` into `crates/warp_cli/src/agent.rs` as `impl Harness { pub fn from_config_name(&str) -> Harness }`. Currently private to the ambient module; needed by the new task-side helper.
- Telemetry: extend `AgentNotificationShown` with `is_ambient` once the schema is updated, to track cloud-notification volume.
