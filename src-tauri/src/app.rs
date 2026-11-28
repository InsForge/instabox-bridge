//! The app's state and the logic around it.
//!
//! Plain functions decide what the UI shows (org filtering, plan, sizes, sleep choices,
//! which sandboxes to sync). `Core` holds the state, talks to the API and the other
//! modules, and reports every change through one emit callback. The Tauri glue stays in
//! `lib.rs`, `commands.rs` and `tray.rs`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::Serialize;
use tokio::sync::watch;
use url::Url;

use crate::ade::{self, Ade, Open, SshLogin};
use crate::agents::{self, Agent};
use crate::api::{Client, Credentials, Machine, MachineSpec, Me, Org, Snapshot, Template};
use crate::auth::{self, Provider, Session};
use crate::clipboard::SecretClipboard;
use crate::connected::ConnectedStore;
use crate::errors::ErrorView;
use crate::gate::{Gate, guarded};
use crate::mcp::{self, KeyStore, McpClient};
use crate::old_app::OldAppView;
use crate::sandbox::connect::{Connected, NewSandbox, Source, Step};
use crate::sandbox::sync::{Decision, SyncOutcome};
use crate::sandbox::view::{
    self, SandboxView, SizeChoice, SleepChoice, ViewState, is_linux_template,
};
use crate::sandbox::{self, SandboxError, connect, lifecycle};
use crate::ssh::{self, Kind, SshTokenStore};
use crate::ssh_config;
use crate::t3proxy;
use crate::updater::UpdateView;

const DEFAULT_API: &str = "https://api.instabox.sh";
/// A sandbox with an InstaADE SSH host (Orca, Paseo) gets a new background token once the
/// stored one has fewer days left than this, so the host keeps working without a click.
const HOST_TOKEN_RENEW_DAYS: i64 = 7;
const VISIBLE_REFRESH: Duration = Duration::from_secs(5);
const HIDDEN_REFRESH: Duration = Duration::from_secs(30);
/// Account data and the desktop ADE versions are read this often.
pub const FULL_REFRESH: Duration = Duration::from_secs(300);
/// The longest a quit waits to clear a copied secret.
const QUIT_WAIT: Duration = Duration::from_secs(1);
/// The longest the probe install an Open starts may run on the sandbox (`Core::refresh_probe`).
const PROBE_TIMEOUT_SECS: u32 = 60;

/// Called with the whole state after every change.
pub type Emitter = Arc<dyn Fn(&AppState) + Send + Sync>;
/// Opens a URL. With an app (the path of an app bundle), in that app only; without one, in the
/// default handler (browser, or an ADE's app for its links, such as T3 Code for `t3code://`).
pub type Opener = Arc<dyn Fn(&str, Option<&Path>) -> Result<(), String> + Send + Sync>;
/// Opens a login in the user's terminal: the ssh client to run and its arguments
/// (`ssh::open_in_terminal`).
pub type TerminalOpener = Arc<dyn Fn(&Path, &[String]) -> Result<(), String> + Send + Sync>;
/// Finds an ADE's desktop app on this computer (`Ade::locate`).
pub type Locator = Arc<dyn Fn(Ade) -> Option<PathBuf> + Send + Sync>;
/// Checks that the app at a path is the ADE's own (`Ade::verify`).
pub type Verifier = Arc<dyn Fn(Ade, &Path) -> bool + Send + Sync>;
/// Test only: called with the name of a step a flow reached.
#[cfg(test)]
type Hook = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgView {
    pub id: String,
    pub name: String,
    pub tier: String,
    pub personal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotView {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStatus {
    /// `None`: the `claude` CLI is not installed.
    pub claude_code: Option<bool>,
    pub codex: Option<bool>,
}

/// Where a create or connect is. `kind` is `create` (a new sandbox) or `connect` (one that
/// exists). `step` is one of `connecting`, `creating`, `waking`, `installing`, `pairing`,
/// `done`. `machine_id` is set once the sandbox exists, so the UI's Retry can call
/// `connect_sandbox` with it and `ade`. `ade` is the ADE being set up; None for a create
/// without one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressView {
    pub kind: String,
    pub step: String,
    pub machine_id: Option<String>,
    pub ade: Option<Ade>,
    pub error: Option<ErrorView>,
}

/// An ADE whose desktop app runs on this computer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdeView {
    pub id: Ade,
    pub name: &'static str,
    pub short_name: &'static str,
    /// The desktop app's version; None when it is not found on this computer.
    pub version: Option<String>,
    pub download_url: &'static str,
}

impl AdeView {
    fn new(ade: Ade, version: Option<String>) -> AdeView {
        AdeView {
            id: ade,
            name: ade.name(),
            short_name: ade.short_name(),
            version,
            download_url: ade.download_url(),
        }
    }
}

/// A sandbox T3 Code saved with its published port, which it should reach through the T3 proxy
/// instead: T3 Code adds the proxy as a route of the environment it saved, then removes the old
/// route. The pairing URL to paste is never here: `copy_t3_switch_url` copies a new one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct T3SwitchView {
    pub id: String,
    pub name: String,
    /// The route T3 Code saved: the sandbox's published T3 port.
    pub old_address: String,
    /// The route T3 Code adds: the proxy, `http://localhost:<port>`.
    pub new_address: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    pub signed_in: bool,
    pub login: Option<String>,
    /// The code to match on the sign-in page, while a sign-in waits.
    pub sign_in_code: Option<String>,
    pub orgs: Vec<OrgView>,
    pub org_id: Option<String>,
    pub tier: String,
    /// The desktop T3 Code's version, for the UI. The same as T3's entry in `ades`.
    pub t3: Option<String>,
    /// The ADEs that run on this computer, with the versions found here.
    pub ades: Vec<AdeView>,
    pub sandboxes: Vec<SandboxView>,
    pub sizes: Vec<SizeChoice>,
    pub sleep_choices: Vec<SleepChoice>,
    pub snapshots: Vec<SnapshotView>,
    pub mcp: McpStatus,
    pub progress: Option<ProgressView>,
    /// The old app (Instabox.app) left next to this one, offered to move to the Trash.
    pub old_app: Option<OldAppView>,
    /// The guide that moves a sandbox T3 Code saved with its published port to the T3 proxy
    /// (prototype, `INSTAADE_T3_PROXY`), while it shows.
    pub t3_switch: Option<T3SwitchView>,
    pub error: Option<ErrorView>,
    pub start_at_login: bool,
    /// This build's version and its updates (see `updater`). Set once the app starts.
    pub update: Option<UpdateView>,
}

/// What the last refresh read from the API and this computer.
#[derive(Debug, Clone, Default)]
pub struct Data {
    pub me: Option<Me>,
    pub orgs: Vec<Org>,
    pub templates: Vec<Template>,
    pub machines: Vec<Machine>,
    pub snapshots: Vec<Snapshot>,
    /// The ADE version this app last saw on each sandbox.
    pub ade_versions: HashMap<String, String>,
    /// Sandboxes whose ADE is behind the desktop's and could not be updated (busy or failed).
    pub update_pending: HashSet<String>,
    /// Sandboxes whose ADE is newer than the desktop's: the app on this computer is behind.
    pub desktop_behind: HashSet<String>,
    /// Sandboxes the signed-in user paired a pairing ADE (T3) with from this computer. Only
    /// these get commands without a user action. A link ADE (cmux) never adds one.
    pub connected: HashSet<String>,
    /// The user `connected` was loaded for. `None` until the account is known.
    pub connected_user: Option<i64>,
    /// The hash of the probe an Open installed or found on each sandbox, per ADE
    /// (`Ade::probe_on_open`). Kept while the app runs, for the signed-in account.
    pub open_probes: HashMap<(String, Ade), String>,
}

impl Data {
    /// The sandbox runs `version` now.
    pub fn set_version(&mut self, id: &str, version: &str) {
        self.ade_versions
            .insert(id.to_string(), version.to_string());
        self.update_pending.remove(id);
        self.desktop_behind.remove(id);
    }

    /// The ADEs sandbox `id` has: the ones its mark lists that this build knows. A sandbox
    /// with no mark (a plain one) has none. This decides Open against Install.
    pub fn ades_of(&self, id: &str) -> Vec<Ade> {
        self.machines
            .iter()
            .find(|m| m.id == id)
            .map(|m| view::marked_ades(&m.metadata))
            .unwrap_or_default()
    }

    /// The ADE an action on sandbox `id` uses when none was asked for (see `action_ades`);
    /// the default for a sandbox not listed. None when its mark names only ADEs this build
    /// does not know.
    pub fn ade_of(&self, id: &str) -> Option<Ade> {
        self.machines
            .iter()
            .find(|m| m.id == id)
            .map_or_else(|| vec![Ade::default()], action_ades)
            .first()
            .copied()
    }

    /// The ADE a resume of sandbox `id` keeps in step with this computer (T3), if any (see
    /// `action_ades`).
    pub fn pinned_ade_of(&self, id: &str) -> Option<Ade> {
        self.machines
            .iter()
            .find(|m| m.id == id)
            .map_or_else(|| vec![Ade::default()], action_ades)
            .into_iter()
            .find(|a| a.pins_local_version())
    }

    /// The user whose connected set applies: the one it was loaded for, else the account's.
    pub fn user(&self) -> Option<i64> {
        self.connected_user
            .or_else(|| self.me.as_ref().map(|m| m.user_id))
    }

    /// Takes the connected set read from disk for `user`. A set already loaded for another
    /// user is replaced; ids remembered before the first load are kept.
    pub fn use_connected(&mut self, user: i64, set: HashSet<String>) {
        if self.connected_user == Some(user) {
            return;
        }
        if self.connected_user.is_some() {
            self.connected.clear();
        }
        self.connected.extend(set);
        self.connected_user = Some(user);
    }

    /// Records a sync. `None` is a sync that failed. No retry follows: each one is an exec.
    pub fn record_sync(&mut self, id: &str, desktop: &str, outcome: Option<&SyncOutcome>) {
        let Some(o) = outcome else {
            self.update_pending.insert(id.to_string());
            return;
        };
        match o.decision {
            Decision::Update => self.set_version(id, desktop),
            Decision::UpToDate => {
                let v = o.remote_version.as_deref().unwrap_or(desktop).to_string();
                self.set_version(id, &v);
            }
            Decision::Defer => {
                if let Some(v) = &o.remote_version {
                    self.ade_versions.insert(id.to_string(), v.clone());
                }
                self.desktop_behind.remove(id);
                self.update_pending.insert(id.to_string());
            }
            // The sandbox is ahead: never move it back, and do not offer [Update].
            Decision::Older => {
                if let Some(v) = &o.remote_version {
                    self.ade_versions.insert(id.to_string(), v.clone());
                }
                self.update_pending.remove(id);
                self.desktop_behind.insert(id.to_string());
            }
        }
    }
}

// ---- pure logic -----------------------------------------------------------------------

/// The API base: `INSTABOX_API_URL` when it is a valid URL, else production.
pub fn api_base(env: Option<String>) -> Url {
    env.and_then(|v| Url::parse(v.trim()).ok())
        .unwrap_or_else(|| Url::parse(DEFAULT_API).expect("default API URL is valid"))
}

/// How long to wait between machine refreshes.
pub fn refresh_interval(window_visible: bool) -> Duration {
    if window_visible {
        VISIBLE_REFRESH
    } else {
        HIDDEN_REFRESH
    }
}

/// The orgs to show: `/v1/orgs`, or the one org in `/v1/me` when that list is empty.
pub fn effective_orgs(me: &Me, orgs: &[Org]) -> Vec<Org> {
    if !orgs.is_empty() {
        return orgs.to_vec();
    }
    me.org
        .iter()
        .map(|o| Org {
            id: o.id.clone(),
            name: o.name.clone().unwrap_or_default(),
            tier: o.tier.clone(),
            kind: o.kind.clone(),
            personal: o.kind == "personal",
            allowed_specs: Vec::new(),
        })
        .collect()
}

/// Keeps `current` while it is still an org of the user; else the personal org from `me`.
pub fn resolve_org_id(current: Option<&str>, me: &Me, orgs: &[Org]) -> Option<String> {
    if let Some(c) = current
        && orgs.iter().any(|o| o.id == c)
    {
        return Some(c.to_string());
    }
    me.org
        .as_ref()
        .map(|o| o.id.clone())
        .or_else(|| orgs.first().map(|o| o.id.clone()))
}

/// Sizes the selected org allows: its own list, else the user's.
pub fn sizes_for(orgs: &[Org], org_id: Option<&str>, me: &Me) -> Vec<SizeChoice> {
    let own = orgs
        .iter()
        .find(|o| Some(o.id.as_str()) == org_id)
        .map(|o| o.allowed_specs.as_slice())
        .filter(|s| !s.is_empty());
    view::size_choices(own.unwrap_or(&me.allowed_specs))
}

/// The ADEs an action may use on a machine when none was asked for: the ones its mark lists,
/// in the order they were added, else the default when it has no mark at all. Empty when the
/// mark names only ADEs this build does not know, such as one a newer app connected: such a
/// sandbox never counts as the default. Only for picking an ADE to act with (connect, pair
/// again, reinstall, resume, sync). What a sandbox has is its mark alone (`Data::ades_of`).
fn action_ades(m: &Machine) -> Vec<Ade> {
    if view::has_mark(&m.metadata) {
        view::marked_ades(&m.metadata)
    } else {
        vec![Ade::default()]
    }
}

fn template_is_linux(templates: &[Template], id: &str) -> bool {
    templates
        .iter()
        .find(|t| t.id == id)
        .is_none_or(is_linux_template)
}

/// Machines of the selected org on a Linux template. An unknown template shows.
pub fn visible_machines<'a>(
    machines: &'a [Machine],
    templates: &[Template],
    org_id: Option<&str>,
) -> Vec<&'a Machine> {
    machines
        .iter()
        .filter(|m| org_id.is_none_or(|o| m.org_id == o))
        .filter(|m| template_is_linux(templates, &m.template.id))
        .collect()
}

/// The sandbox rows. `T3 Code connected` needs a connected, running sandbox and a
/// running desktop app.
pub fn sandbox_views(d: &Data, org_id: Option<&str>, now: DateTime<Utc>) -> Vec<SandboxView> {
    visible_machines(&d.machines, &d.templates, org_id)
        .into_iter()
        .map(|m| {
            let mut v = view::view(m, now);
            v.t3_version = d.ade_versions.get(&m.id).cloned();
            v.update_pending = d.update_pending.contains(&m.id);
            v.desktop_behind = d.desktop_behind.contains(&m.id);
            v.connected = d.connected.contains(&m.id);
            v
        })
        .collect()
}

/// Ready snapshots of a Linux template in the selected org. Unknown template or org shows.
pub fn snapshot_views(
    snapshots: &[Snapshot],
    templates: &[Template],
    org_id: Option<&str>,
) -> Vec<SnapshotView> {
    snapshots
        .iter()
        .filter(|s| s.status == "ready")
        .filter(|s| template_is_linux(templates, &s.template.id))
        .filter(|s| match (&s.org_id, org_id) {
            (Some(own), Some(sel)) => own == sel,
            _ => true,
        })
        .map(|s| SnapshotView {
            id: s.id.clone(),
            name: s.name.clone(),
        })
        .collect()
}

/// True when a desktop ADE version moved from one known version to another.
/// The first read is not a change.
pub fn version_changed(prev: Option<&str>, new: Option<&str>) -> bool {
    matches!((prev, new), (Some(p), Some(n)) if p != n)
}

/// The sandboxes to sync after a change of `ade`'s desktop version: running, connected from
/// this computer and of that ADE (among others or alone, see `action_ades`), in any of the
/// user's orgs. A sandbox someone else connected is left alone.
pub fn sync_targets(d: &Data, ade: Ade) -> Vec<String> {
    d.machines
        .iter()
        .filter(|m| view::view_state(&m.state) == ViewState::Running)
        .filter(|m| d.connected.contains(&m.id))
        .filter(|m| template_is_linux(&d.templates, &m.template.id))
        .filter(|m| action_ades(m).contains(&ade))
        .map(|m| m.id.clone())
        .collect()
}

/// The MCP status shown in settings.
pub fn mcp_status(
    claude_installed: bool,
    claude_configured: bool,
    codex_configured: bool,
) -> McpStatus {
    McpStatus {
        claude_code: claude_installed.then_some(claude_configured),
        codex: Some(codex_configured),
    }
}

/// The progress after `step`. The kind and, once known, the machine id stay.
pub fn progress_after(prev: &ProgressView, step: &Step) -> ProgressView {
    let machine_id = prev.machine_id.clone();
    let (name, machine_id) = match step {
        Step::Creating => ("creating", machine_id),
        Step::Created(id) => ("creating", Some(id.clone())),
        Step::Waking => ("waking", machine_id),
        Step::Installing(_) => ("installing", machine_id),
        Step::Pairing => ("pairing", machine_id),
        Step::Done => ("done", machine_id),
    };
    ProgressView {
        kind: prev.kind.clone(),
        step: name.to_string(),
        machine_id,
        ade: prev.ade,
        error: None,
    }
}

/// A failed progress ends once its sandbox is gone (deleted here or elsewhere), so its Retry
/// can never point at a sandbox that no longer exists. A running one is left to its flow.
pub fn prune_progress(p: Option<ProgressView>, machines: &[Machine]) -> Option<ProgressView> {
    let listed = |id: &str| {
        machines
            .iter()
            .any(|m| m.id == id && !matches!(m.state.as_str(), "deleting" | "deleted"))
    };
    p.filter(|p| match (&p.error, &p.machine_id) {
        (Some(_), Some(id)) => listed(id),
        _ => true,
    })
}

/// A failed connect. With a machine id the progress stays, with the error, so Retry can
/// connect that sandbox. Without one there is nothing to retry and the progress ends.
pub fn fail_progress(prev: Option<&ProgressView>, error: &ErrorView) -> Option<ProgressView> {
    prev.filter(|p| p.machine_id.is_some())
        .map(|p| ProgressView {
            error: Some(error.clone()),
            ..p.clone()
        })
}

/// URLs the UI may ask the app to open in the default handler: https, and the links of every
/// pairing ADE's app. A link ADE's link (cmux) holds an SSH token, so only `open_link` opens
/// one, and only in the app it found. A host ADE's links (Paradev) open only in its checked
/// app too (`open_host`).
pub fn allowed_url(url: &str) -> bool {
    Url::parse(url).is_ok_and(|u| {
        u.scheme() == "https"
            || ade::ALL
                .iter()
                .any(|a| matches!(a.open(), Open::Pair(_)) && a.url_scheme() == u.scheme())
    })
}

pub fn parse_provider(s: &str) -> Option<Provider> {
    match s {
        "github" => Some(Provider::GitHub),
        "google" => Some(Provider::Google),
        _ => None,
    }
}

pub fn parse_mcp_client(s: &str) -> Option<McpClient> {
    match s {
        "claudeCode" => Some(McpClient::ClaudeCode),
        "codex" => Some(McpClient::Codex),
        _ => None,
    }
}

impl AppState {
    /// Fills the account and sandbox fields from `d`. Keeps the selected org while it is valid.
    pub fn apply(&mut self, d: &Data, now: DateTime<Utc>) {
        let Some(me) = &d.me else { return };
        let orgs = effective_orgs(me, &d.orgs);
        let org_id = resolve_org_id(self.org_id.as_deref(), me, &orgs);
        let tier = orgs
            .iter()
            .find(|o| Some(&o.id) == org_id.as_ref())
            .map(|o| o.tier.clone())
            .unwrap_or_default();
        self.login = Some(me.login.clone());
        self.orgs = orgs
            .iter()
            .map(|o| OrgView {
                id: o.id.clone(),
                name: o.name.clone(),
                tier: o.tier.clone(),
                personal: o.personal,
            })
            .collect();
        self.sizes = sizes_for(&orgs, org_id.as_deref(), me);
        self.sleep_choices = view::sleep_choices(me, tier == "free");
        self.sandboxes = sandbox_views(d, org_id.as_deref(), now);
        self.snapshots = snapshot_views(&d.snapshots, &d.templates, org_id.as_deref());
        self.tier = tier;
        self.org_id = org_id;
    }

    /// The version of `ade`'s desktop app found on this computer.
    pub fn desktop(&self, ade: Ade) -> Option<&str> {
        self.ades
            .iter()
            .find(|a| a.id == ade)
            .and_then(|a| a.version.as_deref())
    }

    /// Records the version of `ade`'s desktop app found on this computer; None: not found.
    pub fn set_desktop(&mut self, ade: Ade, version: Option<String>) {
        if ade == Ade::T3 {
            self.t3 = version.clone();
        }
        match self.ades.iter_mut().find(|a| a.id == ade) {
            Some(a) => a.version = version,
            None => self.ades.push(AdeView::new(ade, version)),
        }
    }

    /// The state after sign-out: what belongs to the account is gone.
    pub fn signed_out(&self) -> AppState {
        AppState {
            t3: self.t3.clone(),
            ades: self.ades.clone(),
            old_app: self.old_app.clone(),
            start_at_login: self.start_at_login,
            update: self.update.clone(),
            ..AppState::default()
        }
    }
}

// ---- Core -----------------------------------------------------------------------------

struct Inner {
    state: AppState,
    data: Data,
    session: Option<Arc<Session>>,
    client: Option<Client>,
    /// Counts account changes (sign-out, a new session, a session the server ended). Only
    /// `Core::transition` moves it, under the account lock. A flow that started under an
    /// older count must not act or write.
    account: u64,
}

impl Inner {
    fn rebuild(&mut self) {
        self.state.apply(&self.data, Utc::now());
    }

    /// Records the desktop version of `ade` just read. For an ADE that pins the desktop
    /// version, a move from one known version to another gives the sandboxes to bring along.
    fn set_desktop_version(&mut self, ade: Ade, version: Option<String>) -> Option<SyncJob> {
        let changed = version_changed(self.state.desktop(ade), version.as_deref());
        let job = (changed && ade.pins_local_version())
            .then(|| version.clone())
            .flatten()
            .map(|v| SyncJob {
                ade,
                version: v,
                ids: sync_targets(&self.data, ade),
                account: self.account,
            });
        self.state.set_desktop(ade, version);
        job
    }

    /// `set_desktop_version` for each ADE read.
    fn set_desktop_versions(&mut self, found: Vec<(Ade, Option<String>)>) -> Vec<SyncJob> {
        found
            .into_iter()
            .filter_map(|(ade, v)| self.set_desktop_version(ade, v))
            .collect()
    }

    /// The desktop version to bring sandbox `ade` in step with: known, and only for an ADE
    /// that pins it.
    fn sync_version(&self, ade: Ade) -> Option<String> {
        ade.pins_local_version()
            .then(|| self.state.desktop(ade).map(str::to_string))
            .flatten()
    }

    /// Ends the progress, and the copy of its error in `state.error`.
    fn clear_progress(&mut self) {
        if let Some(p) = self.state.progress.take()
            && p.error.is_some()
            && p.error == self.state.error
        {
            self.state.error = None;
        }
    }

    /// Shows a new sign-in code. The error of an earlier attempt, such as an expired code,
    /// no longer applies.
    fn show_sign_in_code(&mut self, code: String) {
        self.state.sign_in_code = Some(code);
        self.state.error = None;
    }

    /// Shows a failed save of the current session's rotated tokens. A session the server
    /// ended is an account change, which `Core::end_session` makes.
    fn session_flags(&mut self) {
        if let Some(e) = self.session.as_ref().and_then(|s| s.store_error()) {
            self.state.error = Some(ErrorView::other(format!(
                "Could not save your sign-in to the keychain: {e}"
            )));
        }
    }
}

/// A desktop version change: the version of `ade` to bring these sandboxes to.
pub struct SyncJob {
    pub ade: Ade,
    pub version: String,
    pub ids: Vec<String>,
    /// The account count when the job was made; a job for an older account is dropped.
    account: u64,
}

pub struct CreateArgs {
    pub name: String,
    pub snapshot_id: Option<String>,
    pub vcpus: u32,
    pub mem_mib: u32,
    pub auto_suspend_after_seconds: Option<i64>,
    /// The ADE to set up; None: a plain sandbox, with no ADE.
    pub ade: Option<Ade>,
}

/// What an account change puts in place of the current account.
enum Next {
    /// No account: a sign-out, or a session the server ended.
    Nobody,
    /// The session the last run saved. It never replaces another session.
    Saved(Session),
    /// The session of a device sign-in; its tokens are saved as part of the change.
    SignedIn(Session),
}

/// An account change that happened.
struct Changed {
    /// The account count after the change.
    account: u64,
    /// The new session is in place.
    signed_in: bool,
    /// The first store failure; the error line shows it too.
    error: Option<String>,
}

pub struct Core {
    base: Url,
    keys: Arc<dyn KeyStore>,
    ssh: Arc<dyn SshTokenStore>,
    connected: Arc<dyn ConnectedStore>,
    open: Opener,
    /// Opens a login in the user's terminal, for Open in Terminal and a terminal ADE (Herdr);
    /// a test puts its own in place.
    terminal: TerminalOpener,
    /// `Ade::locate` and `Ade::verify`; a test puts its own in place.
    locate: Locator,
    verify: Verifier,
    /// The clipboard for secrets, which clears them a minute later; a test puts its own in
    /// place.
    clipboard: Arc<SecretClipboard>,
    emit: Emitter,
    home: PathBuf,
    inner: Mutex<Inner>,
    /// The account lock (see `gate` for the lock order). Every account change holds it from
    /// start to end; every gated side effect holds it while it checks the account and acts.
    side_effects: Mutex<()>,
    /// Test only: called at named steps of a flow, so a test can change the account there.
    #[cfg(test)]
    hook: Mutex<Option<Hook>>,
    /// A pass of `renew_ssh_hosts` is running.
    renewing: std::sync::atomic::AtomicBool,
    /// The log said, this run, that the server made an ordinary token in place of a background
    /// one (`note_ordinary`). It says so once.
    ordinary_logged: std::sync::atomic::AtomicBool,
    /// The sandboxes and ADEs whose probe an Open installs now (`refresh_probe`).
    probing: Mutex<HashSet<(String, Ade)>>,
    /// The T3 local proxy (prototype), with the `INSTAADE_T3_PROXY` it was started with. Set at
    /// startup only when that is on.
    t3_proxy: OnceLock<(Arc<t3proxy::Hub>, t3proxy::Gate)>,
    /// Test only: the probe installs Opens started, so a test can wait for them.
    #[cfg(test)]
    probe_tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    /// Test only: every line `log` wrote.
    #[cfg(test)]
    logs: Mutex<Vec<String>>,
}

/// A flow's gate: open only while the account it began under is current.
struct FlowGate {
    core: Arc<Core>,
    account: u64,
}

impl Gate for FlowGate {
    fn run(&self, f: &mut dyn FnMut()) -> bool {
        let _account_lock = self
            .core
            .side_effects
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if self.core.lock().account != self.account {
            return false;
        }
        f();
        true
    }
}

async fn detect(ade: Ade) -> Option<String> {
    tokio::task::spawn_blocking(move || ade.detect())
        .await
        .ok()
        .flatten()
}

/// The desktop version of each ADE that runs on this computer; None: not found.
async fn detect_local() -> Vec<(Ade, Option<String>)> {
    detect_each(ade::local(), Ade::detect).await
}

/// Runs `detect` for every ADE at once, off the async runtime: each may block for seconds,
/// so the wait is the slowest one, not their sum. The results keep the order of `ades`.
async fn detect_each(
    ades: Vec<Ade>,
    detect: fn(Ade) -> Option<String>,
) -> Vec<(Ade, Option<String>)> {
    let running: Vec<_> = ades
        .into_iter()
        .map(|ade| (ade, tokio::task::spawn_blocking(move || detect(ade))))
        .collect();
    let mut found = Vec::with_capacity(running.len());
    for (ade, task) in running {
        found.push((ade, task.await.ok().flatten()));
    }
    found
}

fn anyhow_view(e: &anyhow::Error) -> ErrorView {
    ErrorView::other(format!("{e:#}"))
}

fn account_changed() -> ErrorView {
    ErrorView::other("The account changed. Nothing was done.")
}

fn unknown_ade() -> ErrorView {
    ErrorView::other(
        "A newer version of InstaADE connected this sandbox to another ADE. Update InstaADE to use it.",
    )
}

/// A host ADE (Paradev) opens through `open_host`, never by a link.
fn host_not_link(ade: Ade) -> ErrorView {
    ErrorView::other(format!("{} runs sandboxes as hosts", ade.name()))
}

fn terminal_not_link(ade: Ade) -> ErrorView {
    ErrorView::other(format!("{} opens in a terminal", ade.name()))
}

fn not_signed_in() -> ErrorView {
    ErrorView {
        kind: crate::errors::ErrorKind::SignedOut,
        message: "You are not signed in".to_string(),
        minutes: None,
        code: None,
    }
}

type Res<T> = Result<T, ErrorView>;

/// What the user does next in an ADE's app after an action opened it, when opening it does not
/// finish the job (Orca: connect the host InstaADE wrote; Paseo: add it once). None for every
/// other ADE.
pub type Notice = Option<String>;

/// The ids of every sandbox in an account's whole machine list.
struct Listed {
    account: u64,
    ids: HashSet<String>,
}

/// SSH hosts whose sandbox is not in the whole machine list of `account`.
struct Gone {
    account: u64,
    ids: Vec<String>,
}

impl Core {
    pub fn new(
        base: Url,
        keys: Arc<dyn KeyStore>,
        ssh: Arc<dyn SshTokenStore>,
        connected: Arc<dyn ConnectedStore>,
        open: Opener,
        emit: Emitter,
        home: PathBuf,
    ) -> Core {
        // The connected set is per user: it loads once the account is known (refresh).
        let mut state = AppState::default();
        for ade in ade::local() {
            state.set_desktop(ade, None);
        }
        Core {
            base,
            keys,
            ssh,
            connected,
            open,
            terminal: Arc::new(|ssh: &Path, args: &[String]| ssh::open_in_terminal(ssh, args)),
            locate: Arc::new(Ade::locate),
            verify: Arc::new(|ade: Ade, app: &Path| ade.verify(app)),
            clipboard: Arc::new(SecretClipboard::system()),
            emit,
            home,
            side_effects: Mutex::new(()),
            #[cfg(test)]
            hook: Mutex::new(None),
            renewing: std::sync::atomic::AtomicBool::new(false),
            ordinary_logged: std::sync::atomic::AtomicBool::new(false),
            probing: Mutex::new(HashSet::new()),
            t3_proxy: OnceLock::new(),
            #[cfg(test)]
            probe_tasks: Mutex::new(Vec::new()),
            #[cfg(test)]
            logs: Mutex::new(Vec::new()),
            inner: Mutex::new(Inner {
                state,
                data: Data::default(),
                session: None,
                client: None,
                account: 0,
            }),
        }
    }

    /// Test only: finds the ADEs' apps with `locate` and checks them with `verify` instead of
    /// on this computer.
    #[cfg(test)]
    fn with_apps(mut self, locate: Locator, verify: Verifier) -> Core {
        self.locate = locate;
        self.verify = verify;
        self
    }

    /// Test only: opens logins with `terminal` instead of in a terminal.
    #[cfg(test)]
    fn with_terminal(mut self, terminal: TerminalOpener) -> Core {
        self.terminal = terminal;
        self
    }

    /// Test only: copies secrets to `clipboard` instead of this computer's clipboard.
    #[cfg(test)]
    fn with_clipboard(mut self, clipboard: SecretClipboard) -> Core {
        self.clipboard = Arc::new(clipboard);
        self
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Changes the state, then emits it. The lock is not held while emitting. For changes
    /// that belong to no account flow: desktop facts, settings, a user action on the screen.
    fn update<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        let (r, state) = {
            let mut g = self.lock();
            let r = f(&mut g);
            (r, g.state.clone())
        };
        (self.emit)(&state);
        r
    }

    /// Changes the state for a flow that began under `account`: `f` runs only while that
    /// account is current, and the check and the change are one step. None when it changed.
    fn update_in<R>(&self, account: u64, f: impl FnOnce(&mut Inner) -> R) -> Option<R> {
        let (r, state) = {
            let mut g = self.lock();
            if g.account != account {
                return None;
            }
            let r = f(&mut g);
            (r, g.state.clone())
        };
        (self.emit)(&state);
        Some(r)
    }

    /// Reads the state for a flow that began under `account`; None when it changed.
    fn read_in<R>(&self, account: u64, f: impl FnOnce(&Inner) -> R) -> Option<R> {
        let g = self.lock();
        (g.account == account).then(|| f(&g))
    }

    pub fn state(&self) -> AppState {
        self.lock().state.clone()
    }

    /// The current account count.
    fn account(&self) -> u64 {
        self.lock().account
    }

    /// What a flow captures when it starts, read in one step: the client, the account count
    /// it belongs to, and whatever `f` reads from the state.
    fn signed_in_with<R>(&self, f: impl FnOnce(&Inner) -> R) -> Res<(Client, u64, R)> {
        let g = self.lock();
        let client = g.client.clone().ok_or_else(not_signed_in)?;
        Ok((client, g.account, f(&g)))
    }

    fn signed_in(&self) -> Res<(Client, u64)> {
        self.signed_in_with(|_| ()).map(|(c, a, ())| (c, a))
    }

    fn client(&self) -> Res<Client> {
        self.signed_in().map(|(c, _)| c)
    }

    /// The instabox client of the account signed in now, if any (the T3 proxy reads with it).
    pub fn current_client(&self) -> Option<Client> {
        self.client().ok()
    }

    /// Turns the T3 local proxy on (prototype, `INSTAADE_T3_PROXY` as `gate`): Open in T3 Code
    /// and Pair again pair T3 Code with it from here on.
    pub fn set_t3_proxy(&self, hub: Arc<t3proxy::Hub>, gate: t3proxy::Gate) {
        let _ = self.t3_proxy.set((hub, gate));
    }

    /// The T3 proxy when it pairs `ade` with T3 Code at `desktop`: only T3, only with the flag,
    /// only for a tested T3 Code version unless the flag is `any`.
    fn t3_proxy_for(&self, ade: Ade, desktop: Option<&str>) -> Option<Arc<t3proxy::Hub>> {
        if ade != Ade::T3 {
            return None;
        }
        let (hub, gate) = self.t3_proxy.get()?;
        if !gate.takes(desktop) {
            self.log(format!(
                "t3 proxy: T3 Code {} is not a version the proxy was tried with; pairing directly",
                desktop.unwrap_or("(unknown)")
            ));
            return None;
        }
        Some(hub.clone())
    }

    /// Pairs T3 Code with the proxy of running sandbox `id`.
    async fn proxy_pair_link(
        &self,
        hub: &Arc<t3proxy::Hub>,
        client: &Client,
        id: &str,
        account: u64,
    ) -> Res<String> {
        let name = self
            .read_in(account, |i| {
                i.state
                    .sandboxes
                    .iter()
                    .find(|s| s.id == id)
                    .map(|s| s.name.clone())
            })
            .flatten()
            .unwrap_or_else(|| id.to_string());
        hub.pair_link(client, id, &name)
            .await
            .map_err(|e| ErrorView::other(format!("The T3 proxy could not pair: {e:#}")))
    }

    fn gate(self: &Arc<Self>, account: u64) -> FlowGate {
        FlowGate {
            core: self.clone(),
            account,
        }
    }

    /// Opens `url` in the default handler, only while the flow's account is current.
    fn open_for(self: &Arc<Self>, account: u64, url: &str) -> Res<()> {
        self.open_with(account, url, None)
    }

    /// Opens `url` in the app at `app` (else the default handler), only while the flow's
    /// account is current.
    fn open_with(self: &Arc<Self>, account: u64, url: &str, app: Option<&Path>) -> Res<()> {
        guarded(&self.gate(account), || {
            (self.open)(url, app).map_err(ErrorView::other)
        })
        .unwrap_or_else(|| Err(account_changed()))
    }

    /// A step of a flow after it last read the account and before it writes. Tests change
    /// the account here; in the app it does nothing.
    fn step(&self, _name: &str) {
        #[cfg(test)]
        {
            let hook = self
                .hook
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            if let Some(h) = hook {
                h(_name);
            }
        }
    }

    // ---- account changes ----

    /// The one way the account changes. Under the account lock, and only when the account
    /// count is still `since` (when given): ends the current session first, so a refresh of
    /// it can neither save nor clear tokens from then on (a sign-out also clears its tokens);
    /// saves a signed-in session's tokens; clears the MCP key and the SSH tokens unless the
    /// session saved by the last run comes in; then puts the new session, client and an empty
    /// account state in place and moves the count on, in one step. Emits and starts the
    /// logout of the ended session once the locks are released.
    fn transition(&self, since: Option<u64>, next: Next) -> Option<Changed> {
        let account_lock = self
            .side_effects
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let old = {
            let i = self.lock();
            if since.is_some_and(|s| s != i.account) {
                return None;
            }
            if matches!(next, Next::Saved(_)) && i.session.is_some() {
                return None;
            }
            i.session.clone()
        };
        // Read before the session ends: an ended session has no token.
        let token = old.as_ref().and_then(|s| s.current_access_token());
        let saved = matches!(next, Next::Saved(_));
        let mut errors: Vec<String> = Vec::new();
        if let Some(s) = &old {
            match next {
                Next::Nobody => errors.extend(s.sign_out().err().map(|e| e.to_string())),
                _ => s.retire(),
            }
        }
        let session = match next {
            Next::Nobody => None,
            Next::Saved(s) => Some(s),
            Next::SignedIn(s) => match s.save() {
                Ok(()) => Some(s),
                Err(e) => {
                    errors.push(e.to_string());
                    // The ended session's tokens must not stay behind either.
                    if let Some(o) = &old {
                        errors.extend(o.sign_out().err().map(|e| e.to_string()));
                    }
                    None
                }
            },
        };
        if !saved {
            errors.extend(self.keys.clear().err());
            errors.extend(self.ssh.clear_all().err());
        }
        let error = errors.into_iter().next();
        let signed_in = session.is_some();
        let (account, state) = {
            let mut i = self.lock();
            i.account += 1;
            i.state = i.state.signed_out();
            i.data = Data::default();
            i.session = session.map(Arc::new);
            i.client = i.session.clone().map(|s| {
                let creds: Arc<dyn Credentials> = s;
                Client::new(self.base.clone(), creds)
            });
            i.state.signed_in = signed_in;
            i.state.error = error.clone().map(ErrorView::other);
            (i.account, i.state.clone())
        };
        drop(account_lock);
        (self.emit)(&state);
        if let (Some(token), Ok(rt)) = (token, tokio::runtime::Handle::try_current()) {
            // Best effort, in the background: the local change never waits for the server.
            let base = self.base.clone();
            rt.spawn(async move { auth::logout_with(&base, &token).await });
        }
        Some(Changed {
            account,
            signed_in,
            error,
        })
    }

    /// Puts the session the last run saved in place, unless the account changed since
    /// `since` (a device sign-in finished while the keychain was read).
    pub fn install_session(self: &Arc<Self>, session: Session, since: u64) -> bool {
        let ended = session.ended();
        let Some(c) = self.transition(Some(since), Next::Saved(session)) else {
            return false;
        };
        self.watch_session(ended, c.account);
        true
    }

    /// Puts a device sign-in's session in place and saves its tokens, unless the account
    /// changed since the sign-in began under `since`. Returns the new account count.
    fn install_signed_in(self: &Arc<Self>, session: Session, since: u64) -> Res<u64> {
        let ended = session.ended();
        let c = self
            .transition(Some(since), Next::SignedIn(session))
            .ok_or_else(account_changed)?;
        if !c.signed_in {
            return Err(ErrorView::other(c.error.unwrap_or_default()));
        }
        self.watch_session(ended, c.account);
        Ok(c.account)
    }

    /// Waits, in the background, for the session installed as `account` to end, then ends
    /// the account with it. Whatever ends the session (a refresh 401 inside any command or
    /// refresh, a sign-out, a replacement), this runs once, and only the server's end of a
    /// session still in place changes anything.
    fn watch_session(self: &Arc<Self>, ended: Option<watch::Receiver<bool>>, account: u64) {
        let (Some(mut ended), Ok(rt)) = (ended, tokio::runtime::Handle::try_current()) else {
            return;
        };
        let core = Arc::downgrade(self);
        rt.spawn(async move {
            // An error means the session is gone without an end: nothing to do.
            let over = ended.wait_for(|e| *e).await.is_ok();
            if over && let Some(core) = core.upgrade() {
                core.end_session(account);
            }
        });
    }

    /// When the session installed as `account` is still in place and ended on the server
    /// (a refresh got 401), the account ends here too: the same change as a sign-out,
    /// without the logout call. After a sign-out or a replacement the count has moved on
    /// and this does nothing.
    fn end_session(&self, account: u64) {
        self.step("session ended");
        let ended = self
            .read_in(account, |i| {
                i.session.as_ref().is_some_and(|s| s.is_signed_out())
            })
            .unwrap_or(false);
        if ended {
            self.transition(Some(account), Next::Nobody);
        }
    }

    /// The app is quitting: clears a copied secret whose clearing is still pending, such as
    /// Paradev's SSH address, when the clipboard still holds exactly it. Waits at most
    /// `QUIT_WAIT`, and a failure never stops the quit.
    pub fn on_quit(&self) {
        self.clipboard.clear_pending_within(QUIT_WAIT);
    }

    pub fn set_start_at_login(&self, on: bool) {
        self.update(|i| i.state.start_at_login = on);
    }

    pub fn set_update(&self, update: Option<UpdateView>) {
        self.update(|i| i.state.update = update);
    }

    /// The old copies of the app to offer, or None once they are gone or dismissed.
    pub fn set_old_app(&self, old: Option<OldAppView>) {
        self.update(|i| i.state.old_app = old);
    }

    pub fn open_url(&self, url: &str) -> Res<()> {
        let r = if allowed_url(url) {
            (self.open)(url, None).map_err(ErrorView::other)
        } else {
            Err(ErrorView::other("This link cannot be opened"))
        };
        self.settle(r)
    }

    /// Runs the result of a user action on the current screen through the error rules: a
    /// failure shows in `state.error`, the next success clears it. Not for a result that
    /// comes after an await: that is `settle_in`. It never waits for the account lock, so the
    /// synchronous commands on the main thread never block on it.
    pub fn settle<T>(&self, r: Res<T>) -> Res<T> {
        self.update(|i| {
            match &r {
                Ok(_) => i.state.error = None,
                Err(e) => i.state.error = Some(e.clone()),
            }
            i.session_flags();
        });
        r
    }

    /// `settle` for a flow that began under `account`: when the account changed since, the
    /// result or error belongs to nobody on screen and is only returned. The check is part
    /// of the state change.
    fn settle_in<T>(&self, account: u64, r: Res<T>) -> Res<T> {
        self.step("settle");
        self.update_in(account, |i| {
            match &r {
                Ok(_) => i.state.error = None,
                Err(e) => i.state.error = Some(e.clone()),
            }
            i.session_flags();
        });
        r
    }

    // ---- launch and refresh ----

    pub async fn start(self: &Arc<Self>) {
        let found = detect_local().await;
        self.update(|i| {
            for (ade, v) in found {
                i.state.set_desktop(ade, v);
            }
        });
        let since = self.account();
        let base = self.base.clone();
        let session = tokio::task::spawn_blocking(move || Session::load(&base))
            .await
            .ok()
            .flatten();
        if let Some(s) = session
            && self.install_session(s, since)
        {
            self.refresh_and_sync(true).await;
            self.refresh_mcp().await;
        }
    }

    /// Reads machines, and with `full` also the account, templates, snapshots and the
    /// desktop ADE versions. Lists only; nothing here runs a command on a sandbox.
    pub async fn refresh(self: &Arc<Self>, full: bool) -> Vec<SyncJob> {
        self.refresh_listed(full).await.0
    }

    /// `refresh`, also with the ids of every sandbox of the account when its whole machine
    /// list came back (every page). None when the list failed or the account changed meanwhile.
    async fn refresh_listed(self: &Arc<Self>, full: bool) -> (Vec<SyncJob>, Option<Listed>) {
        let Ok((client, account, no_me)) = self.signed_in_with(|i| i.data.me.is_none()) else {
            return (Vec::new(), None);
        };
        let full = full || no_me;
        let machines = client.machines().await;
        let account_reads = if full {
            let (me, orgs, templates, snapshots) = tokio::join!(
                client.me(),
                client.orgs(),
                client.templates(),
                client.snapshots()
            );
            Some((me, orgs, templates, snapshots, detect_local().await))
        } else {
            None
        };
        let was_full = account_reads.is_some();
        let user = match &account_reads {
            Some((Ok(me), ..)) => Some(me.user_id),
            _ => self
                .read_in(account, |i| i.data.me.as_ref().map(|m| m.user_id))
                .flatten(),
        };
        let connected = self.load_connected(user, account).await;
        // When the account changed while the reads were in flight, they are not its data.
        let jobs = self.update_in(account, |i| {
            let mut failure: Option<ErrorView> = None;
            if let Some((user, set, problem)) = connected {
                i.data.use_connected(user, set);
                if let Some(e) = problem {
                    // Shown once: the list is loaded again only for another user.
                    i.state.error = Some(ErrorView::other(e));
                }
            }
            let mut listed = None;
            match machines {
                Ok(m) => {
                    i.state.progress = prune_progress(i.state.progress.take(), &m);
                    listed = Some(Listed {
                        account,
                        ids: m.iter().map(|m| m.id.clone()).collect(),
                    });
                    i.data.machines = m;
                }
                Err(e) => failure = Some(e.into()),
            }
            let mut found = Vec::new();
            if let Some((me, orgs, templates, snapshots, desktop)) = account_reads {
                match me {
                    Ok(v) => i.data.me = Some(v),
                    Err(e) => failure = Some(e.into()),
                }
                if let Ok(v) = orgs {
                    i.data.orgs = v;
                }
                if let Ok(v) = templates {
                    i.data.templates = v;
                }
                if let Ok(v) = snapshots {
                    i.data.snapshots = v;
                }
                found = desktop;
            }
            i.rebuild();
            // A failed poll is silent; only a failed full read shows.
            if let (true, Some(e)) = (was_full, failure) {
                i.state.error = Some(e);
            }
            let jobs = i.set_desktop_versions(found);
            i.session_flags();
            (jobs, listed)
        });
        jobs.unwrap_or_default()
    }

    /// The connected set of `user` from disk, when it is not the one in use.
    /// A list that cannot be read comes back empty, with the reason.
    async fn load_connected(
        self: &Arc<Self>,
        user: Option<i64>,
        account: u64,
    ) -> Option<(i64, HashSet<String>, Option<String>)> {
        let user = user.filter(|u| self.lock().data.connected_user != Some(*u))?;
        let store = self.connected.clone();
        let core = self.clone();
        let r = tokio::task::spawn_blocking(move || store.load_checked(user, &core.gate(account)))
            .await
            .ok()?;
        Some(match r {
            Ok(set) => (user, set, None),
            Err(e) => (user, HashSet::new(), Some(e)),
        })
    }

    /// Looks for the ADEs' desktop apps on this computer now (the "Check again" button),
    /// instead of waiting for the next full refresh.
    pub async fn refresh_ades(self: &Arc<Self>) -> Res<()> {
        let found = detect_local().await;
        let jobs = self.update(|i| i.set_desktop_versions(found));
        self.spawn_sync(jobs);
        Ok(())
    }

    /// `refresh`, then syncs the sandboxes after a desktop version change in the background. A
    /// full one (at startup, at sign-in and every few minutes) also renews the tokens of the
    /// SSH hosts in the background.
    pub async fn refresh_and_sync(self: &Arc<Self>, full: bool) {
        let gone = self.refresh_and_find_gone(full).await;
        if full {
            let core = self.clone();
            tokio::spawn(async move { core.renew_ssh_hosts(gone).await });
        }
    }

    /// `refresh` and its syncs. A full one also finds the SSH hosts (Orca, Paseo) whose sandbox the
    /// account's whole machine list no longer has. Only hosts written before the list was read
    /// count: one written since may be of a sandbox made after it. None when the list failed,
    /// was not whole, or the account changed.
    async fn refresh_and_find_gone(self: &Arc<Self>, full: bool) -> Option<Gone> {
        let before = if full { self.host_ids().await } else { None };
        let (jobs, listed) = self.refresh_listed(full).await;
        self.spawn_sync(jobs);
        let (before, listed) = (before?, listed?);
        Some(Gone {
            account: listed.account,
            ids: before
                .into_iter()
                .filter(|id| !listed.ids.contains(id))
                .collect(),
        })
    }

    /// The sandbox ids of the SSH hosts InstaADE wrote; None when the file cannot be read.
    async fn host_ids(&self) -> Option<Vec<String>> {
        let home = self.home.clone();
        match tokio::task::spawn_blocking(move || ssh_config::aliases(&home)).await {
            Ok(Ok(hosts)) => Some(hosts.into_iter().map(|(id, _)| id).collect()),
            Ok(Err(e)) => {
                self.log(format!("SSH hosts: could not read them: {e:#}"));
                None
            }
            Err(_) => None,
        }
    }

    /// Writes a line to the app's log (stderr). Tests read the lines back.
    fn log(&self, line: String) {
        eprintln!("{line}");
        #[cfg(test)]
        self.logs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line);
    }

    /// `ade`'s SSH token of sandbox `id` (`Ade::ssh_token`), from `ssh::token`: the stored one
    /// while it has more than a day left, else a new 30-day one.
    async fn ade_token(
        self: &Arc<Self>,
        ade: Ade,
        client: &Client,
        id: &str,
        account: u64,
    ) -> Res<crate::api::SshToken> {
        let kind = ade.ssh_token().unwrap_or(Kind::Ordinary);
        let t = ssh::token(client, &*self.ssh, id, kind, &self.gate(account))
            .await
            .map_err(ErrorView::from)?;
        self.note_ordinary(kind, &t);
        Ok(t)
    }

    /// Logs, once per run, that the server made an ordinary token where `kind` asked for a
    /// background one, as an instabox without background tokens does. The token is used all
    /// the same.
    fn note_ordinary(&self, kind: Kind, t: &crate::api::SshToken) {
        use std::sync::atomic::Ordering::SeqCst;
        if ssh::ordinary_in_place_of_background(kind, t) && !self.ordinary_logged.swap(true, SeqCst)
        {
            self.log(
                "SSH tokens: the server made an ordinary token where a background one was asked \
                 for, so the connections of cmux, Orca and Paseo count as activity"
                    .to_string(),
            );
        }
    }

    /// Keeps every SSH host InstaADE wrote (Orca, Paseo) working without a click. For each sandbox
    /// with a host, a stored background token with fewer than 7 days left, or none, is renewed
    /// the way `ssh::token` makes one, and the token store rewrites the host's `User` as it
    /// saves it. So a host an older release wrote with the ordinary token gets the background
    /// one at the first pass, and keeps its alias. The ordinary token is not renewed here: no
    /// host holds it. A token with 7 days or more is left alone. Tokens are made over the API
    /// only, so no sandbox wakes. A sandbox the API no longer has loses its host. Every write goes through
    /// the gate of the account the pass began under, so nothing happens once it changed. A
    /// failure is logged and tried again on the next full refresh; the popover never shows it.
    /// A host that could not take a new token keeps the stored token behind, so the next pass
    /// makes another one and tries again, and a host that fell behind the stored token gets
    /// it. The hosts in `gone` (their sandbox is not in the account's whole machine list) are
    /// removed instead, whatever their token. One pass runs at a time. Returns what it logged.
    async fn renew_ssh_hosts(self: &Arc<Self>, gone: Option<Gone>) -> Vec<String> {
        use std::sync::atomic::Ordering::SeqCst;
        let mut logged = Vec::new();
        let mut log = |line: String| {
            self.log(line.clone());
            logged.push(line);
        };
        if self.renewing.swap(true, SeqCst) {
            return logged;
        }
        struct Done<'a>(&'a std::sync::atomic::AtomicBool);
        impl Drop for Done<'_> {
            fn drop(&mut self) {
                self.0.store(false, SeqCst);
            }
        }
        let _done = Done(&self.renewing);
        let Ok((client, account)) = self.signed_in() else {
            return logged;
        };
        let home = self.home.clone();
        let hosts = match tokio::task::spawn_blocking(move || ssh_config::aliases(&home)).await {
            Ok(Ok(hosts)) => hosts,
            Ok(Err(e)) => {
                log(format!(
                    "SSH hosts: could not read them to renew their tokens: {e:#}"
                ));
                return logged;
            }
            Err(_) => return logged,
        };
        let gate = self.gate(account);
        let margin = chrono::Duration::days(HOST_TOKEN_RENEW_DAYS);
        // Only the list of the account this pass runs for.
        let gone: HashSet<String> = gone
            .filter(|g| g.account == account)
            .map(|g| g.ids.into_iter().collect())
            .unwrap_or_default();
        for (id, _) in hosts {
            if self.account() != account {
                return logged;
            }
            self.step("renew");
            if gone.contains(&id) {
                let home = self.home.clone();
                if let Some(Err(e)) = guarded(&gate, || ssh_config::remove_host(&home, &id)) {
                    log(format!(
                        "SSH host of sandbox {id}: could not remove it: {e:#}"
                    ));
                }
                continue;
            }
            let kind = Kind::Background;
            match ssh::token_and_save(&client, &*self.ssh, &id, kind, &gate, margin).await {
                // The store refused the new token, as it does when the host cannot take it. The
                // host may hold it all the same (it is written before the keychain), so an
                // ordinary token in place of a background one is still said once.
                Ok((t, Err(e))) => {
                    self.note_ordinary(kind, &t);
                    log(format!("SSH host of sandbox {id}: {e}"));
                }
                // The host holds the token in use, also when it fell behind the stored one, or
                // holds the ordinary token an older release put there.
                Ok((t, Ok(()))) => {
                    self.note_ordinary(kind, &t);
                    let home = self.home.clone();
                    if let Some(Err(e)) =
                        guarded(&gate, || ssh_config::update_login(&home, &id, &t))
                    {
                        log(format!(
                            "SSH host of sandbox {id}: could not update it: {e:#}"
                        ));
                    }
                }
                // Gone only when the sandbox itself is gone too, not on any 404.
                Err(e) if matches!(e.status, 404 | 410) => match client.machine(&id).await {
                    Err(e) if matches!(e.status, 404 | 410) => {
                        let home = self.home.clone();
                        if let Some(Err(e)) = guarded(&gate, || ssh_config::remove_host(&home, &id))
                        {
                            log(format!(
                                "SSH host of sandbox {id}: could not remove it: {e:#}"
                            ));
                        }
                    }
                    _ => log(format!(
                        "SSH host of sandbox {id}: could not renew its token: {}",
                        e.message
                    )),
                },
                Err(e) => log(format!(
                    "SSH host of sandbox {id}: could not renew its token: {}",
                    e.message
                )),
            }
        }
        logged
    }

    fn spawn_sync(self: &Arc<Self>, jobs: Vec<SyncJob>) {
        for job in jobs.into_iter().filter(|j| !j.ids.is_empty()) {
            let core = self.clone();
            tokio::spawn(async move { core.sync_all(job).await });
        }
    }

    /// One sync per running connected sandbox. This exec follows a version change; it is
    /// not a timer. A sandbox that cannot be updated shows `updatePending`; nothing retries.
    async fn sync_all(&self, job: SyncJob) {
        let Some(client) = self.read_in(job.account, |i| i.client.clone()).flatten() else {
            return;
        };
        for id in &job.ids {
            let r = sandbox::sync::sync(&client, job.ade, id, &job.version).await;
            self.record_sync(id, &job.version, r.as_ref().ok(), job.account);
        }
    }

    fn record_sync(&self, id: &str, desktop: &str, outcome: Option<&SyncOutcome>, account: u64) {
        self.update_in(account, |i| {
            i.data.record_sync(id, desktop, outcome);
            i.rebuild();
        });
    }

    fn record_version(&self, id: &str, version: &str, account: u64) {
        self.update_in(account, |i| {
            i.data.set_version(id, version);
            i.rebuild();
        });
    }

    pub async fn refresh_mcp(&self) {
        let home = self.home.clone();
        let status = tokio::task::spawn_blocking(move || {
            let installed = mcp::claude_installed();
            mcp_status(
                installed,
                installed && mcp::configured(&home, McpClient::ClaudeCode),
                mcp::configured(&home, McpClient::Codex),
            )
        })
        .await
        .unwrap_or_default();
        self.update(|i| i.state.mcp = status);
    }

    /// Refreshes machines on a timer until the process ends.
    pub async fn run_loop(self: Arc<Self>, window_visible: impl Fn() -> bool) {
        let mut last = Instant::now();
        let mut last_full = Instant::now();
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if last.elapsed() < refresh_interval(window_visible()) || !self.state().signed_in {
                continue;
            }
            last = Instant::now();
            let full = last_full.elapsed() >= FULL_REFRESH;
            if full {
                last_full = Instant::now();
            }
            self.refresh_and_sync(full).await;
        }
    }

    // ---- account ----

    /// A device sign-in. It is a flow of the account it began under (usually none): when
    /// another account change happens first, it shows nothing, opens nothing and installs
    /// nothing.
    pub async fn sign_in(self: &Arc<Self>, provider: Provider) -> Res<()> {
        let since = self.account();
        match self.sign_in_inner(provider, since).await {
            Ok(_) => {
                self.refresh_and_sync(true).await;
                self.refresh_mcp().await;
                Ok(())
            }
            Err(e) => {
                self.update_in(since, |i| i.state.sign_in_code = None);
                self.settle_in(since, Err(e))
            }
        }
    }

    async fn sign_in_inner(self: &Arc<Self>, provider: Provider, since: u64) -> Res<u64> {
        let start = auth::start(&self.base, provider)
            .await
            .map_err(|e| ErrorView::from_auth(&e))?;
        let code = start.user_code.clone();
        self.update_in(since, |i| i.show_sign_in_code(code))
            .ok_or_else(account_changed)?;
        self.open_for(since, start.browser_url())?;
        let session = auth::poll(&self.base, &start)
            .await
            .map_err(|e| ErrorView::from_auth(&e))?;
        self.install_signed_in(session, since)
    }

    /// An account boundary: forgets the sign-in tokens, the MCP key and every SSH token,
    /// and clears the account state, the user's connected set included (it stays on disk
    /// for that user). Then it ends the session on the server in the background, with the
    /// token it had, and never refreshes for that. The state clears even when a store
    /// fails; the first failure is returned and shown.
    pub async fn sign_out(&self) -> Res<()> {
        match self.transition(None, Next::Nobody) {
            Some(Changed { error: Some(e), .. }) => Err(ErrorView::other(e)),
            _ => Ok(()),
        }
    }

    pub fn switch_org(&self, org_id: &str) -> Res<()> {
        let r = self.update(|i| {
            if i.data.orgs.iter().any(|o| o.id == org_id) {
                i.state.org_id = Some(org_id.to_string());
                i.rebuild();
                Ok(())
            } else {
                Err(ErrorView::other("Unknown organization"))
            }
        });
        self.settle(r)
    }

    // ---- sandbox actions ----

    async fn desktop_version(&self, ade: Ade) -> Res<String> {
        if let Some(v) = self.lock().state.desktop(ade) {
            return Ok(v.to_string());
        }
        match detect(ade).await {
            Some(v) => {
                self.update(|i| i.state.set_desktop(ade, Some(v.clone())));
                Ok(v)
            }
            None => Err(ErrorView::other(ade.missing())),
        }
    }

    /// The ADE for an action on sandbox `id`: the one asked for, else the sandbox's own. An
    /// error when the sandbox is marked with an ADE this build does not know.
    fn ade_for(&self, id: &str, asked: Option<Ade>) -> Res<Ade> {
        asked
            .or_else(|| self.lock().data.ade_of(id))
            .ok_or_else(unknown_ade)
    }

    pub async fn create_sandbox(self: &Arc<Self>, a: CreateArgs) -> Res<Notice> {
        let (client, account, org_id) = match self.signed_in_with(|i| i.state.org_id.clone()) {
            Ok(v) => v,
            Err(e) => return self.settle(Err(e)),
        };
        let version = match a.ade {
            Some(ade) => match self.desktop_version(ade).await {
                Ok(v) => Some(v),
                Err(e) => return self.settle_in(account, Err(e)),
            },
            None => None,
        };
        if let Some(ade) = a.ade
            && let Err(e) = self.check_app_first(ade).await
        {
            return self.settle_in(account, Err(e));
        }
        let req = NewSandbox {
            name: a.name,
            source: a.snapshot_id.map_or(Source::Standard, Source::Snapshot),
            spec: MachineSpec {
                vcpus: a.vcpus,
                mem_mib: a.mem_mib,
            },
            auto_suspend_after_seconds: a.auto_suspend_after_seconds,
            org_id,
        };
        self.begin_progress("create", "creating", None, a.ade, account);
        let (Some(ade), Some(version)) = (a.ade, version) else {
            let r = connect::create(&client, &req, &self.progress_cb(account)).await;
            return self.finish_create(r, account).await.map(|()| None);
        };
        let r =
            connect::create_and_connect(&client, ade, &req, &version, self.progress_cb(account))
                .await;
        self.finish_connect(ade, r, &version, account).await
    }

    /// Connects an existing sandbox: wakes it and sets the ADE up. A pairing ADE (T3) is
    /// installed (never an older version) and paired, then its app opens with the pairing
    /// link; a link ADE (cmux) gets its prep command, then its link opens; an SSH host ADE
    /// (Orca, Paseo) gets its setup (Orca's probe, Paseo's daemon), then its host written and
    /// its app started.
    /// A host ADE (Paradev) is checked: once its host runs, the sandbox is marked and opens in
    /// it; else the user sets the host up in the app (`open_host`). A terminal ADE (Herdr) is
    /// installed, then opens in a terminal (`open_terminal`).
    pub async fn connect_sandbox(self: &Arc<Self>, id: &str, ade: Option<Ade>) -> Res<Notice> {
        let (client, account) = match self.signed_in() {
            Ok(v) => v,
            Err(e) => return self.settle(Err(e)),
        };
        let ade = match self.ade_for(id, ade) {
            Ok(a) => a,
            Err(e) => return self.settle_in(account, Err(e)),
        };
        let version = match self.desktop_version(ade).await {
            Ok(v) => v,
            Err(e) => return self.settle_in(account, Err(e)),
        };
        if let Err(e) = self.check_app_first(ade).await {
            return self.settle_in(account, Err(e));
        }
        self.begin_progress(
            "connect",
            "connecting",
            Some(id.to_string()),
            Some(ade),
            account,
        );
        let r =
            connect::connect_existing(&client, ade, id, &version, self.progress_cb(account)).await;
        self.finish_connect(ade, r, &version, account).await
    }

    /// Open in the ADE's app. A pairing ADE (T3) on a sandbox connected from here and marked
    /// with it: wake it, sync the ADE, then open a new pairing link (no setup, no mark change),
    /// so the app gets the sandbox back even when it lost it. No link when the sync left a setup
    /// that would refuse the pairing (`sync_before_pairing`). A link ADE (cmux) on a sandbox
    /// marked with it: wake it, since its SSH link does not wake a paused one, then open its
    /// link with an SSH token. An SSH host ADE (Orca, Paseo) on a sandbox marked with it: wake
    /// it, write its host and start the app.
    /// A host ADE (Paradev) on a sandbox marked with it: wake it (the app does not), then open
    /// its link. A terminal ADE (Herdr) on a sandbox marked with it: `open_terminal`. Anything
    /// else goes through the connect flow.
    pub async fn open_in_ade(self: &Arc<Self>, id: &str, asked: Option<Ade>) -> Res<Notice> {
        let snapshot = self.signed_in_with(|i| {
            // None: the sandbox is marked only with ADEs this build does not know.
            let ade = asked.or_else(|| i.data.ade_of(id));
            let connected = i.state.sandboxes.iter().any(|s| s.id == id && s.connected);
            let ready = ade.is_some_and(|a| {
                i.data.ades_of(id).contains(&a)
                    && match a.open() {
                        Open::Pair(_) => connected,
                        Open::Link(_) => true,
                        Open::SshHost(_) => true,
                        Open::Host(_) => true,
                        Open::Terminal(_) => true,
                    }
            });
            (ready, ade, ade.and_then(|a| i.sync_version(a)))
        });
        let (client, account, ade, version) = match snapshot {
            Ok((client, account, (true, Some(ade), version))) => (client, account, ade, version),
            _ => return self.connect_sandbox(id, asked).await,
        };
        let r = async {
            if let Open::Terminal(_) = ade.open() {
                return self.open_terminal(ade, id, account).await;
            }
            if let Open::Host(_) = ade.open() {
                // The app is checked before the sandbox wakes, and again right before it gets
                // the link (`open_host`): the wake can take minutes.
                self.check_app_first(ade).await?;
                sandbox::ensure_running(&client, id)
                    .await
                    .map_err(ErrorView::from)?;
                let link = connect::host_link(&client, ade, id)
                    .await
                    .map_err(ErrorView::from)?;
                return self.open_host(ade, id, Some(&link), account).await;
            }
            let Open::Pair(_) = ade.open() else {
                return self.open_link(ade, id, account).await;
            };
            sandbox::ensure_running(&client, id)
                .await
                .map_err(ErrorView::from)?;
            if let Some(v) = &version {
                let (out, pairing) = sandbox::sync::sync_before_pairing(&client, ade, id, v).await;
                self.record_sync(id, v, out.as_ref().ok(), account);
                // A sandbox whose old setup would refuse this pairing is not paired: T3 Code
                // would only show "credential invalid".
                pairing.map_err(ErrorView::from)?;
            }
            let link = match self.t3_proxy_for(ade, version.as_deref()) {
                // Prototype: T3 Code pairs with the sandbox's local proxy.
                Some(hub) => {
                    let link = self.proxy_pair_link(&hub, &client, id, account).await?;
                    if self.offer_t3_switch(&hub, id, account).await {
                        return Ok(None);
                    }
                    link
                }
                // The port URL stays the same, so the app updates the environment it saved.
                None => connect::pair_link(&client, ade, id)
                    .await
                    .map_err(ErrorView::from)?,
            };
            self.open_for(account, &link).map(|()| None)
        };
        let out = self.act(account, r).await;
        if out.is_ok() {
            self.refresh_probe(ade, id, account);
        }
        out
    }

    /// After an Open of sandbox `id` in `ade` that ran nothing there, installs or updates the
    /// ADE's probe in the background (`Ade::probe_on_open`). Once per sandbox and probe hash
    /// while the app runs: it skips a sandbox whose record (`Data::open_probes`) has this hash,
    /// one whose setup is in progress (the setup installs the probe), and one it already works
    /// on. Open never waits for it. A failure is only logged and records nothing, so the next
    /// Open tries again.
    fn refresh_probe(self: &Arc<Self>, ade: Ade, id: &str, account: u64) {
        let Some(probe) = ade.probe_on_open() else {
            return;
        };
        let hash = probe.hash();
        let key = (id.to_string(), ade);
        let client = self
            .read_in(account, |i| {
                let current = i.data.open_probes.get(&key) == Some(&hash);
                let setting_up = i
                    .state
                    .progress
                    .as_ref()
                    .is_some_and(|p| p.machine_id.as_deref() == Some(id) && p.error.is_none());
                (!current && !setting_up)
                    .then(|| i.client.clone())
                    .flatten()
            })
            .flatten();
        let Some(client) = client else {
            return;
        };
        let mut probing = self.probing.lock().unwrap_or_else(PoisonError::into_inner);
        if !probing.insert(key.clone()) {
            return;
        }
        drop(probing);
        let core = self.clone();
        let task = tokio::spawn(async move {
            let req = crate::api::ExecRequest {
                cmd: probe.script(),
                timeout_seconds: Some(PROBE_TIMEOUT_SECS),
            };
            let failed = match client.exec(&key.0, &req).await {
                Ok(r) if r.exit_code == 0 && !r.timed_out => {
                    core.update_in(account, |i| {
                        i.data.open_probes.insert(key.clone(), hash);
                    });
                    None
                }
                Ok(r) if r.timed_out => Some(format!("timed out after {PROBE_TIMEOUT_SECS} s")),
                Ok(r) => Some(format!(
                    "exit {}: {}",
                    r.exit_code,
                    r.stderr.trim().chars().take(200).collect::<String>()
                )),
                Err(e) => Some(e.to_string()),
            };
            if let Some(why) = failed {
                core.log(format!(
                    "{}: could not install its idle probe on sandbox {}: {why}",
                    key.1.name(),
                    key.0
                ));
            }
            core.probing
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&key);
        });
        #[cfg(test)]
        self.probe_tasks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(task);
        #[cfg(not(test))]
        drop(task);
    }

    /// Test only: waits for every probe install an Open started.
    #[cfg(test)]
    async fn probes_settled(&self) {
        let tasks: Vec<_> = self
            .probe_tasks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
            .collect();
        for t in tasks {
            t.await.unwrap();
        }
    }

    /// Opens a link ADE's link to sandbox `id`. The link holds an SSH token, so it opens only
    /// in the ADE's own app found on this computer, never in whatever handles its scheme. It
    /// checks the app, wakes the sandbox (the SSH gateway refuses a paused one, and the link
    /// does not wake it), and checks the app again (identifier and signature) right before the
    /// token is made or loaded, since the wake can take minutes: with no app, or one that fails
    /// the check, no token is made and nothing opens. A wake that fails is the error, and
    /// nothing opens. The token is the sandbox's background one (`ade_token`): the app stays
    /// connected on its own, and its connections must not keep the sandbox awake.
    ///
    /// An SSH host ADE (Orca, Paseo) goes the same way. It checks the app, wakes the sandbox,
    /// checks the app again, makes or loads the background token, writes it into the sandbox's
    /// host in the user's SSH config, copies the host's address for an app that adds hosts by
    /// it (Paseo), starts the app that passed the last check and returns what the user does
    /// next there. A host an older release wrote with the ordinary token keeps its alias and
    /// gets the background token as its `User`.
    async fn open_link(self: &Arc<Self>, ade: Ade, id: &str, account: u64) -> Res<Notice> {
        let open = ade.open();
        if let Open::Pair(_) = open {
            return Err(ErrorView::other(format!("{} opens by pairing", ade.name())));
        }
        if let Open::Host(_) = open {
            return Err(host_not_link(ade));
        }
        if let Open::Terminal(_) = open {
            return Err(terminal_not_link(ade));
        }
        // Before the sandbox is touched: with no app, or one that fails the check, nothing wakes.
        self.checked_app(ade).await?;
        let (client, title) = self
            .read_in(account, |i| {
                let title = i
                    .data
                    .machines
                    .iter()
                    .find(|m| m.id == id)
                    .map_or_else(|| id.to_string(), |m| m.name.clone());
                i.client.clone().map(|c| (c, title))
            })
            .flatten()
            .ok_or_else(account_changed)?;
        // The app connects next, and the SSH gateway refuses a paused sandbox: an SSH link does
        // not wake it (cmux showed "Session open refused by peer"). Its name makes the link's
        // title and the host's alias, and a sandbox made just now is not in the list yet.
        let m = sandbox::ensure_running(&client, id)
            .await
            .map_err(ErrorView::from)?;
        let title = if m.name.is_empty() { title } else { m.name };
        // A wake can take minutes: the app is found and checked again right before the token,
        // and the app that passes this check is the one opened.
        let app = self.checked_app(ade).await?;
        let t = self.ade_token(ade, &client, id, account).await?;
        match open {
            Open::Link(l) => {
                let link = (l.link)(&SshLogin {
                    host: &t.host,
                    port: t.port,
                    user: &t.token,
                    title: &title,
                });
                self.open_with(account, &link, Some(&app)).map(|()| None)
            }
            Open::SshHost(h) => {
                // Written only while the account is current, so a sign-out leaves no token.
                let alias = guarded(&self.gate(account), || {
                    ssh_config::write_host(&self.home, id, &title, &t)
                })
                .ok_or_else(account_changed)?
                .map_err(|e| {
                    ErrorView::other(format!(
                        "Could not add the sandbox to your SSH config: {e:#}"
                    ))
                })?;
                if let Some(address) = h.address {
                    self.copy_address(ade, address(&alias)).await;
                }
                self.open_with(account, &app.to_string_lossy(), None)?;
                Ok(Some((h.notice)(&alias)))
            }
            Open::Pair(_) => Err(ErrorView::other(format!("{} opens by pairing", ade.name()))),
            Open::Host(_) => Err(host_not_link(ade)),
            Open::Terminal(_) => Err(terminal_not_link(ade)),
        }
    }

    /// Copies the address the user adds an SSH host by in `ade`'s app (Paseo's `ssh://<alias>`).
    /// It holds no token, so it stays on the clipboard. The notice names it too, so a copy that
    /// fails (no clipboard here yet, as on Windows) is only logged.
    async fn copy_address(self: &Arc<Self>, ade: Ade, address: String) {
        // pbcopy blocks for a moment, so off the async runtime.
        let core = self.clone();
        let copied = tokio::task::spawn_blocking(move || core.clipboard.copy_plain(&address))
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r);
        if let Err(e) = copied {
            self.log(format!(
                "{}: the host address was not copied: {e}",
                ade.name()
            ));
        }
    }

    /// The ADE's app on this computer, found and checked now (cmux, Orca, Paradev, Paseo: its
    /// identifier and signature), never the version found earlier. An error when there is no app or it fails
    /// the check.
    async fn checked_app(&self, ade: Ade) -> Res<PathBuf> {
        let (locate, verify) = (self.locate.clone(), self.verify.clone());
        let checked = tokio::task::spawn_blocking(move || {
            locate(ade).map(|app| {
                let ok = verify(ade, &app);
                (app, ok)
            })
        })
        .await
        .ok()
        .flatten();
        match checked {
            Some((app, true)) => Ok(app),
            Some((_, false)) => Err(ErrorView::other(format!(
                "Could not verify the {} app. Install it again from its official download.",
                ade.name()
            ))),
            None => Err(ErrorView::other(ade.missing())),
        }
    }

    /// For an ADE whose app reaches the sandbox over SSH (cmux, Orca, Paradev, Paseo), the app is
    /// checked before the sandbox is touched: with no app, or one that fails the check, nothing
    /// wakes, is created or is marked. `open_link` and `open_host` check it again right before
    /// the token. For a terminal ADE (Herdr), the OpenSSH client is looked for the same way.
    async fn check_app_first(&self, ade: Ade) -> Res<()> {
        if matches!(ade.open(), Open::Pair(_)) {
            return Ok(());
        }
        self.checked_app(ade).await.map(drop)
    }

    /// Opens a host ADE (Paradev) for running sandbox `id`, only in the ADE's own app, checked
    /// again right before. With the sandbox's `link` (its host runs), it opens the sandbox in
    /// the app. Without one, the user sets the host up in the app: the SSH address, with the
    /// sandbox's ordinary SSH token (`ade_token`), goes on the clipboard, the app opens its host
    /// setup, and the answer is the ADE's notice of what to do there. With no app, or one that
    /// fails the check, no token is made, nothing is copied and nothing opens.
    async fn open_host(
        self: &Arc<Self>,
        ade: Ade,
        id: &str,
        link: Option<&str>,
        account: u64,
    ) -> Res<Notice> {
        let Open::Host(h) = ade.open() else {
            return Err(ErrorView::other(format!(
                "{} does not run sandboxes as hosts",
                ade.name()
            )));
        };
        let app = self.checked_app(ade).await?;
        if let Some(link) = link {
            self.open_with(account, link, Some(&app))?;
            return Ok(None);
        }
        let client = self
            .read_in(account, |i| i.client.clone())
            .flatten()
            .ok_or_else(account_changed)?;
        let t = self.ade_token(ade, &client, id, account).await?;
        let address = (h.ssh_address)(&SshLogin {
            host: &t.host,
            port: t.port,
            user: &t.token,
            title: "",
        });
        // pbcopy blocks for a moment, so off the async runtime. The address is cleared a minute
        // later, unless the user copied something else since.
        let core = self.clone();
        tokio::task::spawn_blocking(move || {
            guarded(&core.gate(account), || core.clipboard.copy(&address))
        })
        .await
        .map_err(|e| ErrorView::other(e.to_string()))?
        .ok_or_else(account_changed)?
        .map_err(ErrorView::other)?;
        self.open_with(account, h.setup_link, Some(&app))?;
        Ok(Some(h.notice.to_string()))
    }

    /// Opens a terminal ADE (Herdr) on sandbox `id`: the user's terminal opens with an SSH
    /// session that runs the ADE on the sandbox. The OpenSSH client is looked for first, then
    /// the sandbox wakes, since the SSH gateway refuses a paused one. The client is looked for
    /// again right before the token, since the wake can take minutes, and the one found then is
    /// the one that runs: its exact path goes to the terminal, never a bare `ssh` the terminal
    /// would look up. The token is the sandbox's ordinary one (`ade_token`), as for Open in
    /// Terminal, and goes to that client only, on its command line, never through the SSH host
    /// Orca and Paseo use. A host or token that is not plain opens nothing
    /// (`ssh::terminal_args`).
    async fn open_terminal(self: &Arc<Self>, ade: Ade, id: &str, account: u64) -> Res<Notice> {
        let Open::Terminal(t) = ade.open() else {
            return Err(ErrorView::other(format!(
                "{} does not open in a terminal",
                ade.name()
            )));
        };
        self.check_app_first(ade).await?;
        let client = self
            .read_in(account, |i| i.client.clone())
            .flatten()
            .ok_or_else(account_changed)?;
        sandbox::ensure_running(&client, id)
            .await
            .map_err(ErrorView::from)?;
        let ssh_client = self.checked_app(ade).await?;
        let tok = self.ade_token(ade, &client, id, account).await?;
        let args = ssh::terminal_args(&tok, t.command).map_err(ErrorView::other)?;
        guarded(&self.gate(account), || (self.terminal)(&ssh_client, &args))
            .ok_or_else(account_changed)?
            .map_err(ErrorView::other)?;
        Ok(None)
    }

    /// Makes a new pairing link for a connected sandbox and opens it. Like Open, it makes none
    /// when the sandbox's old setup would refuse the pairing and could not be brought up to date.
    pub async fn pair_again(self: &Arc<Self>, id: &str, ade: Option<Ade>) -> Res<()> {
        let (client, account) = self.signed_in()?;
        let linked = std::sync::atomic::AtomicBool::new(false);
        let r = async {
            let ade = self.ade_for(id, ade)?;
            // Like Open: a sandbox whose old setup would refuse this pairing is synced first,
            // and gets no link when that could not bring its setup up to date.
            let desktop = self.read_in(account, |i| i.sync_version(ade)).flatten();
            let link = if let Some(hub) = self.t3_proxy_for(ade, desktop.as_deref()) {
                // Prototype: T3 Code pairs with the sandbox's local proxy. The proxy pairs with
                // T3 there with DPoP, which needs the current setup just as T3 Code would: the
                // same sync first, the same errors.
                connect::ready_for_pairing_again(&client, ade, id, desktop.as_deref(), |v, out| {
                    self.record_sync(id, v, out, account)
                })
                .await
                .map_err(ErrorView::from)?;
                let link = self.proxy_pair_link(&hub, &client, id, account).await?;
                if self.offer_t3_switch(&hub, id, account).await {
                    return Ok(());
                }
                link
            } else {
                connect::pair_again(&client, ade, id, desktop.as_deref(), |v, out| {
                    self.record_sync(id, v, out, account)
                })
                .await
                .map_err(ErrorView::from)?
            };
            linked.store(true, std::sync::atomic::Ordering::SeqCst);
            self.open_for(account, &link)
        };
        let out = self.act(account, r).await;
        if linked.load(std::sync::atomic::Ordering::SeqCst) {
            // After `act`, which clears the error: a failed save stays visible.
            self.remember(id, account).await;
        }
        out
    }

    /// Shows the T3 switch guide instead of opening the proxy's pairing link, when T3 Code most
    /// likely saved sandbox `id` with its published port: InstaADE paired it from this computer
    /// before (`connected`), yet T3 Code never paired with its proxy (`Hub::switch_guide`). That
    /// pairing was then a direct one, and T3 Code refuses a pairing link for an environment it
    /// saved under another address. True when the guide shows.
    async fn offer_t3_switch(&self, hub: &t3proxy::Hub, id: &str, account: u64) -> bool {
        let Some(guide) = hub.switch_guide(id).await else {
            return false;
        };
        self.update_in(account, |i| {
            if !i.data.connected.contains(id) {
                return false;
            }
            let name = i
                .state
                .sandboxes
                .iter()
                .find(|s| s.id == id)
                .map_or_else(|| id.to_string(), |s| s.name.clone());
            i.state.t3_switch = Some(T3SwitchView {
                id: id.to_string(),
                name,
                old_address: guide.old_address,
                new_address: guide.new_address,
            });
            true
        }) == Some(true)
    }

    /// Copies a new pairing URL of sandbox `id`'s T3 proxy, which T3 Code's Add route takes.
    /// Like Paradev's address, it is cleared from the clipboard a minute later.
    pub async fn copy_t3_switch_url(self: &Arc<Self>, id: &str) -> Res<()> {
        let (_, account) = self.signed_in()?;
        let hub = self
            .t3_proxy
            .get()
            .map(|(hub, _)| hub.clone())
            .ok_or_else(|| ErrorView::other("The T3 proxy is off."))?;
        let url = hub.pairing_url(id).await.ok_or_else(|| {
            ErrorView::other(
                "The T3 proxy of this sandbox is not running. Use Open in T3 Code again.",
            )
        })?;
        let core = self.clone();
        tokio::task::spawn_blocking(move || {
            guarded(&core.gate(account), || core.clipboard.copy(&url))
        })
        .await
        .map_err(|e| ErrorView::other(e.to_string()))?
        .ok_or_else(account_changed)?
        .map_err(ErrorView::other)
    }

    /// Closes the T3 switch guide of sandbox `id`. `done`: the user finished it or turned it
    /// off, so it is not offered again for this sandbox.
    pub async fn close_t3_switch(&self, id: &str, done: bool) {
        if done && let Some((hub, _)) = self.t3_proxy.get() {
            hub.finish_switch_guide(id).await;
        }
        self.update(|i| {
            if i.state.t3_switch.as_ref().is_some_and(|g| g.id == id) {
                i.state.t3_switch = None;
            }
        });
    }

    /// Adds the sandbox to the set the user connected from this computer, on disk and in
    /// the state. Without a known user it stays in memory until the account loads.
    /// Nothing changes, in memory or on disk, once the account changed since `account`.
    async fn remember(self: &Arc<Self>, id: &str, account: u64) {
        let user = self.update_in(account, |i| {
            i.data.connected.insert(id.to_string());
            i.rebuild();
            i.data.user()
        });
        let Some(Some(user)) = user else { return };
        let (store, core, id) = (self.connected.clone(), self.clone(), id.to_string());
        let r =
            tokio::task::spawn_blocking(move || store.add(user, &id, &core.gate(account))).await;
        self.report_store(r, account);
    }

    /// Removes the sandbox from the set the user connected from this computer.
    /// Nothing changes, in memory or on disk, once the account changed since `account`.
    async fn forget(self: &Arc<Self>, id: &str, account: u64) {
        let user = self.update_in(account, |i| {
            i.data.connected.remove(id);
            i.rebuild();
            i.data.user()
        });
        let Some(Some(user)) = user else { return };
        let (store, core, id) = (self.connected.clone(), self.clone(), id.to_string());
        let r =
            tokio::task::spawn_blocking(move || store.remove(user, &id, &core.gate(account))).await;
        self.report_store(r, account);
    }

    /// Shows a failed connected-list change, only to the account it was made for.
    fn report_store(&self, r: Result<Result<(), String>, tokio::task::JoinError>, account: u64) {
        let err = match r {
            Ok(Ok(())) => return,
            Ok(Err(e)) => e,
            Err(e) => e.to_string(),
        };
        self.update_in(account, |i| {
            i.state.error = Some(ErrorView::other(format!(
                "Could not save the sandboxes connected from this computer: {err}"
            )));
        });
    }

    fn begin_progress(
        &self,
        kind: &str,
        step: &str,
        machine_id: Option<String>,
        ade: Option<Ade>,
        account: u64,
    ) {
        self.update_in(account, |i| {
            i.state.error = None;
            i.state.progress = Some(ProgressView {
                kind: kind.to_string(),
                step: step.to_string(),
                machine_id,
                ade,
                error: None,
            });
        });
    }

    /// Moves the progress on. A progress that was dismissed or pruned stays gone.
    fn progress_cb(&self, account: u64) -> impl Fn(Step) + '_ {
        move |step| {
            self.update_in(account, |i| {
                if let Some(p) = &i.state.progress {
                    i.state.progress = Some(progress_after(p, &step));
                }
            });
        }
    }

    /// The × on a failed progress.
    pub fn dismiss_progress(&self) {
        self.update(Inner::clear_progress);
    }

    /// Shows the result of a create with no ADE. A failure ends the progress: its Retry would
    /// set an ADE up.
    async fn finish_create(
        self: &Arc<Self>,
        r: Result<String, SandboxError>,
        account: u64,
    ) -> Res<()> {
        if self.account() != account {
            return Ok(());
        }
        let r = r.map(drop).map_err(ErrorView::from);
        self.update_in(account, Inner::clear_progress);
        let out = self.settle_in(account, r);
        if self.account() == account {
            self.refresh_and_sync(false).await;
        }
        out
    }

    /// Shows the result of a create or connect that began under `account`, and opens the
    /// ADE: the pairing link, a link ADE's link, or a host ADE's link or host setup. Every
    /// write checks the account as part of the write, so a change at any point drops the rest.
    async fn finish_connect(
        self: &Arc<Self>,
        ade: Ade,
        r: Result<Connected, SandboxError>,
        version: &str,
        account: u64,
    ) -> Res<Notice> {
        if self.account() != account {
            // Signed out or switched account while this ran: drop the result.
            return Ok(None);
        }
        self.step("finish");
        let out = match r {
            Ok(c) => {
                // A sandbox that kept a newer version is not moved to the desktop's: the
                // desktop is behind. One that kept an older version because the ADE was busy
                // offers the update.
                self.update_in(account, |i| {
                    i.state.progress = None;
                    if let Some(v) = &c.version {
                        i.data.set_version(&c.machine_id, v);
                        if c.update_pending {
                            i.data.update_pending.insert(c.machine_id.clone());
                        } else if v != version {
                            i.data.desktop_behind.insert(c.machine_id.clone());
                        }
                    }
                    i.rebuild();
                });
                let opened = match &c.link {
                    _ if matches!(ade.open(), Open::Host(_)) => {
                        self.open_host(ade, &c.machine_id, c.link.as_deref(), account)
                            .await
                    }
                    _ if matches!(ade.open(), Open::Terminal(_)) => {
                        self.open_terminal(ade, &c.machine_id, account).await
                    }
                    Some(link) => match self.t3_proxy_for(ade, Some(version)) {
                        // Prototype: T3 Code pairs with the sandbox's local proxy instead.
                        Some(hub) => match self.current_client() {
                            Some(client) => self
                                .proxy_pair_link(&hub, &client, &c.machine_id, account)
                                .await
                                .and_then(|l| self.open_for(account, &l).map(|()| None)),
                            None => Err(not_signed_in()),
                        },
                        None => self.open_for(account, link).map(|()| None),
                    },
                    None => self.open_link(ade, &c.machine_id, account).await,
                };
                let out = self.settle_in(account, opened);
                // The connected set says a pairing ADE (T3) was paired from here. A link ADE
                // (cmux) pairs nothing, so it must not add the sandbox: T3 would then count
                // as paired here when another computer set it up.
                if matches!(ade.open(), Open::Pair(_)) {
                    // After `settle_in`, which clears the error: a failed save stays visible.
                    self.remember(&c.machine_id, account).await;
                }
                out
            }
            Err(e) => {
                let view = ErrorView::from(e);
                self.update_in(account, |i| {
                    i.state.progress = fail_progress(i.state.progress.as_ref(), &view);
                });
                self.settle_in(account, Err(view))
            }
        };
        if self.account() == account {
            self.refresh_and_sync(false).await;
        }
        out
    }

    /// Runs a sandbox action of `account`, then refreshes machines.
    async fn act<T>(
        self: &Arc<Self>,
        account: u64,
        f: impl std::future::Future<Output = Res<T>>,
    ) -> Res<T> {
        let r = f.await;
        let out = self.settle_in(account, r);
        if self.account() == account {
            self.refresh_and_sync(false).await;
        }
        out
    }

    pub async fn pause(self: &Arc<Self>, id: &str) -> Res<()> {
        let (client, account) = self.signed_in()?;
        self.act(account, async {
            lifecycle::pause(&client, id).await.map_err(ErrorView::from)
        })
        .await
    }

    /// Wakes the sandbox. A connected one also gets its ADE brought in step with the desktop:
    /// the one that keeps in step (T3), whatever other ADEs it has.
    pub async fn resume(self: &Arc<Self>, id: &str) -> Res<()> {
        // Only a sandbox connected from this computer, to an ADE this build knows, gets its
        // ADE synced after a resume.
        let (client, account, sync) = self.signed_in_with(|i| {
            let ade = i
                .data
                .pinned_ade_of(id)
                .filter(|_| i.data.connected.contains(id))?;
            i.sync_version(ade).map(|v| (ade, v))
        })?;
        let r = async {
            match &sync {
                Some((ade, v)) => {
                    let o = lifecycle::resume(&client, *ade, id, v)
                        .await
                        .map_err(ErrorView::from)?;
                    if let Some(o) = o {
                        self.record_sync(id, v, Some(&o), account);
                    }
                    Ok(())
                }
                None => sandbox::ensure_running(&client, id)
                    .await
                    .map(|_| ())
                    .map_err(ErrorView::from),
            }
        };
        self.act(account, r).await
    }

    pub async fn delete(self: &Arc<Self>, id: &str) -> Res<()> {
        let (client, account) = self.signed_in()?;
        let r = self
            .act(account, async {
                lifecycle::delete(&client, id)
                    .await
                    .map_err(ErrorView::from)
            })
            .await;
        if r.is_ok() {
            self.update_in(account, |i| {
                i.data.ade_versions.remove(id);
                i.data.update_pending.remove(id);
                i.data.desktop_behind.remove(id);
                i.data.open_probes.retain(|(m, _), _| m != id);
                // A progress of this sandbox can only retry a sandbox that is gone.
                let of_this = i.state.progress.as_ref().map(|p| p.machine_id.as_deref());
                if of_this == Some(Some(id)) {
                    i.clear_progress();
                }
                i.rebuild();
            });
            self.forget(id, account).await;
            // Its SSH host (Orca, Paseo) goes too. The delete itself succeeded, so a failure
            // here is only logged; the next full refresh removes a host whose sandbox is not listed.
            let (home, gone) = (self.home.clone(), id.to_string());
            match tokio::task::spawn_blocking(move || ssh_config::remove_host(&home, &gone)).await {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => self.log(format!(
                    "SSH host of sandbox {id}: could not remove it: {e:#}"
                )),
                Err(e) => self.log(format!(
                    "SSH host of sandbox {id}: could not remove it: {e}"
                )),
            }
        }
        r
    }

    pub async fn resize(self: &Arc<Self>, id: &str, vcpus: u32, mem_mib: u32) -> Res<()> {
        let (client, account) = self.signed_in()?;
        self.act(account, async {
            lifecycle::resize(&client, id, vcpus, mem_mib)
                .await
                .map_err(ErrorView::from)
        })
        .await
    }

    pub async fn set_auto_sleep(self: &Arc<Self>, id: &str, seconds: i64) -> Res<()> {
        let (client, account) = self.signed_in()?;
        self.act(account, async {
            lifecycle::set_auto_sleep(&client, id, seconds)
                .await
                .map_err(ErrorView::from)
        })
        .await
    }

    /// Sets the ADE up again on the sandbox at the desktop version.
    pub async fn reinstall(self: &Arc<Self>, id: &str, ade: Option<Ade>) -> Res<()> {
        let (client, account) = self.signed_in()?;
        let r = async {
            let ade = self.ade_for(id, ade)?;
            let v = self.desktop_version(ade).await?;
            lifecycle::reinstall(&client, ade, id, &v)
                .await
                .map_err(ErrorView::from)?;
            self.record_version(id, &v, account);
            Ok(())
        };
        self.act(account, r).await
    }

    pub async fn agent_sign_in(self: &Arc<Self>, id: &str, agent: Agent) -> Res<()> {
        let (client, account) = self.signed_in()?;
        let open = self.open.clone();
        let gate = self.gate(account);
        let r = async {
            agents::sign_in(
                &client,
                &*self.ssh,
                id,
                agent,
                move |u| open(u, None).map_err(anyhow::Error::msg),
                &gate,
            )
            .await
            .map_err(|e| anyhow_view(&e))
        };
        self.act(account, r).await
    }

    /// Whether `agent` is signed in on the sandbox. One status exec, and only when the
    /// sandbox is running: a sleeping sandbox answers false without being woken.
    pub async fn agent_status(&self, id: &str, agent: Agent) -> Res<bool> {
        let client = self.client()?;
        let machine = client.machine(id).await.map_err(ErrorView::from)?;
        if machine.state != "running" {
            return Ok(false);
        }
        Ok(agents::signed_in(&client, id, agent).await)
    }

    /// The SSH login command to show and copy, with the sandbox's ordinary token: the user
    /// runs it, and it wakes the sandbox. The details view shows a failure next to the SSH row,
    /// so this leaves `state.error` alone.
    pub async fn ssh_command(self: &Arc<Self>, id: &str) -> Res<String> {
        let r = match self.signed_in() {
            Ok((client, account)) => self.login_command(&client, id, account).await,
            Err(e) => Err(e),
        };
        if r.is_err() {
            self.update(Inner::session_flags);
        }
        r
    }

    async fn login_command(
        self: &Arc<Self>,
        client: &Client,
        id: &str,
        account: u64,
    ) -> Res<String> {
        let t = ssh::token(client, &*self.ssh, id, Kind::Ordinary, &self.gate(account))
            .await
            .map_err(ErrorView::from)?;
        Ok(ssh::display_command(&t))
    }

    /// Opens the sandbox's SSH login in a terminal, with its ordinary token, built from the
    /// token's fields (`ssh::login_args`), never from the command string the server formats. A
    /// host or token that is not plain opens nothing.
    pub async fn open_ssh(self: &Arc<Self>, id: &str) -> Res<()> {
        let (client, account) = match self.signed_in() {
            Ok(v) => v,
            Err(e) => return self.settle(Err(e)),
        };
        let r = async {
            let gate = self.gate(account);
            let t = ssh::token(&client, &*self.ssh, id, Kind::Ordinary, &gate)
                .await
                .map_err(ErrorView::from)?;
            let args = ssh::login_args(&t).map_err(ErrorView::other)?;
            guarded(&self.gate(account), || {
                (self.terminal)(Path::new("ssh"), &args)
            })
            .ok_or_else(account_changed)?
            .map_err(ErrorView::other)
        };
        self.settle_in(account, r.await)
    }

    pub async fn add_mcp(self: &Arc<Self>, which: McpClient) -> Res<()> {
        let (client, account) = self.signed_in()?;
        let r = mcp::add(
            &client,
            &mcp::mcp_url(&self.base),
            &self.home,
            which,
            self.keys.clone(),
            Arc::new(self.gate(account)),
        )
        .await
        .map_err(|e| anyhow_view(&e));
        let out = self.settle_in(account, r);
        self.refresh_mcp().await;
        out
    }
}

// ---- tests ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{TemplateRef, Tokens};
    use crate::auth::{MemoryStore, TokenStore};
    use crate::connected::{FileConnectedStore, MemoryConnectedStore};
    use crate::mcp::MemoryKeyStore;
    use crate::ssh::MemorySshStore;
    use crate::updater::UpdateStep;
    use chrono::TimeZone;
    use std::collections::BTreeMap;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 19, 12, 0, 0).unwrap()
    }

    fn spec(v: u32, m: u32) -> MachineSpec {
        MachineSpec {
            vcpus: v,
            mem_mib: m,
        }
    }

    fn me() -> Me {
        Me {
            user_id: 7,
            login: "alex".into(),
            allowed_specs: vec![spec(2, 4096), spec(4, 8192)],
            org: Some(crate::api::OrgRef {
                id: "org-me".into(),
                name: Some("alex".into()),
                tier: "free".into(),
                kind: "personal".into(),
                always_on_allowed: Some(false),
            }),
            fixed_auto_suspend_seconds: None,
            idle_choices_seconds: vec![300, 900, 1800],
            always_on_allowed: false,
        }
    }

    fn org(id: &str, tier: &str, specs: Vec<MachineSpec>) -> Org {
        Org {
            id: id.into(),
            name: id.into(),
            tier: tier.into(),
            kind: if id == "org-me" { "personal" } else { "team" }.into(),
            personal: id == "org-me",
            allowed_specs: specs,
        }
    }

    fn template(id: &str, os: Option<&str>, pool: Option<&str>) -> Template {
        Template {
            id: id.into(),
            os: os.map(str::to_string),
            pool: pool.map(str::to_string),
            hidden: false,
            creatable: true,
            latest: true,
        }
    }

    fn machine(id: &str, org: &str, tpl: &str, state: &str, connected: bool) -> Machine {
        let mut metadata = BTreeMap::new();
        if connected {
            metadata.insert("instabox-bridge".to_string(), "t3".to_string());
        }
        Machine {
            id: id.into(),
            name: format!("name-{id}"),
            state: state.into(),
            desired_state: None,
            revision: None,
            template: TemplateRef { id: tpl.into() },
            spec: spec(4, 8192),
            auto_suspend_after_seconds: Some(900),
            max_run_minutes: -1,
            run_started_at: None,
            cooldown_until: None,
            auto_suspended_at: None,
            metadata,
            org_id: org.into(),
            free: false,
            operation_id: None,
        }
    }

    fn data() -> Data {
        Data {
            me: Some(me()),
            orgs: vec![
                org("org-me", "free", vec![]),
                org("org-team", "pro", vec![spec(8, 16384)]),
            ],
            templates: vec![
                template("agent-browser", None, None),
                template("mac", Some("macos"), None),
                template("gpu", None, Some("gpu")),
            ],
            machines: vec![
                machine("a", "org-me", "agent-browser", "running", true),
                machine("b", "org-team", "agent-browser", "running", true),
                machine("c", "org-me", "mac", "running", false),
                machine("d", "org-me", "gpu", "suspended", false),
                machine("e", "org-me", "agent-browser", "suspended", true),
                machine("f", "org-me", "agent-browser", "running", false),
                machine("g", "org-me", "custom-unknown", "running", false),
            ],
            snapshots: vec![
                snap("s1", "ready", "agent-browser", None),
                snap("s2", "creating", "agent-browser", None),
                snap("s3", "ready", "mac", None),
                snap("s4", "ready", "agent-browser", Some("org-team")),
            ],
            ade_versions: HashMap::from([("a".to_string(), "0.1.10".to_string())]),
            update_pending: HashSet::from(["e".to_string()]),
            desktop_behind: HashSet::from(["g".to_string()]),
            connected: HashSet::from(["a".to_string(), "b".to_string(), "e".to_string()]),
            connected_user: Some(7),
            open_probes: HashMap::new(),
        }
    }

    fn snap(id: &str, status: &str, tpl: &str, org: Option<&str>) -> Snapshot {
        Snapshot {
            id: id.into(),
            name: format!("snap-{id}"),
            template: TemplateRef { id: tpl.into() },
            status: status.into(),
            org_id: org.map(str::to_string),
        }
    }

    fn ids(v: &[SandboxView]) -> Vec<&str> {
        v.iter().map(|s| s.id.as_str()).collect()
    }

    /// The state the shared fixture `src/fixtures/state.json` holds.
    fn fixture_state() -> AppState {
        let mut d = data();
        d.machines[0].free = true;
        d.machines[0].run_started_at = Some(now() - chrono::Duration::minutes(20));
        d.machines[0].max_run_minutes = 120;
        // e has T3 and cmux.
        d.machines[4]
            .metadata
            .insert("instabox-ades".into(), "t3,cmux".into());
        let mut s = AppState::default();
        s.apply(&d, now());
        s.signed_in = true;
        s.sign_in_code = None;
        s.set_desktop(Ade::T3, Some("0.1.10".into()));
        s.set_desktop(Ade::Cmux, Some("0.64.25".into()));
        s.set_desktop(Ade::Orca, Some("1.4.269".into()));
        s.mcp = McpStatus {
            claude_code: Some(true),
            codex: Some(false),
        };
        s.old_app = Some(OldAppView {
            paths: vec!["/Applications/Instabox.app".into()],
        });
        s.t3_switch = Some(T3SwitchView {
            id: "e".into(),
            name: "name-e".into(),
            old_address: "https://3774-e.instabox.sh".into(),
            new_address: "http://localhost:37800".into(),
        });
        s.progress = Some(ProgressView {
            kind: "create".into(),
            step: "installing".into(),
            machine_id: Some("a".into()),
            ade: Some(Ade::T3),
            error: None,
        });
        s.error = Some(ErrorView::from(SandboxError::Cooldown { minutes: 12 }));
        s.start_at_login = true;
        s.update = Some(UpdateView {
            current: "0.1.7".into(),
            enabled: true,
            step: UpdateStep::Downloading,
            version: Some("0.2.0".into()),
            percent: Some(42),
            error: None,
        });
        s
    }

    #[test]
    fn state_json_matches_the_shared_fixture() {
        let got = serde_json::to_value(fixture_state()).unwrap();
        let want: serde_json::Value =
            serde_json::from_str(include_str!("../../src/fixtures/state.json")).unwrap();
        assert_eq!(got, want);
    }

    #[test]
    fn state_keys_are_camel_case() {
        let got = serde_json::to_value(fixture_state()).unwrap();
        let keys: std::collections::BTreeSet<&str> = got
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let want: std::collections::BTreeSet<&str> = [
            "signedIn",
            "login",
            "signInCode",
            "orgs",
            "orgId",
            "tier",
            "t3",
            "ades",
            "sandboxes",
            "sizes",
            "sleepChoices",
            "snapshots",
            "mcp",
            "progress",
            "oldApp",
            "t3Switch",
            "error",
            "startAtLogin",
            "update",
        ]
        .into_iter()
        .collect();
        assert_eq!(keys, want);
    }

    #[test]
    fn selected_org_filters_machines_and_hides_non_linux() {
        let d = data();
        let mut s = AppState::default();
        s.apply(&d, now());
        // org-me (the personal default): a, e, f and the unknown-template g.
        // Hidden: b (other org), c (macOS), d (pool template).
        assert_eq!(s.org_id.as_deref(), Some("org-me"));
        assert_eq!(ids(&s.sandboxes), ["a", "e", "f", "g"]);
        s.org_id = Some("org-team".into());
        s.apply(&d, now());
        assert_eq!(ids(&s.sandboxes), ["b"]);
    }

    #[test]
    fn templates_that_name_linux_still_show_their_machines() {
        // Since Nov 26, 2026 the API names every template's os, and Linux templates say "linux".
        let mut d = data();
        for t in &mut d.templates {
            if t.os.is_none() && t.pool.is_none() {
                t.os = Some("linux".into());
            }
        }
        let mut s = AppState::default();
        s.apply(&d, now());
        assert_eq!(ids(&s.sandboxes), ["a", "e", "f", "g"]);
    }

    #[test]
    fn plan_sizes_and_sleep_follow_the_selected_org() {
        let d = data();
        let mut s = AppState::default();
        s.apply(&d, now());
        // free personal org: the user's specs, one fixed sleep choice.
        assert_eq!(s.tier, "free");
        assert_eq!(s.sizes.len(), 2);
        assert_eq!(s.sleep_choices.len(), 1);
        assert_eq!(s.sleep_choices[0].seconds, 1800);
        s.org_id = Some("org-team".into());
        s.apply(&d, now());
        assert_eq!(s.tier, "pro");
        assert_eq!(s.sizes.len(), 1);
        assert_eq!(s.sizes[0].label, "8 vCPU / 16 GB");
        assert_eq!(
            s.sleep_choices
                .iter()
                .map(|c| c.seconds)
                .collect::<Vec<_>>(),
            [300, 900, 1800]
        );
    }

    #[test]
    fn unknown_selected_org_falls_back_to_the_personal_org() {
        let mut s = AppState {
            org_id: Some("gone".into()),
            ..AppState::default()
        };
        s.apply(&data(), now());
        assert_eq!(s.org_id.as_deref(), Some("org-me"));
    }

    #[test]
    fn orgs_come_from_me_when_the_list_is_empty() {
        let mut d = data();
        d.orgs.clear();
        let mut s = AppState::default();
        s.apply(&d, now());
        assert_eq!(s.orgs.len(), 1);
        assert_eq!(s.orgs[0].id, "org-me");
        assert!(s.orgs[0].personal);
    }

    #[test]
    fn sandbox_views_carry_the_t3_version_seen() {
        let d = data();
        let get = |d: &Data, id: &str| {
            sandbox_views(d, Some("org-me"), now())
                .into_iter()
                .find(|v| v.id == id)
                .unwrap()
        };
        assert_eq!(get(&d, "a").t3_version.as_deref(), Some("0.1.10"));
        assert_eq!(get(&d, "f").t3_version, None);
    }

    #[test]
    fn snapshots_are_ready_linux_and_in_the_selected_org() {
        let d = data();
        let got = |org: &str| {
            snapshot_views(&d.snapshots, &d.templates, Some(org))
                .into_iter()
                .map(|s| s.id)
                .collect::<Vec<_>>()
        };
        // s2 not ready, s3 macOS; s4 belongs to the team org; s1 has no org and shows.
        assert_eq!(got("org-me"), ["s1"]);
        assert_eq!(got("org-team"), ["s1", "s4"]);
    }

    #[test]
    fn desktop_version_change_selects_running_connected_sandboxes_of_every_org() {
        let mut d = data();
        d.machines
            .push(machine("h", "org-me", "agent-browser", "stopped", true));
        // a (org-me) and b (org-team) are running and connected. e: suspended. f/g: running
        // but not connected. h: stopped. c, d: not Linux.
        assert_eq!(sync_targets(&d, Ade::T3), ["a", "b"]);
        assert!(version_changed(Some("0.1.9"), Some("0.1.10")));
        assert!(!version_changed(Some("0.1.10"), Some("0.1.10")));
        assert!(!version_changed(None, Some("0.1.10")), "first read");
        assert!(!version_changed(Some("0.1.10"), None), "T3 not readable");
    }

    fn outcome(decision: Decision, remote: Option<&str>) -> SyncOutcome {
        SyncOutcome {
            decision,
            remote_version: remote.map(str::to_string),
        }
    }

    fn view_of(d: &Data, id: &str) -> SandboxView {
        sandbox_views(d, Some("org-me"), now())
            .into_iter()
            .find(|v| v.id == id)
            .unwrap()
    }

    #[test]
    fn sync_outcomes_set_the_version_and_update_pending() {
        let mut d = data();
        d.update_pending.clear();
        // Defer: the sandbox keeps its own version and is pending.
        d.record_sync(
            "a",
            "0.1.11",
            Some(&outcome(Decision::Defer, Some("0.1.10"))),
        );
        let v = view_of(&d, "a");
        assert!(v.update_pending);
        assert_eq!(v.t3_version.as_deref(), Some("0.1.10"));
        // Older: ahead of the desktop, also pending.
        d.record_sync(
            "a",
            "0.1.11",
            Some(&outcome(Decision::Older, Some("0.1.12"))),
        );
        assert!(
            !view_of(&d, "a").update_pending,
            "ahead is not an update to offer"
        );
        assert!(view_of(&d, "a").desktop_behind);
        assert_eq!(view_of(&d, "a").t3_version.as_deref(), Some("0.1.12"));
        // A failed sync is pending and keeps the known version.
        d.record_sync("e", "0.1.11", None);
        assert!(view_of(&d, "e").update_pending);
        // Update: now at the desktop version, no longer pending.
        d.record_sync(
            "a",
            "0.1.11",
            Some(&outcome(Decision::Update, Some("0.1.10"))),
        );
        let v = view_of(&d, "a");
        assert!(!v.update_pending);
        assert_eq!(v.t3_version.as_deref(), Some("0.1.11"));
        // UpToDate: the sandbox's own version.
        d.record_sync(
            "e",
            "0.1.11",
            Some(&outcome(Decision::UpToDate, Some("0.1.11"))),
        );
        assert!(!view_of(&d, "e").update_pending);
    }

    #[test]
    fn a_marked_sandbox_not_connected_from_here_is_left_alone() {
        let mut d = data();
        // A teammate connected "t": it has the server mark but is not in the local set.
        d.machines
            .push(machine("t", "org-me", "agent-browser", "running", true));
        let t = view_of(&d, "t");
        assert!(!t.connected);
        assert!(!sync_targets(&d, Ade::T3).contains(&"t".to_string()));
        // Once this computer connects it, it is a sync target.
        d.connected.insert("t".into());
        assert!(view_of(&d, "t").connected);
        assert!(sync_targets(&d, Ade::T3).contains(&"t".to_string()));
    }

    #[test]
    fn a_sandbox_of_an_unknown_ade_is_not_synced_as_the_default() {
        let mut d = data();
        // A newer app connected "n" from here to an ADE this build does not know.
        let mut n = machine("n", "org-me", "agent-browser", "running", true);
        n.metadata
            .insert("instabox-bridge".into(), "future-ade".into());
        d.machines.push(n);
        d.connected.insert("n".into());
        assert!(!sync_targets(&d, Ade::T3).contains(&"n".to_string()));
        assert_eq!(d.ade_of("n"), None);
    }

    // A closed local port: a test that reaches the network fails here, never at the real API.
    fn core_with(store: Arc<dyn ConnectedStore>) -> Arc<Core> {
        core_on(Url::parse("http://127.0.0.1:9/").unwrap(), store)
    }

    fn core_on(base: Url, store: Arc<dyn ConnectedStore>) -> Arc<Core> {
        Arc::new(Core::new(
            base,
            Arc::new(MemoryKeyStore::default()),
            Arc::new(MemorySshStore::default()),
            store,
            Arc::new(|_, _| Ok(())),
            Arc::new(|_| {}),
            PathBuf::from("/nonexistent"),
        ))
    }

    fn connected_result(id: &str, version: &str) -> Connected {
        Connected {
            machine_id: id.into(),
            link: Some("t3code://pair?pairingUrl=x".into()),
            version: Some(version.into()),
            update_pending: false,
        }
    }

    #[tokio::test]
    async fn connecting_a_busy_sandbox_with_an_older_t3_offers_the_update() {
        let core = core_with(Arc::new(MemoryConnectedStore::default()));
        core.update(|i| {
            i.data.machines = vec![machine("t", "org-me", "agent-browser", "running", true)];
            i.data.me = Some(me());
            i.data.orgs = data().orgs;
            i.rebuild();
        });
        let c = Connected {
            update_pending: true,
            ..connected_result("t", "0.1.9")
        };
        core.finish_connect(Ade::T3, Ok(c), "0.1.11", core.account())
            .await
            .unwrap();
        let v = &core.state().sandboxes[0];
        assert!(v.connected && v.update_pending && !v.desktop_behind);
        assert_eq!(v.t3_version.as_deref(), Some("0.1.9"));
    }

    #[tokio::test]
    async fn connect_adds_to_the_local_set_and_delete_removes_it() {
        let store = Arc::new(MemoryConnectedStore::default());
        let core = core_with(store.clone());
        core.update(|i| {
            i.data.machines = vec![machine("t", "org-me", "agent-browser", "running", true)];
            i.data.templates = data().templates;
            i.data.me = Some(me());
            i.data.orgs = data().orgs;
            i.rebuild();
        });
        assert!(!core.state().sandboxes[0].connected);
        core.finish_connect(
            Ade::T3,
            Ok(connected_result("t", "0.1.11")),
            "0.1.11",
            core.account(),
        )
        .await
        .unwrap();
        assert!(store.load(7).contains("t"), "saved for the signed-in user");
        let s = core.state();
        assert!(s.sandboxes[0].connected);
        assert_eq!(s.sandboxes[0].t3_version.as_deref(), Some("0.1.11"));
        assert!(!s.sandboxes[0].desktop_behind);
        core.forget("t", core.account()).await;
        assert!(store.load(7).is_empty(), "removed");
        assert!(!core.state().sandboxes[0].connected);
    }

    #[tokio::test]
    async fn connecting_a_sandbox_with_a_newer_t3_marks_the_desktop_behind() {
        let core = core_with(Arc::new(MemoryConnectedStore::default()));
        core.update(|i| {
            i.data.machines = vec![machine("t", "org-me", "agent-browser", "running", true)];
            i.data.me = Some(me());
            i.data.orgs = data().orgs;
            i.rebuild();
        });
        core.finish_connect(
            Ade::T3,
            Ok(connected_result("t", "0.1.12")),
            "0.1.11",
            core.account(),
        )
        .await
        .unwrap();
        let v = &core.state().sandboxes[0];
        assert!(v.desktop_behind && !v.update_pending);
        assert_eq!(v.t3_version.as_deref(), Some("0.1.12"));
    }

    /// A mock API that lists no machines, and an API key session on it.
    async fn listing_server() -> (wiremock::MockServer, Url) {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/machines"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"machines": [], "nextCursor": null})),
            )
            .mount(&server)
            .await;
        let base = Url::parse(&server.uri()).unwrap();
        (server, base)
    }

    fn sign_in_as(core: &Arc<Core>, base: &Url, user_id: i64) {
        let session = Session::load_from(
            base,
            Some("ibk_test".into()),
            Arc::new(MemoryStore::default()),
        )
        .unwrap();
        core.install_signed_in(session, core.account()).unwrap();
        core.update(|i| i.data.me = Some(Me { user_id, ..me() }));
    }

    #[tokio::test]
    async fn the_connected_set_is_per_user_and_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("connected.json");
        let (_server, base) = listing_server().await;
        let first = core_with(Arc::new(FileConnectedStore::new(path.clone())));
        first.update(|i| i.data.me = Some(me()));
        first.remember("m1", first.account()).await;

        // A restart: nothing is loaded until the account is known.
        let again = Arc::new(Core::new(
            base.clone(),
            Arc::new(MemoryKeyStore::default()),
            Arc::new(MemorySshStore::default()),
            Arc::new(FileConnectedStore::new(path)),
            Arc::new(|_, _| Ok(())),
            Arc::new(|_| {}),
            PathBuf::from("/nonexistent"),
        ));
        assert!(again.lock().data.connected.is_empty());
        sign_in_as(&again, &base, 7);
        again.refresh(false).await;
        assert!(again.lock().data.connected.contains("m1"), "user 7's set");

        again.sign_out().await.unwrap();
        assert!(
            again.lock().data.connected.is_empty(),
            "gone with the account"
        );

        sign_in_as(&again, &base, 8);
        again.refresh(false).await;
        assert!(
            again.lock().data.connected.is_empty(),
            "another user does not get user 7's sandboxes"
        );
        sign_in_as(&again, &base, 7);
        again.refresh(false).await;
        assert!(again.lock().data.connected.contains("m1"), "kept on disk");
    }

    #[test]
    fn progress_keeps_the_machine_id_and_failure_keeps_it_for_retry() {
        let start = ProgressView {
            kind: "create".into(),
            step: "creating".into(),
            machine_id: None,
            ade: Some(Ade::Cmux),
            error: None,
        };
        let p1 = progress_after(&start, &Step::Creating);
        assert_eq!(p1.machine_id, None);
        let p2 = progress_after(&p1, &Step::Created("m1".into()));
        let p3 = progress_after(&p2, &Step::Installing("0.1.10".into()));
        assert_eq!(p3.step, "installing");
        assert_eq!(p3.kind, "create");
        assert_eq!(p3.machine_id.as_deref(), Some("m1"));
        assert_eq!(p3.ade, Some(Ade::Cmux), "the ADE stays for Retry");
        let err = ErrorView::from(SandboxError::Setup {
            ade: Ade::T3,
            stderr_tail: "boom".into(),
        });
        let failed = fail_progress(Some(&p3), &err).unwrap();
        assert_eq!(failed.machine_id.as_deref(), Some("m1"));
        assert_eq!(failed.error, Some(err.clone()));
        assert_eq!(fail_progress(Some(&p1), &err), None, "nothing to retry yet");
    }

    fn failed_progress(id: &str) -> ProgressView {
        ProgressView {
            kind: "connect".into(),
            step: "installing".into(),
            machine_id: Some(id.into()),
            ade: Some(Ade::T3),
            error: Some(ErrorView::from(SandboxError::Setup {
                ade: Ade::T3,
                stderr_tail: "tar: bad".into(),
            })),
        }
    }

    #[test]
    fn a_failed_progress_ends_when_its_sandbox_is_gone() {
        let listed = vec![machine("m1", "org-me", "agent-browser", "running", false)];
        let kept = prune_progress(Some(failed_progress("m1")), &listed);
        assert_eq!(kept, Some(failed_progress("m1")), "still listed");
        assert_eq!(
            prune_progress(Some(failed_progress("m2")), &listed),
            None,
            "not listed"
        );
        let deleted = vec![machine("m1", "org-me", "agent-browser", "deleted", false)];
        assert_eq!(prune_progress(Some(failed_progress("m1")), &deleted), None);
        // A running create may not be listed yet: its flow ends it.
        let mut running = failed_progress("m2");
        running.error = None;
        assert_eq!(
            prune_progress(Some(running.clone()), &listed),
            Some(running)
        );
    }

    #[tokio::test]
    async fn deleting_the_sandbox_of_a_failed_progress_ends_it() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        let m1 = serde_json::json!({
            "id": "m1", "name": "n", "state": "running",
            "template": {"id": "agent-browser"}, "spec": {"vcpus": 2, "memMiB": 4096},
            "metadata": {}, "orgId": "org-me"
        });
        Mock::given(method("DELETE"))
            .and(path("/v1/machines/m1"))
            .respond_with(ResponseTemplate::new(202).set_body_json(serde_json::json!({
                "id": "op1", "machineId": "m1", "type": "delete", "state": "succeeded",
                "machine": m1
            })))
            .mount(&server)
            .await;
        // A list that still shows the sandbox: the delete itself ends the progress.
        Mock::given(method("GET"))
            .and(path("/v1/machines"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"machines": [m1], "nextCursor": null})),
            )
            .mount(&server)
            .await;
        let base = Url::parse(&server.uri()).unwrap();
        let core = Arc::new(Core::new(
            base.clone(),
            Arc::new(MemoryKeyStore::default()),
            Arc::new(MemorySshStore::default()),
            Arc::new(MemoryConnectedStore::default()),
            Arc::new(|_, _| Ok(())),
            Arc::new(|_| {}),
            PathBuf::from("/nonexistent"),
        ));
        let session = Session::load_from(
            &base,
            Some("ibk_test".into()),
            Arc::new(MemoryStore::default()),
        )
        .unwrap();
        assert!(core.install_session(session, core.account()));
        core.update(|i| {
            i.data.me = Some(me());
            i.state.progress = Some(failed_progress("m1"));
        });
        core.delete("m1").await.unwrap();
        assert_eq!(core.state().progress, None);
    }

    #[test]
    fn dismiss_ends_the_progress_and_its_copy_of_the_error_only() {
        let core = core_with(Arc::new(MemoryConnectedStore::default()));
        let p = failed_progress("m1");
        core.update(|i| {
            i.state.error = p.error.clone();
            i.state.progress = Some(p.clone());
        });
        core.dismiss_progress();
        assert_eq!(core.state().progress, None);
        assert_eq!(core.state().error, None);
        core.update(|i| {
            i.state.error = Some(ErrorView::other("Unknown organization"));
            i.state.progress = Some(p.clone());
        });
        core.dismiss_progress();
        assert_eq!(core.state().progress, None);
        assert_eq!(
            core.state().error,
            Some(ErrorView::other("Unknown organization")),
            "another error stays"
        );
    }

    #[test]
    fn a_new_desktop_version_brings_the_connected_sandboxes() {
        let core = core_with(Arc::new(MemoryConnectedStore::default()));
        let mut i = core.lock();
        i.data = data();
        assert!(
            i.set_desktop_version(Ade::T3, Some("0.1.10".into()))
                .is_none(),
            "first read"
        );
        let job = i
            .set_desktop_version(Ade::T3, Some("0.1.11".into()))
            .unwrap();
        assert_eq!(job.version, "0.1.11");
        assert_eq!(job.ids, ["a", "b"]);
        assert!(i.set_desktop_version(Ade::T3, None).is_none());
        assert_eq!(i.state.t3, None);
    }

    #[test]
    fn ades_hold_the_desktop_versions_and_t3_mirrors_its_own() {
        let core = core_with(Arc::new(MemoryConnectedStore::default()));
        let s = core.state();
        // Listed from the start; nothing is found before the first look. cmux and Paradev are
        // macOS only.
        if cfg!(target_os = "macos") {
            assert_eq!(
                s.ades,
                vec![
                    AdeView::new(Ade::T3, None),
                    AdeView::new(Ade::Cmux, None),
                    AdeView::new(Ade::Orca, None),
                    AdeView::new(Ade::Paradev, None),
                    AdeView::new(Ade::Paseo, None),
                    AdeView::new(Ade::Herdr, None)
                ]
            );
        }
        if cfg!(windows) {
            assert_eq!(
                s.ades,
                vec![
                    AdeView::new(Ade::T3, None),
                    AdeView::new(Ade::Orca, None),
                    AdeView::new(Ade::Paseo, None),
                    AdeView::new(Ade::Herdr, None)
                ]
            );
        }
        assert_eq!((s.t3.as_deref(), s.desktop(Ade::T3)), (None, None));

        let mut s = AppState::default();
        s.set_desktop(Ade::T3, Some("0.1.10".into()));
        assert_eq!(s.t3.as_deref(), Some("0.1.10"));
        assert_eq!(s.desktop(Ade::T3), Some("0.1.10"));
        let j = serde_json::to_value(&s).unwrap();
        assert_eq!(
            j["ades"],
            serde_json::json!([{
                "id": "t3", "name": "T3 Code", "shortName": "T3", "version": "0.1.10",
                "downloadUrl": "https://github.com/pingdotgg/t3code/releases/latest"
            }])
        );
        s.set_desktop(Ade::T3, None);
        assert_eq!(s.ades.len(), 1, "one entry per ADE");
        assert_eq!((s.t3.as_deref(), s.desktop(Ade::T3)), (None, None));
        s.set_desktop(Ade::T3, Some("0.1.11".into()));
        s.old_app = Some(OldAppView {
            paths: vec!["/Applications/Instabox.app".into()],
        });
        let out = s.signed_out();
        assert_eq!(out.ades, s.ades, "desktop facts stay");
        assert_eq!(out.t3.as_deref(), Some("0.1.11"));
        assert_eq!(out.old_app, s.old_app, "the old app on this computer stays");
    }

    #[test]
    fn a_sandbox_ade_comes_from_its_mark() {
        let mut d = data();
        // a carries the mark v0.1.x wrote; f has none; "x" is not listed.
        assert_eq!(d.ade_of("a"), Some(Ade::T3));
        assert_eq!(d.ade_of("f"), Some(Ade::default()));
        assert_eq!(d.ade_of("x"), Some(Ade::default()));
        d.machines[0]
            .metadata
            .insert("instabox-bridge".into(), "other".into());
        assert_eq!(
            d.ade_of("a"),
            None,
            "an ADE this build does not know is not the default"
        );
        let views = sandbox_views(&data(), Some("org-me"), now());
        let ades: Vec<Vec<Ade>> = views.iter().map(|v| v.ades.clone()).collect();
        assert_eq!(ids(&views), ["a", "e", "f", "g"]);
        assert_eq!(ades, [vec![Ade::T3], vec![Ade::T3], vec![], vec![]]);
    }

    #[test]
    fn a_sandbox_with_several_ades_keeps_t3_in_step() {
        let mut d = data();
        // a: cmux first, then T3. b (running, connected, other org): cmux only.
        for (i, ades) in [(0, vec![Ade::Cmux, Ade::T3]), (1, vec![Ade::Cmux])] {
            let md = &mut d.machines[i].metadata;
            md.clear();
            for a in ades {
                view::mark(md, a);
            }
        }
        assert_eq!(d.machines[0].metadata["instabox-bridge"], "t3");
        assert_eq!(d.ades_of("a"), [Ade::Cmux, Ade::T3]);
        assert_eq!(d.ade_of("a"), Some(Ade::Cmux), "the first one");
        assert_eq!(d.ade_of("b"), Some(Ade::Cmux), "a cmux mark routes to cmux");
        assert_eq!(d.pinned_ade_of("a"), Some(Ade::T3));
        assert_eq!(d.pinned_ade_of("b"), None);
        assert_eq!(
            sync_targets(&d, Ade::T3),
            ["a"],
            "b has no T3 to bring along"
        );
        assert_eq!(sync_targets(&d, Ade::Cmux), ["a", "b"]);
        // No mark: the sandbox has no ADE, and an action with none asked uses the default.
        assert_eq!(d.ades_of("f"), []);
        assert_eq!(d.ade_of("f"), Some(Ade::T3));
        assert_eq!(d.pinned_ade_of("f"), Some(Ade::T3));
        // A mark keeps the ADEs this build knows and skips the rest.
        let md = &mut d.machines[0].metadata;
        md.insert("instabox-bridge".into(), "t3".into());
        md.insert("instabox-ades".into(), "future-ade,t3".into());
        assert_eq!(d.ade_of("a"), Some(Ade::T3));
        assert_eq!(d.pinned_ade_of("a"), Some(Ade::T3));
        let md = &mut d.machines[0].metadata;
        md.insert("instabox-bridge".into(), "future-ade".into());
        md.insert("instabox-ades".into(), "future-ade".into());
        assert_eq!((d.ade_of("a"), d.pinned_ade_of("a")), (None, None));
        // Only the list, with no instabox-bridge, is a mark too.
        d.machines[0].metadata.remove("instabox-bridge");
        assert_eq!(d.ade_of("a"), None);
        assert!(!sync_targets(&d, Ade::T3).contains(&"a".to_string()));
    }

    #[tokio::test]
    async fn the_ades_on_this_computer_are_looked_for_at_once_in_their_order() {
        use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
        static STARTED: AtomicUsize = AtomicUsize::new(0);
        // Each look waits until both have started. One after the other, the first would give
        // up after 5 s and find nothing.
        fn meet(ade: Ade) -> Option<String> {
            STARTED.fetch_add(1, SeqCst);
            let deadline = Instant::now() + Duration::from_secs(5);
            while STARTED.load(SeqCst) < 2 {
                if Instant::now() > deadline {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Some(format!("{}-1.0", ade.id()))
        }
        let found = detect_each(vec![Ade::Cmux, Ade::T3], meet).await;
        assert_eq!(
            found,
            vec![
                (Ade::Cmux, Some("cmux-1.0".to_string())),
                (Ade::T3, Some("t3-1.0".to_string()))
            ]
        );
        // An ADE that is not found leaves the others alone.
        fn only_t3(ade: Ade) -> Option<String> {
            (ade == Ade::T3).then(|| "0.1.10".into())
        }
        assert_eq!(
            detect_each(vec![Ade::T3, Ade::Cmux], only_t3).await,
            vec![(Ade::T3, Some("0.1.10".into())), (Ade::Cmux, None)]
        );
    }

    #[test]
    fn the_sync_version_is_the_known_desktop_version() {
        let core = core_with(Arc::new(MemoryConnectedStore::default()));
        let mut i = core.lock();
        assert_eq!(i.sync_version(Ade::T3), None, "not found yet");
        i.state.set_desktop(Ade::T3, Some("0.1.10".into()));
        assert!(Ade::T3.pins_local_version());
        assert_eq!(i.sync_version(Ade::T3).as_deref(), Some("0.1.10"));
    }

    #[test]
    fn pairing_links_may_be_opened_by_the_ui_and_token_links_never() {
        for a in ade::ALL {
            match a.open() {
                Open::Pair(p) => {
                    assert!(allowed_url(&(p.link)("https://x.test", "c")), "{a:?}");
                }
                Open::Link(l) => {
                    let login = SshLogin {
                        host: "ssh.example.test",
                        port: 2222,
                        user: "ibssh_x",
                        title: "box",
                    };
                    // It holds an SSH token: only `open_link` opens it, in the app it found.
                    assert!(!allowed_url(&(l.link)(&login)), "{a:?}");
                }
                // No link at all: the token goes into the SSH config, never into a URL.
                Open::SshHost(_) => assert!(!allowed_url("://x"), "{a:?}"),
                Open::Host(h) => {
                    // Only `open_host` opens them, in the app it checked.
                    assert!(!allowed_url(h.setup_link), "{a:?}");
                    assert!(!allowed_url(&(h.link)("0123456789abcdef0123456789abcdef")));
                }
                // No link at all: the token goes on the ssh command line in a terminal.
                Open::Terminal(_) => assert!(!allowed_url("://x"), "{a:?}"),
            }
        }
        assert!(!allowed_url("t3codex://x"));
        assert!(!allowed_url("cmux-nightly://ssh?host=x"));
    }

    #[tokio::test]
    async fn the_ssh_command_leaves_the_error_line_alone() {
        let ssh = Arc::new(MemorySshStore::default());
        ssh.save("m1", Kind::Ordinary, &ssh_token()).unwrap();
        let base = api_base(None);
        let core = Arc::new(Core::new(
            base.clone(),
            Arc::new(MemoryKeyStore::default()),
            ssh,
            Arc::new(MemoryConnectedStore::default()),
            Arc::new(|_, _| Ok(())),
            Arc::new(|_| {}),
            PathBuf::from("/nonexistent"),
        ));
        let boom = Some(ErrorView::other("boom"));
        core.update(|i| i.state.error = boom.clone());
        assert!(core.ssh_command("m1").await.is_err(), "signed out");
        assert_eq!(core.state().error, boom, "a failure shows in the row");
        // An API key session; the stored token is reused, so no request is made.
        let session = Session::load_from(
            &base,
            Some("ibk_test".into()),
            Arc::new(MemoryStore::default()),
        )
        .unwrap();
        assert!(core.install_session(session, core.account()));
        core.update(|i| i.state.error = boom.clone());
        assert_eq!(
            core.ssh_command("m1").await.unwrap(),
            "ssh ibssh_x@ssh.example.test"
        );
        assert_eq!(core.state().error, boom, "success does not clear it");
    }

    #[test]
    fn small_decisions() {
        assert_eq!(refresh_interval(true), Duration::from_secs(5));
        assert_eq!(refresh_interval(false), Duration::from_secs(30));
        assert_eq!(api_base(None).as_str(), "https://api.instabox.sh/");
        assert_eq!(
            api_base(Some("http://localhost:8080".into())).as_str(),
            "http://localhost:8080/"
        );
        assert_eq!(
            api_base(Some("nope".into())).as_str(),
            "https://api.instabox.sh/"
        );
        assert!(allowed_url("https://github.com/x"));
        assert!(allowed_url("t3code://pair?pairingUrl=x"));
        assert!(!allowed_url("file:///etc/passwd"));
        assert!(!allowed_url("javascript:alert(1)"));
        assert_eq!(parse_provider("github"), Some(Provider::GitHub));
        assert_eq!(parse_provider("x"), None);
        assert_eq!(parse_mcp_client("claudeCode"), Some(McpClient::ClaudeCode));
        assert_eq!(parse_mcp_client("codex"), Some(McpClient::Codex));
        assert_eq!(mcp_status(false, false, true).claude_code, None);
        assert_eq!(mcp_status(true, false, true).claude_code, Some(false));
    }

    // ---- Core ----

    fn tokens() -> Tokens {
        // The real clock: the session must not refresh before the logout call.
        Tokens {
            access_token: "iba_a".into(),
            access_expires_at: Utc::now() + chrono::Duration::hours(1),
            refresh_token: "ibr_a".into(),
            refresh_expires_at: Utc::now() + chrono::Duration::days(30),
        }
    }

    fn ssh_token() -> crate::api::SshToken {
        crate::api::SshToken {
            host: "ssh.example.test".into(),
            port: 22,
            token: "ibssh_x".into(),
            command: "ssh ibssh_x@ssh.example.test".into(),
            expires_at: Some(Utc::now() + chrono::Duration::days(20)),
            host_key_fingerprint: None,
            background: false,
        }
    }

    /// Signs out a token session against a mock API whose logout answers `status`.
    async fn sign_out_with_logout(status: u16) {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/logout"))
            .and(header("authorization", "Bearer iba_a"))
            .respond_with(ResponseTemplate::new(status))
            .expect(1)
            .mount(&server)
            .await;
        let base = Url::parse(&server.uri()).unwrap();
        let store = Arc::new(MemoryStore::with(tokens()));
        let keys = Arc::new(MemoryKeyStore::default());
        keys.save("ibk_secret").unwrap();
        let ssh = Arc::new(MemorySshStore::default());
        ssh.save("a", Kind::Ordinary, &ssh_token()).unwrap();
        ssh.save("a", Kind::Background, &ssh_token()).unwrap();
        ssh.save("e", Kind::Ordinary, &ssh_token()).unwrap();
        let connected = Arc::new(MemoryConnectedStore::default());
        connected.add(7, "a", &crate::gate::Open).unwrap();
        let emitted = Arc::new(Mutex::new(Vec::<AppState>::new()));
        let sink = emitted.clone();
        let core = Arc::new(Core::new(
            base.clone(),
            keys.clone(),
            ssh.clone(),
            connected.clone(),
            Arc::new(|_, _| Ok(())),
            Arc::new(move |s| sink.lock().unwrap().push(s.clone())),
            PathBuf::from("/nonexistent"),
        ));
        let session = Session::load_from(&base, None, store.clone()).expect("session");
        assert!(core.install_session(session, core.account()));
        core.update(|i| {
            i.data = data();
            i.data.connected_user = Some(7);
            i.rebuild();
            i.state.set_desktop(Ade::T3, Some("0.1.10".into()));
            i.state.start_at_login = true;
        });
        assert!(core.state().signed_in);
        assert!(!core.state().sandboxes.is_empty());

        core.sign_out().await.unwrap();

        assert_eq!(store.load().unwrap(), None, "tokens cleared");
        assert_eq!(keys.load().unwrap(), None, "MCP key cleared");
        for kind in [Kind::Ordinary, Kind::Background] {
            assert_eq!(ssh.load("a", kind).unwrap(), None, "SSH tokens cleared");
            assert_eq!(ssh.load("e", kind).unwrap(), None);
        }
        let s = core.state();
        assert!(!s.signed_in);
        assert!(s.sandboxes.is_empty() && s.orgs.is_empty() && s.login.is_none());
        assert!(core.lock().data.connected.is_empty(), "no connected set");
        assert!(connected.load(7).contains("a"), "kept on disk for user 7");
        assert_eq!(s.t3.as_deref(), Some("0.1.10"), "desktop facts stay");
        assert_eq!(s.desktop(Ade::T3), Some("0.1.10"));
        assert!(s.start_at_login);
        assert!(
            !emitted.lock().unwrap().last().unwrap().signed_in,
            "state emitted"
        );
        assert!(core.client().is_err(), "no client after sign-out");
        for _ in 0..50 {
            if !server
                .received_requests()
                .await
                .unwrap_or_default()
                .is_empty()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        // Dropping the server checks the one logout call.
    }

    #[tokio::test]
    async fn sign_out_ends_the_server_session_and_clears_every_secret() {
        sign_out_with_logout(204).await;
    }

    #[tokio::test]
    async fn sign_out_goes_on_when_the_logout_call_fails() {
        sign_out_with_logout(500).await;
    }

    type WithSession = (
        Arc<Core>,
        Arc<MemoryStore>,
        Arc<MemoryKeyStore>,
        Arc<MemorySshStore>,
        Arc<tokio::sync::Notify>,
    );

    /// A core signed in with `tok` against `server`, its token, key and SSH stores, and a
    /// signal that fires when the state shows the sign-in screen.
    fn core_with_session(server_uri: &str, tok: Tokens) -> WithSession {
        let base = Url::parse(server_uri).unwrap();
        let store = Arc::new(MemoryStore::with(tok));
        let keys = Arc::new(MemoryKeyStore::default());
        keys.save("ibk_secret").unwrap();
        let ssh = Arc::new(MemorySshStore::default());
        ssh.save("a", Kind::Ordinary, &ssh_token()).unwrap();
        ssh.save("a", Kind::Background, &ssh_token()).unwrap();
        let signed_out = Arc::new(tokio::sync::Notify::new());
        let signal = signed_out.clone();
        let core = Arc::new(Core::new(
            base.clone(),
            keys.clone(),
            ssh.clone(),
            Arc::new(MemoryConnectedStore::default()),
            Arc::new(|_, _| Ok(())),
            Arc::new(move |s: &AppState| {
                if !s.signed_in {
                    signal.notify_one();
                }
            }),
            PathBuf::from("/nonexistent"),
        ));
        let session = Session::load_from(&base, None, store.clone()).expect("session");
        assert!(core.install_session(session, core.account()));
        (core, store, keys, ssh, signed_out)
    }

    #[tokio::test]
    async fn sign_out_clears_secrets_without_waiting_for_the_server() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/logout"))
            .respond_with(ResponseTemplate::new(204).set_delay(Duration::from_secs(10)))
            .mount(&server)
            .await;
        let (core, store, keys, ssh, _) = core_with_session(&server.uri(), tokens());
        let started = std::time::Instant::now();
        core.sign_out().await.unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "sign_out waited for the server"
        );
        assert_eq!(store.load().unwrap(), None, "tokens cleared");
        assert_eq!(keys.load().unwrap(), None, "MCP key cleared");
        for kind in [Kind::Ordinary, Kind::Background] {
            assert_eq!(ssh.load("a", kind).unwrap(), None, "SSH tokens cleared");
        }
    }

    #[tokio::test]
    async fn sign_out_never_refreshes_and_logs_out_with_the_captured_token() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/refresh"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/logout"))
            .and(header("authorization", "Bearer iba_a"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
        // The access token is about to expire: bearer() would refresh it.
        let mut tok = tokens();
        tok.access_expires_at = Utc::now() + chrono::Duration::seconds(5);
        let (core, store, _keys, _ssh, _) = core_with_session(&server.uri(), tok);
        core.sign_out().await.unwrap();
        assert_eq!(store.load().unwrap(), None);
        for _ in 0..50 {
            let n = server.received_requests().await.unwrap_or_default().len();
            if n > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        // Dropping the server checks: one logout with the old token, no refresh.
    }

    #[tokio::test]
    async fn sign_out_without_a_session_still_clears_the_mcp_key() {
        let keys = Arc::new(MemoryKeyStore::default());
        keys.save("ibk_secret").unwrap();
        let core = Core::new(
            api_base(None),
            keys.clone(),
            Arc::new(MemorySshStore::default()),
            Arc::new(MemoryConnectedStore::default()),
            Arc::new(|_, _| Ok(())),
            Arc::new(|_| {}),
            PathBuf::from("/nonexistent"),
        );
        core.sign_out().await.unwrap();
        assert_eq!(keys.load().unwrap(), None);
    }

    #[test]
    fn failures_of_small_commands_set_state_error() {
        let core = Core::new(
            api_base(None),
            Arc::new(MemoryKeyStore::default()),
            Arc::new(MemorySshStore::default()),
            Arc::new(MemoryConnectedStore::default()),
            Arc::new(|_, _| Ok(())),
            Arc::new(|_| {}),
            PathBuf::from("/nonexistent"),
        );
        assert!(core.switch_org("nope").is_err());
        assert_eq!(core.state().error.unwrap().message, "Unknown organization");
        assert!(core.open_url("file:///etc/passwd").is_err());
        assert_eq!(
            core.state().error.unwrap().message,
            "This link cannot be opened"
        );
        core.open_url("https://github.com/x").unwrap();
        assert_eq!(core.state().error, None, "next success clears it");
    }

    #[test]
    fn the_old_app_is_offered_until_cleared() {
        let emitted = Arc::new(Mutex::new(Vec::<AppState>::new()));
        let sink = emitted.clone();
        let core = Core::new(
            api_base(None),
            Arc::new(MemoryKeyStore::default()),
            Arc::new(MemorySshStore::default()),
            Arc::new(MemoryConnectedStore::default()),
            Arc::new(|_, _| Ok(())),
            Arc::new(move |s| sink.lock().unwrap().push(s.clone())),
            PathBuf::from("/nonexistent"),
        );
        assert_eq!(core.state().old_app, None);
        let old = OldAppView {
            paths: vec!["/Applications/Instabox.app".into()],
        };
        core.set_old_app(Some(old.clone()));
        assert_eq!(core.state().old_app, Some(old.clone()));
        assert_eq!(emitted.lock().unwrap().last().unwrap().old_app, Some(old));
        // Moved to the Trash, or Not now.
        core.set_old_app(None);
        assert_eq!(core.state().old_app, None);
        assert_eq!(emitted.lock().unwrap().last().unwrap().old_app, None);
    }

    #[test]
    fn settle_sets_and_clears_the_error() {
        let core = Core::new(
            api_base(None),
            Arc::new(MemoryKeyStore::default()),
            Arc::new(MemorySshStore::default()),
            Arc::new(MemoryConnectedStore::default()),
            Arc::new(|_, _| Ok(())),
            Arc::new(|_| {}),
            PathBuf::from("/nonexistent"),
        );
        assert!(core.settle::<()>(Err(ErrorView::other("boom"))).is_err());
        assert_eq!(core.state().error, Some(ErrorView::other("boom")));
        core.settle(Ok(())).unwrap();
        assert_eq!(core.state().error, None);
    }

    #[test]
    fn a_new_sign_in_code_clears_the_expired_one() {
        let core = core_on(
            Url::parse("http://127.0.0.1:9").unwrap(),
            Arc::new(MemoryConnectedStore::default()),
        );
        let expired = ErrorView::from_auth(&auth::Error::Expired);
        assert!(core.settle::<()>(Err(expired.clone())).is_err());
        assert_eq!(core.state().error, Some(expired));
        core.update(|i| i.show_sign_in_code("WXYZ-1234".into()));
        let s = core.state();
        assert_eq!(s.sign_in_code.as_deref(), Some("WXYZ-1234"));
        assert_eq!(s.error, None);
    }

    #[tokio::test]
    async fn sign_out_during_a_refresh_keeps_the_store_empty_and_logs_out() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        let rotated = Tokens {
            access_token: "iba_new".into(),
            access_expires_at: Utc::now() + chrono::Duration::hours(1),
            refresh_token: "ibr_new".into(),
            refresh_expires_at: Utc::now() + chrono::Duration::days(30),
        };
        Mock::given(method("POST"))
            .and(path("/v1/auth/refresh"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::to_value(&rotated).unwrap())
                    .set_delay(Duration::from_millis(800)),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/logout"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        let mut tok = tokens();
        tok.access_expires_at = Utc::now() + chrono::Duration::seconds(5);
        let (core, store, _keys, _ssh, _) = core_with_session(&server.uri(), tok);
        // An API call already in flight (a poll), holding the old session: it refreshes.
        let s = core.lock().session.clone().unwrap();
        let call = tokio::spawn(async move { s.bearer().await });
        tokio::time::sleep(Duration::from_millis(200)).await;
        core.sign_out().await.unwrap();
        assert_eq!(store.load().unwrap(), None, "cleared right after sign_out");
        let got = call.await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        let reqs = server.received_requests().await.unwrap_or_default();
        let logouts = reqs
            .iter()
            .filter(|r| r.url.path() == "/v1/auth/logout")
            .count();
        assert!(
            got.is_err(),
            "the late refresh reports a signed-out session"
        );
        assert_eq!(logouts, 1, "the logout was sent with the token it had");
        assert_eq!(
            store.load().unwrap(),
            None,
            "rotated tokens saved back after sign_out"
        );
    }

    struct FailingConnected;
    impl ConnectedStore for FailingConnected {
        fn load(&self, _: i64) -> std::collections::HashSet<String> {
            Default::default()
        }
        fn add(&self, _: i64, _: &str, _: &dyn Gate) -> Result<(), String> {
            Err("disk full".into())
        }
        fn remove(&self, _: i64, _: &str, _: &dyn Gate) -> Result<(), String> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn a_failed_connected_list_save_stays_visible_after_connect() {
        let core = core_with(Arc::new(FailingConnected));
        core.update(|i| {
            i.data.machines = vec![machine("t", "org-me", "agent-browser", "running", true)];
            i.data.me = Some(me());
            i.data.orgs = data().orgs;
            i.rebuild();
        });
        core.finish_connect(
            Ade::T3,
            Ok(connected_result("t", "0.1.11")),
            "0.1.11",
            core.account(),
        )
        .await
        .unwrap();
        let e = core.state().error.expect("error shown");
        assert!(e.message.contains("disk full"), "{e:?}");
    }

    fn machine_json() -> serde_json::Value {
        serde_json::json!({
            "id": "m1", "name": "n", "state": "running", "revision": 1,
            "template": {"id": "agent-browser", "version": 1, "digest": "d"},
            "spec": {"vcpus": 2, "memMiB": 4096, "persistentDiskGiB": 10},
            "runtime": {"ready": true}, "autoSuspendAfterSeconds": 1800,
            "maxRunMinutes": -1, "runStartedAt": null, "cooldownUntil": null,
            "metadata": {}, "orgId": "o1", "free": false, "operationId": null
        })
    }

    /// A mock API for one running sandbox `m1` that can pair; `delay` slows the pairing.
    async fn pairing_server(delay: Duration) -> (wiremock::MockServer, Url) {
        use wiremock::matchers::{body_string_contains, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/machines/m1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(machine_json()))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/machines"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"machines": [], "nextCursor": null})),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/machines/m1/exec"))
            .and(body_string_contains("t3 pair"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!(
                        {"stdout": "Token: abc123\n", "stderr": "", "exitCode": 0}
                    ))
                    .set_delay(delay),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/machines/m1/ports"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!(
                {"id": "p1", "port": 3774, "url": "https://m1.example.test", "access": "public"}
            )))
            .mount(&server)
            .await;
        let base = Url::parse(&server.uri()).unwrap();
        (server, base)
    }

    #[tokio::test]
    async fn a_failed_connected_list_save_stays_visible_after_pair_again() {
        let (_server, base) = pairing_server(Duration::ZERO).await;
        let core = core_on(base.clone(), Arc::new(FailingConnected));
        sign_in_as(&core, &base, 7);
        core.pair_again("m1", None).await.unwrap();
        let e = core.state().error.expect("error shown");
        assert!(e.message.contains("disk full"), "{e:?}");
    }

    #[tokio::test]
    async fn pair_again_in_flight_during_sign_out_remembers_nothing() {
        let (_server, base) = pairing_server(Duration::from_millis(600)).await;
        let store = Arc::new(MemoryConnectedStore::default());
        let core = core_on(base.clone(), store.clone());
        sign_in_as(&core, &base, 7);
        let c = core.clone();
        let call = tokio::spawn(async move { c.pair_again("m1", None).await });
        tokio::time::sleep(Duration::from_millis(200)).await;
        core.sign_out().await.unwrap();
        let _ = call.await.unwrap();
        // Another account signs in meanwhile.
        sign_in_as(&core, &base, 8);
        assert!(store.load(7).is_empty() && store.load(8).is_empty());
        assert!(core.lock().data.connected.is_empty());
    }

    #[tokio::test]
    async fn a_connect_result_after_sign_out_is_dropped() {
        let store = Arc::new(MemoryConnectedStore::default());
        let core = core_with(store.clone());
        core.update(|i| i.data.me = Some(me()));
        let started = core.account();
        core.sign_out().await.unwrap();
        // The same user signs in again: still a new account count.
        let base = Url::parse("http://127.0.0.1:9").unwrap();
        sign_in_as(&core, &base, 7);
        let before = core.state();
        core.finish_connect(
            Ade::T3,
            Ok(connected_result("t", "0.1.11")),
            "0.1.11",
            started,
        )
        .await
        .unwrap();
        assert!(store.load(7).is_empty(), "nothing remembered");
        assert!(core.lock().data.connected.is_empty());
        assert_eq!(core.state().error, before.error);
    }

    #[tokio::test]
    async fn a_refresh_started_for_one_account_does_not_fill_the_next() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/machines"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({
                        "machines": [machine_json()], "nextCursor": null
                    }))
                    .set_delay(Duration::from_millis(600)),
            )
            .mount(&server)
            .await;
        let base = Url::parse(&server.uri()).unwrap();
        let core = core_on(base.clone(), Arc::new(MemoryConnectedStore::default()));
        sign_in_as(&core, &base, 7);
        let c = core.clone();
        let call = tokio::spawn(async move { c.refresh(false).await });
        tokio::time::sleep(Duration::from_millis(200)).await;
        core.sign_out().await.unwrap();
        sign_in_as(&core, &base, 8);
        call.await.unwrap();
        assert!(core.lock().data.machines.is_empty(), "A's machines dropped");
        assert_ne!(core.lock().data.connected_user, Some(7));
    }

    #[tokio::test]
    async fn an_unreadable_connected_list_is_reported_once_and_not_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("connected.json");
        std::fs::write(&path, "{not json").unwrap();
        let (_server, base) = listing_server().await;
        let core = core_on(
            base.clone(),
            Arc::new(FileConnectedStore::new(path.clone())),
        );
        sign_in_as(&core, &base, 7);
        core.refresh(false).await;
        let e = core.state().error.expect("error shown");
        assert!(
            e.message
                .contains("Could not read the list of connected sandboxes"),
            "{e:?}"
        );
        assert!(core.lock().data.connected.is_empty());
        assert_eq!(std::fs::read(&path).unwrap(), b"{not json");
    }

    #[tokio::test]
    async fn a_pair_again_that_fails_after_sign_out_leaves_no_error_for_the_next_account() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/machines/m1"))
            .respond_with(
                ResponseTemplate::new(500)
                    .set_body_json(serde_json::json!({"error": {
                        "code": "boom", "message": "failed", "retryable": false}}))
                    .set_delay(Duration::from_millis(500)),
            )
            .mount(&server)
            .await;
        let base = Url::parse(&server.uri()).unwrap();
        let core = core_on(base.clone(), Arc::new(MemoryConnectedStore::default()));
        sign_in_as(&core, &base, 7);
        let c = core.clone();
        let call = tokio::spawn(async move { c.pair_again("m1", None).await });
        tokio::time::sleep(Duration::from_millis(150)).await;
        core.sign_out().await.unwrap();
        sign_in_as(&core, &base, 8);
        assert!(call.await.unwrap().is_err(), "the caller still sees it");
        assert_eq!(core.state().error, None, "the new account shows no error");
    }

    #[tokio::test]
    async fn a_delete_finishing_after_an_account_switch_leaves_the_new_list_alone() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/v1/machines/m1"))
            .respond_with(
                ResponseTemplate::new(202)
                    .set_body_json(serde_json::json!({
                        "id": "op1", "machineId": "m1", "type": "delete",
                        "state": "succeeded", "error": null, "machine": null
                    }))
                    .set_delay(Duration::from_millis(500)),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/machines"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"machines": [], "nextCursor": null})),
            )
            .mount(&server)
            .await;
        let base = Url::parse(&server.uri()).unwrap();
        let store = Arc::new(MemoryConnectedStore::default());
        store.add(8, "m1", &crate::gate::Open).unwrap();
        let core = core_on(base.clone(), store.clone());
        sign_in_as(&core, &base, 7);
        let c = core.clone();
        let call = tokio::spawn(async move { c.delete("m1").await });
        tokio::time::sleep(Duration::from_millis(150)).await;
        core.sign_out().await.unwrap();
        sign_in_as(&core, &base, 8);
        core.update(|i| {
            i.data.connected_user = Some(8);
            i.data.connected.insert("m1".into());
        });
        let _ = call.await.unwrap();
        assert!(store.load(8).contains("m1"), "user 8's list is untouched");
        assert!(core.lock().data.connected.contains("m1"));
    }

    #[tokio::test]
    async fn progress_from_a_flow_of_an_old_account_is_dropped() {
        let base = Url::parse("http://127.0.0.1:9").unwrap();
        let core = core_on(base.clone(), Arc::new(MemoryConnectedStore::default()));
        sign_in_as(&core, &base, 7);
        let old = core.account();
        let stale = core.progress_cb(old);
        core.sign_out().await.unwrap();
        sign_in_as(&core, &base, 8);
        // The old flow starts and moves its progress after the switch: nothing shows.
        core.begin_progress("create", "creating", None, Some(Ade::T3), old);
        stale(Step::Pairing);
        assert_eq!(core.state().progress, None);
        // The new account's own progress is not moved by the old flow's callback.
        core.begin_progress(
            "connect",
            "connecting",
            Some("m2".into()),
            Some(Ade::T3),
            core.account(),
        );
        let before = core.state().progress;
        stale(Step::Pairing);
        assert_eq!(core.state().progress, before);
    }

    /// What a recording opener writes down: the URL, and the app it had to open it in.
    fn opened_as(url: &str, app: Option<&Path>) -> String {
        match app {
            Some(a) => format!("{url} in {}", a.display()),
            None => url.to_string(),
        }
    }

    /// A core on `base` with a recording opener and the given stores and home, not yet shared.
    /// The opener also writes down each login opened in a terminal, as
    /// `terminal: <ssh client> <arguments>`, so no test opens a real one.
    fn recording_core(
        base: Url,
        keys: Arc<MemoryKeyStore>,
        ssh: Arc<dyn SshTokenStore>,
        home: PathBuf,
    ) -> (Core, Arc<Mutex<Vec<String>>>) {
        let opened = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = opened.clone();
        let terminal = opened.clone();
        let core = Core::new(
            base,
            keys,
            ssh,
            Arc::new(MemoryConnectedStore::default()),
            Arc::new(move |u: &str, app: Option<&Path>| {
                sink.lock().unwrap().push(opened_as(u, app));
                Ok(())
            }),
            Arc::new(|_| {}),
            home,
        )
        .with_terminal(Arc::new(move |ssh: &Path, args: &[String]| {
            let line = format!("terminal: {} {}", ssh.display(), args.join(" "));
            terminal.lock().unwrap().push(line);
            Ok(())
        }));
        (core, opened)
    }

    /// A core on `base` with a recording opener and the given stores and home.
    fn core_full(
        base: Url,
        keys: Arc<MemoryKeyStore>,
        ssh: Arc<MemorySshStore>,
        home: PathBuf,
    ) -> (Arc<Core>, Arc<Mutex<Vec<String>>>) {
        let (core, opened) = recording_core(base, keys, ssh, home);
        (Arc::new(core), opened)
    }

    #[tokio::test]
    async fn pair_again_after_an_account_switch_opens_nothing() {
        let (_server, base) = pairing_server(Duration::from_millis(600)).await;
        let (core, opened) = core_full(
            base.clone(),
            Arc::new(MemoryKeyStore::default()),
            Arc::new(MemorySshStore::default()),
            PathBuf::from("/nonexistent"),
        );
        sign_in_as(&core, &base, 7);
        let c = core.clone();
        let call = tokio::spawn(async move { c.pair_again("m1", None).await });
        tokio::time::sleep(Duration::from_millis(200)).await;
        core.sign_out().await.unwrap();
        sign_in_as(&core, &base, 8);
        let _ = call.await.unwrap();
        assert!(opened.lock().unwrap().is_empty(), "no pairing link opened");
    }

    /// The reply to a new SSH token `token` of `m1`, a background one when `background`.
    fn minted(token: &str, background: bool) -> serde_json::Value {
        let mut t = serde_json::json!({
            "id": "k1", "name": "Instabox", "host": "ssh.example.test", "port": 2222,
            "token": token, "command": format!("ssh -p 2222 {token}@ssh.example.test"),
            "expiresAt": Utc::now() + chrono::Duration::days(30)
        });
        if background {
            t["background"] = true.into();
            t["backgroundActive"] = true.into();
        }
        t
    }

    /// A mock API with one running sandbox `m1` named "dev 1" with `metadata`. It mints SSH
    /// tokens (`ibssh_new`, a background one when asked, as a server with background tokens
    /// does), answers every exec with success, takes a metadata patch and creates sandboxes.
    async fn cmux_server(metadata: serde_json::Value) -> (wiremock::MockServer, Url) {
        use wiremock::matchers::{body_partial_json, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        let m1 = serde_json::json!({
            "id": "m1", "name": "dev 1", "state": "running",
            "template": {"id": "agent-browser"}, "spec": {"vcpus": 2, "memMiB": 4096},
            "metadata": metadata, "orgId": "org-me"
        });
        // Ahead of the ordinary token below, behind a test's own answers (priority 1).
        Mock::given(method("POST"))
            .and(path("/v1/machines/m1/ssh-tokens"))
            .and(body_partial_json(serde_json::json!({"background": true})))
            .respond_with(ResponseTemplate::new(200).set_body_json(minted("ibssh_new", true)))
            .with_priority(3)
            .mount(&server)
            .await;
        let answers = [
            (
                "GET",
                "/v1/machines",
                serde_json::json!({"machines": [m1], "nextCursor": null}),
            ),
            ("GET", "/v1/machines/m1", m1.clone()),
            ("PATCH", "/v1/machines/m1", m1.clone()),
            (
                "POST",
                "/v1/machines/m1/ssh-tokens",
                minted("ibssh_new", false),
            ),
            (
                "POST",
                "/v1/machines/m1/exec",
                serde_json::json!({"stdout": "", "stderr": "", "exitCode": 0}),
            ),
            (
                "GET",
                "/v1/me",
                serde_json::json!({"userId": 7, "login": "alex",
                    "org": {"id": "org-me", "tier": "pro", "kind": "personal"}}),
            ),
            (
                "POST",
                "/v1/machines",
                serde_json::json!({"id": "op1", "machineId": "m1", "type": "create",
                    "state": "queued", "machine": null}),
            ),
            (
                "GET",
                "/v1/operations/op1",
                serde_json::json!({"id": "op1", "machineId": "m1", "type": "create",
                    "state": "succeeded", "machine": m1}),
            ),
        ];
        for (verb, p, body) in answers {
            Mock::given(method(verb))
                .and(path(p))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&server)
                .await;
        }
        let base = Url::parse(&server.uri()).unwrap();
        (server, base)
    }

    async fn requests(server: &wiremock::MockServer) -> Vec<String> {
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|r| format!("{} {}", r.method, r.url.path()))
            .collect()
    }

    const CMUX_LINK: &str = "cmux://ssh?host=ssh.example.test&user=ibssh_new&port=2222&title=dev%201&host-key-policy=accept-new";

    /// The cmux.app the test computer has.
    #[derive(Clone, Copy, PartialEq)]
    enum CmuxHere {
        /// cmux's identifier and a signature that passes.
        Real,
        /// A bundle named cmux.app with another identifier.
        OtherId,
        /// cmux's identifier, but the signature check fails.
        BadSignature,
        Missing,
    }

    /// A signed-in core with a recording opener and a cmux.app as `here` says. The app is a
    /// real bundle on disk, checked by the production identifier check; only codesign is
    /// replaced by `here`.
    struct CmuxCore {
        core: Arc<Core>,
        opened: Arc<Mutex<Vec<String>>>,
        app: PathBuf,
        _dir: tempfile::TempDir,
    }

    fn cmux_core(base: &Url, ssh: Arc<MemorySshStore>, here: CmuxHere) -> CmuxCore {
        let dir = tempfile::tempdir().unwrap();
        let id = if here == CmuxHere::OtherId {
            "com.example.cmux"
        } else {
            ade::cmux::BUNDLE_ID
        };
        let app = ade::cmux::test_bundle(dir.path(), id, "0.64.25", false);
        let (core, opened) = recording_core(
            base.clone(),
            Arc::new(MemoryKeyStore::default()),
            ssh,
            PathBuf::from("/nonexistent"),
        );
        let found = app.clone();
        let core = Arc::new(core.with_apps(
            Arc::new(move |ade| {
                (here != CmuxHere::Missing && ade == Ade::Cmux).then(|| found.clone())
            }),
            Arc::new(move |ade, path| {
                ade == Ade::Cmux && ade::cmux::verify_with(path, |_| here != CmuxHere::BadSignature)
            }),
        ));
        sign_in_as(&core, base, 7);
        CmuxCore {
            core,
            opened,
            app,
            _dir: dir,
        }
    }

    /// The cmux link as the opener got it: always with the cmux app named.
    fn cmux_opened(app: &Path) -> String {
        format!("{CMUX_LINK} in {}", app.display())
    }

    #[tokio::test]
    async fn open_in_cmux_opens_its_link_in_the_cmux_app_only() {
        let (server, base) =
            cmux_server(serde_json::json!({"instabox-bridge": "t3", "instabox-ades": "t3,cmux"}))
                .await;
        let ssh = Arc::new(MemorySshStore::default());
        let CmuxCore {
            core,
            opened,
            app,
            _dir,
        } = cmux_core(&base, ssh.clone(), CmuxHere::Real);
        core.refresh(false).await;
        core.open_in_ade("m1", Some(Ade::Cmux)).await.unwrap();
        // Never the default handler of cmux:// links: the link holds the SSH token.
        assert_eq!(*opened.lock().unwrap(), [cmux_opened(&app)]);
        let r = requests(&server).await;
        assert!(!r.iter().any(|x| x.ends_with("/exec")), "{r:?}");
        assert!(!r.iter().any(|x| x.starts_with("PATCH")), "{r:?}");
        assert_eq!(
            r.iter().filter(|x| x.ends_with("/ssh-tokens")).count(),
            1,
            "{r:?}"
        );
        // The token is kept as the background one, so the next open reuses it.
        let kept = ssh.load("m1", Kind::Background).unwrap().unwrap();
        assert_eq!((kept.token.as_str(), kept.background), ("ibssh_new", true));
        assert_eq!(ssh.load("m1", Kind::Ordinary).unwrap(), None);
        core.open_in_ade("m1", Some(Ade::Cmux)).await.unwrap();
        assert_eq!(
            *opened.lock().unwrap(),
            [cmux_opened(&app), cmux_opened(&app)]
        );
        let r = requests(&server).await;
        assert_eq!(r.iter().filter(|x| x.ends_with("/ssh-tokens")).count(), 1);
    }

    /// Mounts a sandbox m1 in `state` for the next read of it, before the ones `cmux_server`
    /// answers (running).
    async fn m1_once_in(server: &wiremock::MockServer, md: &serde_json::Value, state: &str) {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};
        Mock::given(method("GET"))
            .and(path("/v1/machines/m1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "m1", "name": "dev 1", "state": state,
                "template": {"id": "agent-browser"}, "spec": {"vcpus": 2, "memMiB": 4096},
                "metadata": md, "orgId": "org-me"
            })))
            .up_to_n_times(1)
            .with_priority(1)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn open_in_cmux_wakes_a_paused_or_stopped_sandbox_before_it_opens_the_link() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};
        for (state, action) in [("suspended", "resume"), ("stopped", "start")] {
            let md = serde_json::json!({"instabox-bridge": "cmux", "instabox-ades": "cmux"});
            let (server, base) = cmux_server(md.clone()).await;
            // The SSH gateway refuses a paused sandbox, and cmux's link does not wake it.
            m1_once_in(&server, &md, state).await;
            Mock::given(method("POST"))
                .and(path(format!("/v1/machines/m1/{action}")))
                .respond_with(ResponseTemplate::new(202).set_body_json(serde_json::json!({
                    "id": "op1", "machineId": "m1", "type": action, "state": "queued",
                    "machine": null
                })))
                .mount(&server)
                .await;
            let CmuxCore {
                core,
                opened,
                app,
                _dir,
            } = cmux_core(&base, Arc::new(MemorySshStore::default()), CmuxHere::Real);
            core.refresh(false).await;
            assert_eq!(core.open_in_ade("m1", Some(Ade::Cmux)).await, Ok(None));
            assert_eq!(*opened.lock().unwrap(), [cmux_opened(&app)], "{state}");
            assert_eq!(core.state().error, None);
            let r = requests(&server).await;
            let at = |p: &str| {
                r.iter()
                    .position(|x| x == p)
                    .unwrap_or_else(|| panic!("{p}: {r:?}"))
            };
            assert!(
                at(&format!("POST /v1/machines/m1/{action}"))
                    < at("POST /v1/machines/m1/ssh-tokens"),
                "{r:?}"
            );
            assert!(!r.iter().any(|x| x.ends_with("/exec")), "{r:?}");
        }
    }

    #[tokio::test]
    async fn a_cmux_sandbox_that_does_not_wake_shows_the_error_and_opens_nothing() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};
        let md = serde_json::json!({"instabox-bridge": "cmux", "instabox-ades": "cmux"});
        let (server, base) = cmux_server(md.clone()).await;
        m1_once_in(&server, &md, "suspended").await;
        Mock::given(method("POST"))
            .and(path("/v1/machines/m1/resume"))
            .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
                "error": {"code": "forbidden", "message": "This sandbox cannot be resumed.",
                    "retryable": false}
            })))
            .mount(&server)
            .await;
        let CmuxCore {
            core,
            opened,
            app: _,
            _dir,
        } = cmux_core(&base, Arc::new(MemorySshStore::default()), CmuxHere::Real);
        core.refresh(false).await;
        let e = core.open_in_ade("m1", Some(Ade::Cmux)).await.unwrap_err();
        assert_eq!(e.message, "This sandbox cannot be resumed.");
        assert_eq!(core.state().error, Some(e), "the app shows it");
        assert!(opened.lock().unwrap().is_empty(), "nothing opened");
        let r = requests(&server).await;
        assert!(!r.iter().any(|x| x.ends_with("/ssh-tokens")), "{r:?}");
    }

    #[tokio::test]
    async fn a_cmux_link_never_opens_without_the_cmux_app() {
        let (server, base) =
            cmux_server(serde_json::json!({"instabox-bridge": "cmux", "instabox-ades": "cmux"}))
                .await;
        let CmuxCore {
            core,
            opened,
            app: _,
            _dir,
        } = cmux_core(
            &base,
            Arc::new(MemorySshStore::default()),
            CmuxHere::Missing,
        );
        core.refresh(false).await;
        let e = core.open_in_ade("m1", Some(Ade::Cmux)).await.unwrap_err();
        assert_eq!(e.message, "cmux is not installed. Install it first.");
        assert_eq!(core.state().error, Some(e));
        assert!(opened.lock().unwrap().is_empty(), "nothing opened");
        let r = requests(&server).await;
        assert!(
            !r.iter().any(|x| x.ends_with("/ssh-tokens")),
            "no token: {r:?}"
        );
        // And the UI cannot open one in the default handler either.
        assert!(core.open_url(CMUX_LINK).is_err());
        assert!(opened.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_cmux_link_never_goes_to_a_lookalike_or_an_unverified_app() {
        for here in [CmuxHere::OtherId, CmuxHere::BadSignature] {
            let (server, base) = cmux_server(
                serde_json::json!({"instabox-bridge": "cmux", "instabox-ades": "cmux"}),
            )
            .await;
            let ssh = Arc::new(MemorySshStore::default());
            // Even a stored token is not loaded for an app that fails the check.
            ssh.save("m1", Kind::Background, &ssh_token()).unwrap();
            let CmuxCore {
                core,
                opened,
                app: _,
                _dir,
            } = cmux_core(&base, ssh, here);
            core.refresh(false).await;
            let e = core.open_in_ade("m1", Some(Ade::Cmux)).await.unwrap_err();
            assert_eq!(
                e.message,
                "Could not verify the cmux app. Install it again from its official download."
            );
            assert!(opened.lock().unwrap().is_empty(), "nothing opened");
            let r = requests(&server).await;
            assert!(
                !r.iter().any(|x| x.ends_with("/ssh-tokens")),
                "no token: {r:?}"
            );
        }
    }

    #[tokio::test]
    async fn open_in_cmux_of_a_sandbox_without_it_runs_the_prep_marks_then_opens() {
        let (server, base) = cmux_server(serde_json::json!({"instabox-bridge": "t3"})).await;
        let CmuxCore {
            core,
            opened,
            app,
            _dir,
        } = cmux_core(&base, Arc::new(MemorySshStore::default()), CmuxHere::Real);
        core.update(|i| i.state.set_desktop(Ade::Cmux, Some("0.64.25".into())));
        core.refresh(false).await;
        core.open_in_ade("m1", Some(Ade::Cmux)).await.unwrap();
        assert_eq!(*opened.lock().unwrap(), [cmux_opened(&app)]);
        let reqs = server.received_requests().await.unwrap();
        let execs: Vec<String> = reqs
            .iter()
            .filter(|r| r.url.path().ends_with("/exec"))
            .map(|r| String::from_utf8_lossy(&r.body).to_string())
            .collect();
        assert_eq!(execs.len(), 1, "only the prep");
        assert!(execs[0].contains("chmod go-w ~/.local ~/.local/state"));
        let patch = reqs.iter().find(|r| r.method.as_str() == "PATCH").unwrap();
        let body: serde_json::Value = serde_json::from_slice(&patch.body).unwrap();
        assert_eq!(body["metadata"]["instabox-bridge"], "t3");
        assert_eq!(body["metadata"]["instabox-ades"], "t3,cmux");
        let s = core.state();
        assert_eq!(s.progress, None);
        assert_eq!(s.error, None);
        // cmux pairs nothing, so the sandbox does not join the set of T3 pairings from here.
        assert!(!core.lock().data.connected.contains("m1"));
    }

    #[tokio::test]
    async fn adding_cmux_here_to_a_t3_sandbox_from_elsewhere_still_needs_t3_pairing() {
        // Another computer set T3 up and marked the sandbox. This one adds cmux.
        let (server, base) = cmux_server(serde_json::json!({"instabox-bridge": "t3"})).await;
        let CmuxCore {
            core,
            opened,
            app: _,
            _dir,
        } = cmux_core(&base, Arc::new(MemorySshStore::default()), CmuxHere::Real);
        core.update(|i| {
            i.state.set_desktop(Ade::Cmux, Some("0.64.25".into()));
            i.state.set_desktop(Ade::T3, Some("0.1.11".into()));
        });
        core.refresh(false).await;
        core.open_in_ade("m1", Some(Ade::Cmux)).await.unwrap();
        assert!(!core.lock().data.connected.contains("m1"));
        assert!(!core.state().sandboxes[0].connected);

        // Open in T3 Code must pair first, not open T3 Code as if this computer had paired.
        let _ = core.open_in_ade("m1", Some(Ade::T3)).await;
        assert!(
            !opened
                .lock()
                .unwrap()
                .iter()
                .any(|o| o.starts_with("t3code://")),
            "{:?}",
            opened.lock().unwrap()
        );
        let reqs = server.received_requests().await.unwrap();
        assert!(
            reqs.iter().any(|r| r.url.path().ends_with("/exec")
                && String::from_utf8_lossy(&r.body).contains("t3 pair")),
            "the pairing ran"
        );
    }

    #[tokio::test]
    async fn connecting_cmux_needs_the_app_on_this_computer() {
        let (_server, base) = cmux_server(serde_json::json!({})).await;
        let (core, opened) = core_full(
            base.clone(),
            Arc::new(MemoryKeyStore::default()),
            Arc::new(MemorySshStore::default()),
            PathBuf::from("/nonexistent"),
        );
        sign_in_as(&core, &base, 7);
        // Found as missing (Windows never looks): connecting asks for the app first.
        core.update(|i| i.state.set_desktop(Ade::Cmux, None));
        let detect_finds_nothing = !cfg!(target_os = "macos") || Ade::Cmux.locate().is_none();
        if detect_finds_nothing {
            let e = core
                .connect_sandbox("m1", Some(Ade::Cmux))
                .await
                .unwrap_err();
            assert_eq!(e.message, "cmux is not installed. Install it first.");
            assert!(opened.lock().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn a_plain_create_sets_no_ade_up_and_opens_nothing() {
        let (server, base) = cmux_server(serde_json::json!({})).await;
        let (core, opened) = core_full(
            base.clone(),
            Arc::new(MemoryKeyStore::default()),
            Arc::new(MemorySshStore::default()),
            PathBuf::from("/nonexistent"),
        );
        sign_in_as(&core, &base, 7);
        core.create_sandbox(CreateArgs {
            name: "dev 1".into(),
            snapshot_id: None,
            vcpus: 2,
            mem_mib: 4096,
            auto_suspend_after_seconds: Some(1800),
            ade: None,
        })
        .await
        .unwrap();
        let r = requests(&server).await;
        assert!(r.contains(&"POST /v1/machines".to_string()), "{r:?}");
        assert!(
            !r.iter().any(|x| x.ends_with("/exec")
                || x.starts_with("PATCH")
                || x.ends_with("/ssh-tokens")),
            "{r:?}"
        );
        assert!(opened.lock().unwrap().is_empty());
        let s = core.state();
        assert_eq!((s.progress, s.error), (None, None));
        assert!(core.lock().data.connected.is_empty(), "not connected");
        // After the refresh the create ran, it is a sandbox with no ADE, not a T3 one.
        let v = &core.state().sandboxes[0];
        assert_eq!((v.id.as_str(), v.ades.clone()), ("m1", vec![]));
    }

    #[tokio::test]
    async fn a_plain_sandbox_has_no_ade_and_a_v0_1_one_has_t3_after_a_refresh() {
        // The API sends an empty object for a sandbox with no metadata.
        for (metadata, has) in [
            (serde_json::json!({}), vec![]),
            (serde_json::json!({"instabox-bridge": "t3"}), vec![Ade::T3]),
        ] {
            let (_server, base) = cmux_server(metadata.clone()).await;
            let (core, _) = core_full(
                base.clone(),
                Arc::new(MemoryKeyStore::default()),
                Arc::new(MemorySshStore::default()),
                PathBuf::from("/nonexistent"),
            );
            sign_in_as(&core, &base, 7);
            core.refresh(false).await;
            let j = serde_json::to_value(&core.state().sandboxes[0]).unwrap();
            assert_eq!(j["ades"], serde_json::json!(has), "{metadata}");
            assert_eq!(core.lock().data.ades_of("m1"), has);
            // An action with no ADE asked still picks T3 for either.
            assert_eq!(core.lock().data.ade_of("m1"), Some(Ade::T3));
        }
    }

    #[tokio::test]
    async fn open_in_t3_of_a_sandbox_with_no_mark_sets_t3_up_first() {
        // Even when it is in the connected set, a sandbox without the T3 mark has no T3.
        let (server, base) = cmux_server(serde_json::json!({})).await;
        let (core, opened) = core_full(
            base.clone(),
            Arc::new(MemoryKeyStore::default()),
            Arc::new(MemorySshStore::default()),
            PathBuf::from("/nonexistent"),
        );
        sign_in_as(&core, &base, 7);
        core.update(|i| {
            i.state.set_desktop(Ade::T3, Some("0.1.11".into()));
            i.data.connected.insert("m1".into());
        });
        core.refresh(false).await;
        let _ = core.open_in_ade("m1", Some(Ade::T3)).await;
        assert!(
            !opened
                .lock()
                .unwrap()
                .iter()
                .any(|o| o.starts_with("t3code://")),
            "{:?}",
            opened.lock().unwrap()
        );
        let reqs = server.received_requests().await.unwrap();
        let execs: Vec<String> = reqs
            .iter()
            .filter(|r| r.url.path().ends_with("/exec"))
            .map(|r| String::from_utf8_lossy(&r.body).to_string())
            .collect();
        assert!(execs.iter().any(|e| e.contains("v=0.1.11")), "T3 setup ran");
    }

    #[tokio::test]
    async fn open_in_t3_of_a_connected_sandbox_pairs_again_on_every_open() {
        use wiremock::matchers::{body_string_contains, method, path};
        use wiremock::{Mock, ResponseTemplate};
        // T3 is set up from here, at the desktop's version. T3 Code may have lost the sandbox
        // since, so each Open pairs again instead of only bringing T3 Code to the front.
        let (server, base) =
            cmux_server(serde_json::json!({"instabox-bridge": "t3", "instabox-ades": "t3"})).await;
        let version = format!("t3 v0.1.11\n{}\n", crate::ade::t3::setup_line());
        for (cmd, stdout) in [
            ("t3 --version", version.as_str()),
            ("t3 pair", "Token: abc123\n"),
        ] {
            Mock::given(method("POST"))
                .and(path("/v1/machines/m1/exec"))
                .and(body_string_contains(cmd))
                .respond_with(ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({"stdout": stdout, "stderr": "", "exitCode": 0}),
                ))
                .with_priority(1)
                .mount(&server)
                .await;
        }
        Mock::given(method("POST"))
            .and(path("/v1/machines/m1/ports"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!(
                {"id": "p1", "port": 3774, "url": "https://3774-m1.example.test", "access": "public"}
            )))
            .mount(&server)
            .await;
        let (core, opened) = core_full(
            base.clone(),
            Arc::new(MemoryKeyStore::default()),
            Arc::new(MemorySshStore::default()),
            PathBuf::from("/nonexistent"),
        );
        sign_in_as(&core, &base, 7);
        core.update(|i| {
            i.state.set_desktop(Ade::T3, Some("0.1.11".into()));
            i.data.connected.insert("m1".into());
        });
        core.refresh(false).await;
        assert!(core.state().sandboxes[0].connected);

        // Open in the popover, then Open in the tray, which names no ADE.
        assert_eq!(core.open_in_ade("m1", Some(Ade::T3)).await, Ok(None));
        crate::tray::run_action(&core, "open", "m1", |_| {}).await;
        let link =
            "t3code://pair?pairingUrl=https%3A%2F%2F3774-m1.example.test%2Fpair%23token%3Dabc123";
        assert_eq!(*opened.lock().unwrap(), [link, link]);
        assert_eq!(core.state().error, None);
        let reqs = server.received_requests().await.unwrap();
        let execs: Vec<String> = reqs
            .iter()
            .filter(|r| r.url.path().ends_with("/exec"))
            .map(|r| String::from_utf8_lossy(&r.body).to_string())
            .collect();
        let pairs = execs.iter().filter(|e| e.contains("t3 pair")).count();
        assert_eq!(pairs, 2, "a new code on each open");
        assert!(!execs.iter().any(|e| e.contains("v=0.1.11")), "no setup");
        assert!(
            !execs
                .iter()
                .any(|e| e.contains(r#""cmd":"set -eu\nprobe="#)),
            "no refresh of a current setup"
        );
        assert!(
            !reqs.iter().any(|r| r.method.as_str() == "PATCH"),
            "no mark change"
        );
    }

    /// Open in T3 of a sandbox connected from here, with T3 Code `desktop` on this computer,
    /// whose version script prints `found` (no current setup line) and whose setup refresh
    /// exits `code` with `stderr`. The core, what Open returned and opened, and the exec bodies.
    async fn open_after_refresh(
        desktop: &str,
        found: &str,
        code: i32,
        stderr: &str,
    ) -> (
        Arc<Core>,
        Res<Notice>,
        Vec<String>,
        Vec<String>,
        wiremock::MockServer,
    ) {
        use wiremock::matchers::{body_string_contains, method, path};
        use wiremock::{Mock, ResponseTemplate};
        let (server, base) =
            cmux_server(serde_json::json!({"instabox-bridge": "t3", "instabox-ades": "t3"})).await;
        // The refresh first: its comments name `t3 pair` too.
        for (priority, cmd, stdout, code, stderr) in [
            (1, "t3 --version", found, 0, ""),
            (1, r#""cmd":"set -eu\nprobe="#, "", code, stderr),
            (2, "t3 pair", "Token: abc123\n", 0, ""),
        ] {
            Mock::given(method("POST"))
                .and(path("/v1/machines/m1/exec"))
                .and(body_string_contains(cmd))
                .respond_with(ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({"stdout": stdout, "stderr": stderr, "exitCode": code}),
                ))
                .with_priority(priority)
                .mount(&server)
                .await;
        }
        Mock::given(method("POST"))
            .and(path("/v1/machines/m1/ports"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!(
                {"id": "p1", "port": 3774, "url": "https://3774-m1.example.test", "access": "public"}
            )))
            .mount(&server)
            .await;
        let (core, opened) = core_full(
            base.clone(),
            Arc::new(MemoryKeyStore::default()),
            Arc::new(MemorySshStore::default()),
            PathBuf::from("/nonexistent"),
        );
        sign_in_as(&core, &base, 7);
        core.update(|i| {
            i.state.set_desktop(Ade::T3, Some(desktop.into()));
            i.data.connected.insert("m1".into());
        });
        core.refresh(false).await;
        let r = core.open_in_ade("m1", Some(Ade::T3)).await;
        let execs = server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|r| r.url.path().ends_with("/exec"))
            .map(|r| String::from_utf8_lossy(&r.body).to_string())
            .collect();
        let opened = opened.lock().unwrap().clone();
        (core, r, opened, execs, server)
    }

    fn paired(execs: &[String]) -> bool {
        execs.iter().any(|e| e.contains(r#""cmd":"t3 pair"#))
    }

    const BUSY: &str = "T3 is busy on this sandbox. Try again when the agent is idle.";

    #[tokio::test]
    async fn open_does_not_pair_a_busy_t3_at_the_desktop_version_whose_old_setup_refuses_dpop() {
        let (core, r, opened, execs, _server) =
            open_after_refresh("0.1.12", "t3 v0.1.12\n", 3, "T3 is busy, update deferred").await;
        let e = r.unwrap_err();
        assert_eq!(
            (e.kind, e.message.as_str()),
            (crate::errors::ErrorKind::AdeBusy, BUSY)
        );
        assert!(opened.is_empty() && !paired(&execs), "{opened:?}");
        // The sync is still recorded: Update is offered.
        assert!(core.state().sandboxes[0].update_pending);
        assert_eq!(core.state().error.map(|e| e.message).as_deref(), Some(BUSY));
    }

    #[tokio::test]
    async fn open_from_the_tray_does_not_pair_a_busy_newer_t3_whose_old_setup_refuses_dpop() {
        let (core, r, opened, execs, _server) = open_after_refresh(
            "0.1.12",
            "t3 v0.1.13-nightly.20261125.5258\n",
            3,
            "T3 is busy, update deferred",
        )
        .await;
        assert_eq!(r.unwrap_err().kind, crate::errors::ErrorKind::AdeBusy);
        assert!(opened.is_empty() && !paired(&execs));
        // The tray's Open, which names no ADE, shows the same error in the popover.
        crate::tray::run_action(&core, "open", "m1", |_| panic!("no notice")).await;
        assert_eq!(core.state().error.map(|e| e.message).as_deref(), Some(BUSY));
        assert!(
            !core.state().sandboxes[0].update_pending,
            "ahead: no Update"
        );
    }

    #[tokio::test]
    async fn open_does_not_pair_after_a_failed_refresh_of_an_old_setup_with_dpop() {
        let (_core, r, opened, execs, _server) =
            open_after_refresh("0.1.12", "t3 v0.1.12\n", 1, "T3 server did not start").await;
        let e = r.unwrap_err();
        assert_eq!(e.kind, crate::errors::ErrorKind::Setup);
        assert!(e.message.contains("T3 server did not start"), "{e:?}");
        assert!(opened.is_empty() && !paired(&execs));
    }

    #[tokio::test]
    async fn pair_again_follows_the_same_rule_as_open() {
        let pairs = |s: &[wiremock::Request]| {
            s.iter()
                .filter(|r| String::from_utf8_lossy(&r.body).contains(r#""cmd":"t3 pair"#))
                .count()
        };
        // Both sides use DPoP and the old setup cannot be refreshed: no link, the busy error.
        let (core, _, _, _, server) =
            open_after_refresh("0.1.12", "t3 v0.1.12\n", 3, "T3 is busy, update deferred").await;
        let e = core.pair_again("m1", None).await.unwrap_err();
        assert_eq!(
            (e.kind, e.message.as_str()),
            (crate::errors::ErrorKind::AdeBusy, BUSY)
        );
        assert_eq!(core.state().error.map(|e| e.message).as_deref(), Some(BUSY));
        assert_eq!(pairs(&server.received_requests().await.unwrap()), 0);
        // T3 Code 0.1.10 pairs with a bearer token: Pair again pairs as before.
        let (core, _, _, _, server) =
            open_after_refresh("0.1.10", "t3 v0.1.10\n", 3, "T3 is busy, update deferred").await;
        assert_eq!(core.pair_again("m1", None).await, Ok(()));
        assert_eq!(pairs(&server.received_requests().await.unwrap()), 2);
    }

    /// Pair again through the T3 proxy (`INSTAADE_T3_PROXY=1`, T3 Code 0.1.13) on a sandbox
    /// with T3 0.1.13 and an older setup, whose refresh exits `code` with `stderr`. The proxy's
    /// own pairing fails here (the sandbox's descriptor answers 404): what matters is whether
    /// the refresh came first. The core's result and the API requests, in order.
    async fn pair_again_through_the_proxy(
        code: i32,
        stderr: &str,
    ) -> (Arc<Core>, Res<()>, Vec<wiremock::Request>) {
        use wiremock::matchers::{body_string_contains, method, path};
        use wiremock::{Mock, ResponseTemplate};
        let (server, base) =
            cmux_server(serde_json::json!({"instabox-bridge": "t3", "instabox-ades": "t3"})).await;
        for (cmd, stdout, code, stderr) in [
            ("t3 --version", "t3 v0.1.13\n", 0, ""),
            (r#""cmd":"set -eu\nprobe="#, "", code, stderr),
        ] {
            Mock::given(method("POST"))
                .and(path("/v1/machines/m1/exec"))
                .and(body_string_contains(cmd))
                .respond_with(ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({"stdout": stdout, "stderr": stderr, "exitCode": code}),
                ))
                .with_priority(1)
                .mount(&server)
                .await;
        }
        Mock::given(method("POST"))
            .and(path("/v1/machines/m1/ports"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!(
                {"id": "p1", "port": 3774, "url": server.uri(), "access": "public"}
            )))
            .mount(&server)
            .await;
        let (core, _opened) = core_full(
            base.clone(),
            Arc::new(MemoryKeyStore::default()),
            Arc::new(MemorySshStore::default()),
            PathBuf::from("/nonexistent"),
        );
        sign_in_as(&core, &base, 7);
        core.update(|i| {
            i.state.set_desktop(Ade::T3, Some("0.1.13".into()));
            i.data.connected.insert("m1".into());
        });
        core.refresh(false).await;
        let dir = tempfile::tempdir().unwrap();
        let hub = t3proxy::Hub::new(
            dir.path().to_path_buf(),
            Arc::new(t3proxy::hub::Instabox::new(Arc::new(|| None::<Client>))),
            Arc::new(t3proxy::hub::MemoryTokenStore::default()),
            t3proxy::runtime::Tuning::default(),
            false,
        );
        core.set_t3_proxy(hub, t3proxy::Gate::Tested);
        let r = core.pair_again("m1", None).await;
        (core, r, server.received_requests().await.unwrap())
    }

    #[tokio::test]
    async fn pair_again_through_the_t3_proxy_brings_an_old_setup_up_to_date_first() {
        let refresh = |r: &wiremock::Request| {
            String::from_utf8_lossy(&r.body).contains(r#""cmd":"set -eu\nprobe="#)
        };
        let publish = |r: &wiremock::Request| r.url.path().ends_with("/ports");
        // T3 busy: the established busy error, and the proxy is not asked to pair.
        let (core, r, requests) =
            pair_again_through_the_proxy(3, "T3 is busy, update deferred").await;
        let e = r.unwrap_err();
        assert_eq!(
            (e.kind, e.message.as_str()),
            (crate::errors::ErrorKind::AdeBusy, BUSY)
        );
        assert_eq!(core.state().error.map(|e| e.message).as_deref(), Some(BUSY));
        assert!(requests.iter().any(refresh), "the refresh ran");
        assert!(!requests.iter().any(publish), "no pairing");
        // T3 idle: the setup is refreshed, then the proxy pairs.
        let (_core, r, requests) = pair_again_through_the_proxy(0, "").await;
        let refreshed = requests.iter().position(refresh).expect("the refresh ran");
        let published = requests.iter().position(publish).expect("the proxy paired");
        assert!(refreshed < published, "refreshed before the pairing");
        assert!(
            r.unwrap_err()
                .message
                .starts_with("The T3 proxy could not pair"),
            "the proxy's own pairing, against a fake descriptor"
        );
    }

    /// A signed-in core with T3 Code 0.1.13 and sandbox m1 marked with T3 at its current
    /// setup, `connected` from this computer or not, and a recording clipboard. With `proxy`,
    /// the T3 proxy is on (`INSTAADE_T3_PROXY=1`) with its state in `proxy`'s folder, and the
    /// mock server also answers as T3 on the sandbox, so the proxy pairs. The core, what it
    /// opened, what it copied and the server.
    async fn t3_switch_core(
        connected: bool,
        proxy: Option<&Path>,
    ) -> (
        Arc<Core>,
        Arc<Mutex<Vec<String>>>,
        Arc<Mutex<Vec<String>>>,
        wiremock::MockServer,
    ) {
        use wiremock::matchers::{body_string_contains, method, path};
        use wiremock::{Mock, ResponseTemplate};
        let (server, base) =
            cmux_server(serde_json::json!({"instabox-bridge": "t3", "instabox-ades": "t3"})).await;
        let version = format!("t3 v0.1.13\n{}\n", crate::ade::t3::setup_line());
        for (cmd, stdout) in [
            ("t3 --version", version.as_str()),
            ("t3 auth pairing create", r#"{"credential":"pc-1"}"#),
            ("t3 pair", "Token: abc123\n"),
        ] {
            Mock::given(method("POST"))
                .and(path("/v1/machines/m1/exec"))
                .and(body_string_contains(cmd))
                .respond_with(ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({"stdout": stdout, "stderr": "", "exitCode": 0}),
                ))
                .with_priority(1)
                .mount(&server)
                .await;
        }
        // T3 on the sandbox, at the published port: the same server.
        let answers = [
            (
                "POST",
                "/v1/machines/m1/ports",
                serde_json::json!({"id": "p1", "port": 3774, "url": server.uri(), "access": "public"}),
            ),
            (
                "GET",
                "/.well-known/t3/environment",
                serde_json::json!({"environmentId": "env-1", "label": "dev 1"}),
            ),
            (
                "GET",
                "/api/auth/session",
                serde_json::json!({"authenticated": true,
                    "auth": {"sessionMethods": ["dpop-access-token"]}}),
            ),
            (
                "POST",
                "/oauth/token",
                serde_json::json!({"access_token": "up-1", "token_type": "DPoP", "expires_in": 3600}),
            ),
        ];
        for (m, p, body) in answers {
            Mock::given(method(m))
                .and(path(p))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&server)
                .await;
        }
        let copied = Arc::new(Mutex::new(Vec::new()));
        let (core, opened) = recording_core(
            base.clone(),
            Arc::new(MemoryKeyStore::default()),
            Arc::new(MemorySshStore::default()),
            PathBuf::from("/nonexistent"),
        );
        let core = Arc::new(core.with_clipboard(SecretClipboard::new(
            Arc::new(RecordingClipboard(copied.clone())),
            Arc::new(|_| Box::pin(std::future::pending())),
        )));
        sign_in_as(&core, &base, 7);
        core.update(|i| {
            i.state.set_desktop(Ade::T3, Some("0.1.13".into()));
            if connected {
                i.data.connected.insert("m1".into());
            }
        });
        core.refresh(false).await;
        if let Some(dir) = proxy {
            let source: t3proxy::hub::ClientSource = {
                let core = Arc::downgrade(&core);
                Arc::new(move || core.upgrade().and_then(|c| c.current_client()))
            };
            let hub = t3proxy::Hub::new(
                dir.to_path_buf(),
                Arc::new(t3proxy::hub::Instabox::new(source)),
                Arc::new(t3proxy::hub::MemoryTokenStore::default()),
                t3proxy::runtime::Tuning::default(),
                false,
            );
            core.set_t3_proxy(hub, t3proxy::Gate::Tested);
        }
        (core, opened, copied, server)
    }

    fn take(list: &Mutex<Vec<String>>) -> Vec<String> {
        std::mem::take(&mut *list.lock().unwrap())
    }

    /// T3 Code's Add route with what the guide copied: the code becomes T3 Code's token.
    async fn add_route(url: &str) {
        let (base, code) = url.split_once("/pair#token=").unwrap();
        let port = base.strip_prefix("http://localhost:").unwrap();
        let res = reqwest::Client::new()
            .post(format!("http://127.0.0.1:{port}/oauth/token"))
            .form(&[
                (
                    "grant_type",
                    "urn:ietf:params:oauth:grant-type:token-exchange",
                ),
                ("subject_token", code),
            ])
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200, "the proxy takes the copied code");
    }

    // One test, so that its proxies take their ports one after the other.
    #[tokio::test]
    async fn the_t3_switch_guide_shows_until_t3_code_uses_the_proxy_or_it_is_done() {
        let proxy_link = |opened: &[String]| {
            opened.len() == 1
                && opened[0].starts_with("t3code://pair?pairingUrl=http%3A%2F%2Flocalhost%3A")
        };
        // A sandbox InstaADE paired before, which T3 Code never reached through the proxy: the
        // guide shows instead of the pairing link, which T3 Code would refuse.
        let dir = tempfile::tempdir().unwrap();
        let (core, opened, copied, server) = t3_switch_core(true, Some(dir.path())).await;
        assert_eq!(core.open_in_ade("m1", Some(Ade::T3)).await, Ok(None));
        assert!(take(&opened).is_empty(), "no pairing link opened");
        let guide = core.state().t3_switch.expect("the guide shows");
        assert_eq!((guide.id.as_str(), guide.name.as_str()), ("m1", "dev 1"));
        assert_eq!(guide.old_address, server.uri());
        assert!(guide.new_address.starts_with("http://localhost:"));
        // Copy: a new pairing URL of the proxy, never in the state.
        core.copy_t3_switch_url("m1").await.unwrap();
        let url = take(&copied).pop().unwrap();
        assert!(url.starts_with(&format!("{}/pair#token=", guide.new_address)));
        assert!(
            !serde_json::to_string(&core.state())
                .unwrap()
                .contains("token=")
        );
        // Closed for now: Pair again shows it again.
        core.close_t3_switch("m1", false).await;
        assert_eq!(core.state().t3_switch, None);
        assert_eq!(core.pair_again("m1", None).await, Ok(()));
        assert!(take(&opened).is_empty());
        assert!(core.state().t3_switch.is_some(), "shown again");
        // T3 Code added the proxy as a route: switched, the guide is not due any more.
        core.copy_t3_switch_url("m1").await.unwrap();
        add_route(&take(&copied).pop().unwrap()).await;
        core.close_t3_switch("m1", false).await;
        assert_eq!(core.open_in_ade("m1", Some(Ade::T3)).await, Ok(None));
        assert!(proxy_link(&take(&opened)), "the proxy's pairing link opens");
        assert_eq!(core.state().t3_switch, None);

        // Done (or Don't show again): not offered again for that sandbox, also after a restart.
        let dir = tempfile::tempdir().unwrap();
        let (core, opened, _, _server) = t3_switch_core(true, Some(dir.path())).await;
        assert_eq!(core.open_in_ade("m1", Some(Ade::T3)).await, Ok(None));
        assert!(core.state().t3_switch.is_some());
        core.close_t3_switch("m1", true).await;
        assert_eq!(core.state().t3_switch, None);
        assert_eq!(core.open_in_ade("m1", Some(Ade::T3)).await, Ok(None));
        assert!(proxy_link(&take(&opened)));
        assert_eq!(core.state().t3_switch, None);
        let saved: t3proxy::runtime::Record = serde_json::from_slice(
            &std::fs::read(t3proxy::runtime::record_path(&dir.path().join("m1"))).unwrap(),
        )
        .unwrap();
        assert!(saved.switch_guide_done, "kept for the next start");

        // A sandbox InstaADE never paired from here: T3 Code has not saved it, no guide.
        let dir = tempfile::tempdir().unwrap();
        let (core, opened, _, _server) = t3_switch_core(false, Some(dir.path())).await;
        assert_eq!(core.pair_again("m1", None).await, Ok(()));
        assert!(proxy_link(&take(&opened)));
        assert_eq!(core.state().t3_switch, None);
    }

    #[tokio::test]
    async fn the_t3_switch_guide_never_shows_with_the_proxy_off() {
        let (core, opened, _, _server) = t3_switch_core(true, None).await;
        assert_eq!(core.open_in_ade("m1", Some(Ade::T3)).await, Ok(None));
        assert_eq!(core.pair_again("m1", None).await, Ok(()));
        let opened = take(&opened);
        assert_eq!(opened.len(), 2);
        assert!(
            opened.iter().all(|l| l.starts_with("t3code://pair?pairingUrl=http")
                && !l.contains("localhost")),
            "{opened:?}"
        );
        assert_eq!(core.state().t3_switch, None);
        assert!(
            core.copy_t3_switch_url("m1")
                .await
                .unwrap_err()
                .message
                .contains("off")
        );
    }

    #[tokio::test]
    async fn open_still_pairs_without_dpop_when_the_refresh_fails() {
        // T3 Code 0.1.10, at the sandbox's version and behind it: a bearer pairing.
        for found in ["t3 v0.1.10\n", "t3 v0.1.12\n"] {
            let (core, r, opened, execs, _server) =
                open_after_refresh("0.1.10", found, 1, "T3 server did not start").await;
            assert_eq!(r, Ok(None), "{found}");
            assert_eq!(opened.len(), 1, "{found}");
            assert!(paired(&execs), "{found}");
            assert_eq!(core.state().error, None, "{found}");
        }
    }

    #[tokio::test]
    async fn open_still_pairs_a_busy_t3_without_dpop() {
        // T3 Code 0.1.10 pairs with a bearer token, which the old setup takes.
        let (core, r, opened, execs, _server) =
            open_after_refresh("0.1.10", "t3 v0.1.10\n", 3, "T3 is busy, update deferred").await;
        assert_eq!(r, Ok(None));
        assert_eq!(
            opened,
            ["t3code://pair?pairingUrl=https%3A%2F%2F3774-m1.example.test%2Fpair%23token%3Dabc123"]
        );
        assert!(paired(&execs));
        assert!(core.state().sandboxes[0].update_pending);
        assert_eq!(core.state().error, None);
    }

    // ---- Paradev ----

    /// What the popover shows once the Paradev host setup opened.
    fn paradev_notice() -> Notice {
        Some(
            "The SSH address is copied. In Paradev, click Set up a host over SSH and paste it."
                .to_string(),
        )
    }

    /// The link to the host of the sandbox whose machine id `paradev_server` reports.
    const PARADEV_LINK: &str = "superset://new-workspace?host=977b9d5310b351a8e85d4a4b333d5481";

    /// `cmux_server`, whose exec answers Paradev's host check with the host service `state`.
    async fn paradev_server(
        metadata: serde_json::Value,
        state: &str,
    ) -> (wiremock::MockServer, Url) {
        use wiremock::matchers::{body_string_contains, method, path};
        use wiremock::{Mock, ResponseTemplate};
        let (server, base) = cmux_server(metadata).await;
        let stdout = format!("service={state}\nmachineid=0123456789abcdef0123456789abcdef\n");
        Mock::given(method("POST"))
            .and(path("/v1/machines/m1/exec"))
            .and(body_string_contains("superset-host"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({"stdout": stdout, "stderr": "", "exitCode": 0}),
                ),
            )
            .with_priority(1)
            .mount(&server)
            .await;
        (server, base)
    }

    /// A signed-in core with a recording opener, a recording clipboard and a Superset.app as
    /// `here` says (as for cmux), found at the last look unless it is missing.
    struct ParadevCore {
        core: Arc<Core>,
        opened: Arc<Mutex<Vec<String>>>,
        copied: Arc<Mutex<Vec<String>>>,
        app: PathBuf,
        _dir: tempfile::TempDir,
    }

    /// A clipboard that records what was copied to it.
    struct RecordingClipboard(Arc<Mutex<Vec<String>>>);

    impl crate::clipboard::Clipboard for RecordingClipboard {
        fn read(&self) -> Option<String> {
            self.0.lock().unwrap().last().cloned()
        }
        fn write(&self, text: &str) -> Result<(), String> {
            self.0.lock().unwrap().push(text.to_string());
            Ok(())
        }
    }

    fn paradev_core(base: &Url, here: CmuxHere) -> ParadevCore {
        let dir = tempfile::tempdir().unwrap();
        let id = if here == CmuxHere::OtherId {
            "com.superset.desktop.canary"
        } else {
            ade::paradev::BUNDLE_ID
        };
        let app = ade::paradev::test_bundle(dir.path(), id, "2.0.11", false);
        let (core, opened) = recording_core(
            base.clone(),
            Arc::new(MemoryKeyStore::default()),
            Arc::new(MemorySshStore::default()),
            PathBuf::from("/nonexistent"),
        );
        let copied = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = copied.clone();
        let found = app.clone();
        let core = Arc::new(
            core.with_apps(
                Arc::new(move |ade| {
                    (here != CmuxHere::Missing && ade == Ade::Paradev).then(|| found.clone())
                }),
                Arc::new(move |ade, path| {
                    ade == Ade::Paradev
                        && ade::paradev::verify_with(path, |_| here != CmuxHere::BadSignature)
                }),
            )
            // The minute before the clearing never ends here: crate::clipboard tests that.
            .with_clipboard(SecretClipboard::new(
                Arc::new(RecordingClipboard(sink)),
                Arc::new(|_| Box::pin(std::future::pending())),
            )),
        );
        sign_in_as(&core, base, 7);
        if here != CmuxHere::Missing {
            core.update(|i| i.state.set_desktop(Ade::Paradev, Some("2.0.11".into())));
        }
        ParadevCore {
            core,
            opened,
            copied,
            app,
            _dir: dir,
        }
    }

    #[tokio::test]
    async fn installing_paradev_copies_the_ssh_address_and_opens_its_host_setup_in_the_app() {
        let (server, base) = paradev_server(serde_json::json!({}), "inactive").await;
        let ParadevCore {
            core,
            opened,
            copied,
            app,
            _dir,
        } = paradev_core(&base, CmuxHere::Real);
        core.refresh(false).await;
        let r = core.connect_sandbox("m1", Some(Ade::Paradev)).await;
        assert_eq!(r, Ok(paradev_notice()));
        // The token is the SSH user, at the gateway's host and port.
        assert_eq!(
            *copied.lock().unwrap(),
            ["ssh://ibssh_new@ssh.example.test:2222"]
        );
        // Only in the Paradev app it checked, never the default handler of superset:// links.
        assert_eq!(
            *opened.lock().unwrap(),
            [format!("superset://settings/hosts in {}", app.display())]
        );
        let r = requests(&server).await;
        assert_eq!(
            r.iter().filter(|x| x.ends_with("/exec")).count(),
            1,
            "the check: {r:?}"
        );
        assert_eq!(
            r.iter().filter(|x| x.ends_with("/ssh-tokens")).count(),
            1,
            "{r:?}"
        );
        assert!(
            !r.iter().any(|x| x.starts_with("PATCH")),
            "not marked: {r:?}"
        );
        let s = core.state();
        assert_eq!((s.progress, s.error), (None, None));
        assert_eq!(s.sandboxes[0].ades, vec![]);
        // Paradev pairs nothing from here.
        assert!(!core.lock().data.connected.contains("m1"));
    }

    #[tokio::test]
    async fn quitting_clears_the_copied_paradev_address() {
        let (_server, base) = paradev_server(serde_json::json!({}), "inactive").await;
        let ParadevCore {
            core,
            opened: _,
            copied,
            app: _,
            _dir,
        } = paradev_core(&base, CmuxHere::Real);
        core.refresh(false).await;
        core.on_quit();
        assert!(copied.lock().unwrap().is_empty(), "nothing to clear yet");
        let r = core.connect_sandbox("m1", Some(Ade::Paradev)).await;
        assert_eq!(r, Ok(paradev_notice()));
        // The minute is not up: the quit clears it.
        core.on_quit();
        assert_eq!(
            *copied.lock().unwrap(),
            ["ssh://ibssh_new@ssh.example.test:2222", ""]
        );
    }

    #[tokio::test]
    async fn installing_paradev_once_its_host_runs_marks_the_sandbox_and_opens_it() {
        let (server, base) =
            paradev_server(serde_json::json!({"instabox-bridge": "t3"}), "active").await;
        let ParadevCore {
            core,
            opened,
            copied,
            app,
            _dir,
        } = paradev_core(&base, CmuxHere::Real);
        core.refresh(false).await;
        let r = core.connect_sandbox("m1", Some(Ade::Paradev)).await;
        assert_eq!(r, Ok(None));
        assert_eq!(
            *opened.lock().unwrap(),
            [format!("{PARADEV_LINK} in {}", app.display())]
        );
        assert!(copied.lock().unwrap().is_empty(), "nothing to paste");
        let reqs = server.received_requests().await.unwrap();
        assert!(!reqs.iter().any(|r| r.url.path().ends_with("/ssh-tokens")));
        let patch = reqs.iter().find(|r| r.method.as_str() == "PATCH").unwrap();
        let body: serde_json::Value = serde_json::from_slice(&patch.body).unwrap();
        assert_eq!(body["metadata"]["instabox-bridge"], "t3");
        assert_eq!(body["metadata"]["instabox-ades"], "t3,paradev");
        assert!(!core.lock().data.connected.contains("m1"));
    }

    /// A Mac with the old Superset.app 2.0.11 and the new Paradev.app 2.0.12: the version shown
    /// is Paradev.app's, and the links open in it, never in Superset.app or whatever handles
    /// superset:// links. Both bundles are fakes in a temporary folder, and the lookup and the
    /// version shown come from there, not from this computer's Applications.
    #[tokio::test]
    async fn paradev_opens_in_the_newest_of_superset_app_and_paradev_app() {
        let (_server, base) =
            paradev_server(serde_json::json!({"instabox-bridge": "t3"}), "active").await;
        let dir = tempfile::tempdir().unwrap();
        let id = ade::paradev::BUNDLE_ID;
        ade::paradev::test_bundle_named(dir.path(), "Superset.app", id, "2.0.11", false);
        let new = ade::paradev::test_bundle_named(dir.path(), "Paradev.app", id, "2.0.12", false);
        let (core, opened) = recording_core(
            base.clone(),
            Arc::new(MemoryKeyStore::default()),
            Arc::new(MemorySshStore::default()),
            PathBuf::from("/nonexistent"),
        );
        let dirs = [dir.path().to_path_buf()];
        let real = |p: &Path| ade::paradev::verify_with(p, |_| true);
        assert_eq!(ade::paradev::locate_in(&dirs, real), Some(new.clone()));
        let core = Arc::new(core.with_apps(
            Arc::new(move |ade| {
                (ade == Ade::Paradev)
                    .then(|| ade::paradev::locate_in(&dirs, real))
                    .flatten()
            }),
            Arc::new(move |ade, path| ade == Ade::Paradev && real(path)),
        ));
        sign_in_as(&core, &base, 7);
        // What the app's detection shows: the version of the bundle the lookup found. Set here,
        // so nothing looks at this computer's Applications.
        let found = (core.locate)(Ade::Paradev).unwrap();
        let shown = ade::paradev::version(&found).unwrap();
        core.update(|i| i.state.set_desktop(Ade::Paradev, Some(shown)));
        core.refresh(false).await;
        let r = core.connect_sandbox("m1", Some(Ade::Paradev)).await;
        assert_eq!(r, Ok(None));
        assert_eq!(core.state().desktop(Ade::Paradev), Some("2.0.12"));
        assert_eq!(
            *opened.lock().unwrap(),
            [format!("{PARADEV_LINK} in {}", new.display())]
        );
    }

    #[tokio::test]
    async fn open_in_paradev_wakes_the_sandbox_then_opens_its_host() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};
        let md = serde_json::json!({"instabox-bridge": "paradev", "instabox-ades": "paradev"});
        let (server, base) = paradev_server(md.clone(), "active").await;
        // Asleep: Paradev would not wake it.
        Mock::given(method("GET"))
            .and(path("/v1/machines/m1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "m1", "name": "dev 1", "state": "suspended",
                "template": {"id": "agent-browser"}, "spec": {"vcpus": 2, "memMiB": 4096},
                "metadata": md, "orgId": "org-me"
            })))
            .up_to_n_times(1)
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/machines/m1/resume"))
            .respond_with(ResponseTemplate::new(202).set_body_json(serde_json::json!({
                "id": "op1", "machineId": "m1", "type": "resume", "state": "queued", "machine": null
            })))
            .mount(&server)
            .await;
        let ParadevCore {
            core,
            opened,
            copied,
            app,
            _dir,
        } = paradev_core(&base, CmuxHere::Real);
        core.refresh(false).await;
        let r = core.open_in_ade("m1", Some(Ade::Paradev)).await;
        assert_eq!(r, Ok(None));
        assert_eq!(
            *opened.lock().unwrap(),
            [format!("{PARADEV_LINK} in {}", app.display())]
        );
        assert!(copied.lock().unwrap().is_empty());
        let r = requests(&server).await;
        let at = |p: &str| {
            r.iter()
                .position(|x| x == p)
                .unwrap_or_else(|| panic!("{p}: {r:?}"))
        };
        assert!(at("POST /v1/machines/m1/resume") < at("POST /v1/machines/m1/exec"));
        assert!(!r.iter().any(|x| x.starts_with("PATCH")), "{r:?}");
        assert!(!r.iter().any(|x| x.ends_with("/ssh-tokens")), "{r:?}");
    }

    #[tokio::test]
    async fn paradev_gets_nothing_without_its_real_app() {
        for here in [CmuxHere::Missing, CmuxHere::OtherId, CmuxHere::BadSignature] {
            for (md, open) in [
                (serde_json::json!({}), false),
                (serde_json::json!({"instabox-bridge": "paradev"}), true),
            ] {
                let (server, base) = paradev_server(md, "inactive").await;
                let ParadevCore {
                    core,
                    opened,
                    copied,
                    app: _,
                    _dir,
                } = paradev_core(&base, here);
                core.refresh(false).await;
                let e = if open {
                    core.open_in_ade("m1", Some(Ade::Paradev)).await
                } else {
                    core.connect_sandbox("m1", Some(Ade::Paradev)).await
                }
                .unwrap_err();
                let want = if here == CmuxHere::Missing {
                    "Paradev is not installed. Install it first."
                } else {
                    "Could not verify the Paradev app. Install it again from its official download."
                };
                assert_eq!(e.message, want, "{open}");
                assert!(opened.lock().unwrap().is_empty(), "nothing opened");
                assert!(copied.lock().unwrap().is_empty(), "nothing copied");
                let r = requests(&server).await;
                assert!(
                    !r.iter().any(|x| x.ends_with("/ssh-tokens")),
                    "no token: {r:?}"
                );
                assert!(!r.iter().any(|x| x.starts_with("PATCH")), "{r:?}");
                // The app is checked before the sandbox is touched: no wake and no check runs.
                assert!(
                    !r.iter()
                        .any(|x| x.ends_with("/exec") || x.ends_with("/resume")),
                    "{open}: {r:?}"
                );
            }
        }
    }

    // ---- Herdr: a terminal that runs it on the sandbox ----

    /// The OpenSSH client the test computer has: where Windows installs it, the place found
    /// when no `ssh.exe` is on the PATH.
    const HERDR_SSH: &str = r"C:\Windows\System32\OpenSSH\ssh.exe";

    /// The terminal Open in Herdr opens: that exact client, in an SSH session that runs
    /// `herdr`, with the token.
    const HERDR_TERMINAL: &str = r"terminal: C:\Windows\System32\OpenSSH\ssh.exe -t -o StrictHostKeyChecking=accept-new -p 2222 ibssh_new@ssh.example.test herdr";

    /// `cmux_server`, whose exec answers Herdr's setup with `answer`.
    async fn herdr_server(
        metadata: serde_json::Value,
        answer: serde_json::Value,
    ) -> (wiremock::MockServer, Url) {
        use wiremock::matchers::{body_string_contains, method, path};
        use wiremock::{Mock, ResponseTemplate};
        let (server, base) = cmux_server(metadata).await;
        Mock::given(method("POST"))
            .and(path("/v1/machines/m1/exec"))
            .and(body_string_contains("https://herdr.dev/install.sh"))
            .respond_with(ResponseTemplate::new(200).set_body_json(answer))
            .with_priority(1)
            .mount(&server)
            .await;
        (server, base)
    }

    /// What Herdr's setup prints when it went well.
    fn herdr_installed() -> serde_json::Value {
        serde_json::json!({"stdout": "herdr 0.9.5\n", "stderr": "", "exitCode": 0})
    }

    /// A signed-in core with a recording opener and terminal, on a computer whose OpenSSH
    /// client is found unless `ssh` is false.
    fn herdr_core(base: &Url, ssh: bool) -> (Arc<Core>, Arc<Mutex<Vec<String>>>) {
        let (core, opened) = recording_core(
            base.clone(),
            Arc::new(MemoryKeyStore::default()),
            Arc::new(MemorySshStore::default()),
            PathBuf::from("/nonexistent"),
        );
        let core = Arc::new(core.with_apps(
            Arc::new(move |ade| (ssh && ade == Ade::Herdr).then(|| PathBuf::from(HERDR_SSH))),
            Arc::new(|ade, path| ade.verify(path)),
        ));
        sign_in_as(&core, base, 7);
        if ssh {
            core.update(|i| {
                i.state
                    .set_desktop(Ade::Herdr, Some("OpenSSH 10.3p1".into()))
            });
        }
        (core, opened)
    }

    #[tokio::test]
    async fn installing_herdr_sets_it_up_marks_the_sandbox_and_opens_a_terminal() {
        let (server, base) = herdr_server(
            serde_json::json!({"instabox-bridge": "t3"}),
            herdr_installed(),
        )
        .await;
        let (core, opened) = herdr_core(&base, true);
        core.refresh(false).await;
        let r = core.connect_sandbox("m1", Some(Ade::Herdr)).await;
        assert_eq!(r, Ok(None), "no notice: the terminal shows Herdr");
        // Only the terminal opens: no link and no app.
        assert_eq!(*opened.lock().unwrap(), [HERDR_TERMINAL]);
        let reqs = server.received_requests().await.unwrap();
        let execs: Vec<String> = reqs
            .iter()
            .filter(|r| r.url.path().ends_with("/exec"))
            .map(|r| String::from_utf8_lossy(&r.body).to_string())
            .collect();
        assert_eq!(execs.len(), 1, "only the setup: {execs:?}");
        assert!(execs[0].contains("https://herdr.dev/install.sh"));
        let patch = reqs.iter().find(|r| r.method.as_str() == "PATCH").unwrap();
        let body: serde_json::Value = serde_json::from_slice(&patch.body).unwrap();
        assert_eq!(body["metadata"]["instabox-bridge"], "t3");
        assert_eq!(body["metadata"]["instabox-ades"], "t3,herdr");
        // The mark comes before the token, which the terminal gets.
        let r = requests(&server).await;
        let at = |p: &str| {
            r.iter()
                .position(|x| x == p)
                .unwrap_or_else(|| panic!("{p}: {r:?}"))
        };
        assert!(at("PATCH /v1/machines/m1") < at("POST /v1/machines/m1/ssh-tokens"));
        let s = core.state();
        assert_eq!((s.progress, s.error), (None, None));
        // Herdr pairs nothing from here.
        assert!(!core.lock().data.connected.contains("m1"));
    }

    #[tokio::test]
    async fn open_in_herdr_wakes_the_sandbox_then_opens_a_terminal_and_installs_nothing() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};
        let md = serde_json::json!({"instabox-bridge": "herdr", "instabox-ades": "herdr"});
        let (server, base) = herdr_server(md.clone(), herdr_installed()).await;
        // Asleep: the SSH gateway refuses a paused sandbox.
        Mock::given(method("GET"))
            .and(path("/v1/machines/m1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "m1", "name": "dev 1", "state": "suspended",
                "template": {"id": "agent-browser"}, "spec": {"vcpus": 2, "memMiB": 4096},
                "metadata": md, "orgId": "org-me"
            })))
            .up_to_n_times(1)
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/machines/m1/resume"))
            .respond_with(ResponseTemplate::new(202).set_body_json(serde_json::json!({
                "id": "op1", "machineId": "m1", "type": "resume", "state": "queued", "machine": null
            })))
            .mount(&server)
            .await;
        let (core, opened) = herdr_core(&base, true);
        core.refresh(false).await;
        let r = core.open_in_ade("m1", Some(Ade::Herdr)).await;
        assert_eq!(r, Ok(None));
        assert_eq!(*opened.lock().unwrap(), [HERDR_TERMINAL]);
        let r = requests(&server).await;
        let at = |p: &str| {
            r.iter()
                .position(|x| x == p)
                .unwrap_or_else(|| panic!("{p}: {r:?}"))
        };
        assert!(at("POST /v1/machines/m1/resume") < at("POST /v1/machines/m1/ssh-tokens"));
        assert!(!r.iter().any(|x| x.ends_with("/exec")), "{r:?}");
        assert!(!r.iter().any(|x| x.starts_with("PATCH")), "{r:?}");
        // The next open reuses the stored token.
        core.open_in_ade("m1", Some(Ade::Herdr)).await.unwrap();
        assert_eq!(*opened.lock().unwrap(), [HERDR_TERMINAL, HERDR_TERMINAL]);
        let r = requests(&server).await;
        assert_eq!(r.iter().filter(|x| x.ends_with("/ssh-tokens")).count(), 1);
    }

    #[tokio::test]
    async fn a_new_sandbox_in_herdr_mode_gets_herdr_and_opens_in_a_terminal() {
        let (server, base) = herdr_server(serde_json::json!({}), herdr_installed()).await;
        let (core, opened) = herdr_core(&base, true);
        let notice = core
            .create_sandbox(CreateArgs {
                name: "dev 1".into(),
                snapshot_id: None,
                vcpus: 2,
                mem_mib: 4096,
                auto_suspend_after_seconds: Some(1800),
                ade: Some(Ade::Herdr),
            })
            .await
            .unwrap();
        assert_eq!(notice, None);
        assert_eq!(*opened.lock().unwrap(), [HERDR_TERMINAL]);
        let r = requests(&server).await;
        assert_eq!(r.iter().filter(|x| x.ends_with("/exec")).count(), 1);
    }

    #[tokio::test]
    async fn herdr_touches_nothing_without_an_ssh_client() {
        for (md, open) in [
            (serde_json::json!({}), false),
            (serde_json::json!({"instabox-bridge": "herdr"}), true),
        ] {
            let (server, base) = herdr_server(md, herdr_installed()).await;
            let (core, opened) = herdr_core(&base, false);
            core.refresh(false).await;
            let e = if open {
                core.open_in_ade("m1", Some(Ade::Herdr)).await
            } else {
                core.connect_sandbox("m1", Some(Ade::Herdr)).await
            }
            .unwrap_err();
            assert_eq!(e.message, ade::herdr::MISSING, "{open}");
            assert!(opened.lock().unwrap().is_empty(), "nothing opened");
            let r = requests(&server).await;
            assert!(
                !r.iter().any(|x| x.ends_with("/ssh-tokens")
                    || x.ends_with("/exec")
                    || x.ends_with("/resume")
                    || x.starts_with("PATCH")),
                "{open}: {r:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_failed_herdr_setup_opens_no_terminal() {
        let answer = serde_json::json!({"stdout": "", "stderr": "download failed", "exitCode": 1});
        let (server, base) = herdr_server(serde_json::json!({}), answer).await;
        let (core, opened) = herdr_core(&base, true);
        core.refresh(false).await;
        let e = core
            .connect_sandbox("m1", Some(Ade::Herdr))
            .await
            .unwrap_err();
        assert!(e.message.contains("download failed"), "{e:?}");
        assert!(opened.lock().unwrap().is_empty());
        let r = requests(&server).await;
        assert!(
            !r.iter()
                .any(|x| x.ends_with("/ssh-tokens") || x.starts_with("PATCH")),
            "{r:?}"
        );
    }

    #[tokio::test]
    async fn open_in_terminal_opens_the_login_from_the_token_fields() {
        let (_server, base) = cmux_server(serde_json::json!({})).await;
        let (core, opened) = herdr_core(&base, true);
        core.open_ssh("m1").await.unwrap();
        assert_eq!(
            *opened.lock().unwrap(),
            ["terminal: ssh -p 2222 ibssh_new@ssh.example.test"]
        );
    }

    /// Answers every new SSH token of `m1` with `host` and `token`, ahead of `cmux_server`'s.
    async fn hostile_token(server: &wiremock::MockServer, host: &str, token: &str) {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};
        Mock::given(method("POST"))
            .and(path("/v1/machines/m1/ssh-tokens"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "k1", "name": "Instabox", "host": host, "port": 2222,
                "token": token, "command": format!("ssh -p 2222 {token}@{host}"),
                "expiresAt": Utc::now() + chrono::Duration::days(30)
            })))
            .with_priority(1)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn a_hostile_ssh_login_opens_no_terminal_for_herdr_or_open_in_terminal() {
        let hostile = [
            ("ssh.example.test;open -a Calculator", "ibssh_x"),
            ("ssh.example.test", "ibssh_x$(touch /tmp/pwned)"),
            ("ssh.example.test", "ibssh_x`touch /tmp/pwned`"),
            ("ssh.example.test", "ibssh_x'\"y"),
            ("ssh.example.test\ntouch /tmp/pwned", "ibssh_x"),
            ("ssh example test", "ibssh_x"),
            ("ssh.example.test", "-oProxyCommand=calc"),
        ];
        for (host, token) in hostile {
            let md = serde_json::json!({"instabox-bridge": "herdr", "instabox-ades": "herdr"});
            let (server, base) = herdr_server(md, herdr_installed()).await;
            hostile_token(&server, host, token).await;
            let (core, opened) = herdr_core(&base, true);
            core.refresh(false).await;
            let herdr = core.open_in_ade("m1", Some(Ade::Herdr)).await.unwrap_err();
            assert_eq!(herdr.message, ssh::NOT_PLAIN, "{host:?} {token:?}");
            let terminal = core.open_ssh("m1").await.unwrap_err();
            assert_eq!(terminal.message, ssh::NOT_PLAIN, "{host:?} {token:?}");
            assert!(opened.lock().unwrap().is_empty(), "{host:?} {token:?}");
        }
    }

    #[tokio::test]
    async fn the_ssh_client_checked_right_before_the_token_is_the_one_that_runs() {
        use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
        let md = serde_json::json!({"instabox-bridge": "herdr", "instabox-ades": "herdr"});
        let (_server, base) = herdr_server(md, herdr_installed()).await;
        let (core, opened) = recording_core(
            base.clone(),
            Arc::new(MemoryKeyStore::default()),
            Arc::new(MemorySshStore::default()),
            PathBuf::from("/nonexistent"),
        );
        // The first look (before the wake) finds one client, the next look another.
        let looks = Arc::new(AtomicUsize::new(0));
        let seen = looks.clone();
        let core = Arc::new(core.with_apps(
            Arc::new(move |ade| {
                (ade == Ade::Herdr).then(|| match seen.fetch_add(1, SeqCst) {
                    0 => PathBuf::from(r"C:\Program Files\Git\usr\bin\ssh.exe"),
                    _ => PathBuf::from(HERDR_SSH),
                })
            }),
            Arc::new(|ade, path| ade.verify(path)),
        ));
        sign_in_as(&core, &base, 7);
        core.refresh(false).await;
        core.open_in_ade("m1", Some(Ade::Herdr)).await.unwrap();
        assert_eq!(
            looks.load(SeqCst),
            2,
            "before the wake and before the token"
        );
        assert_eq!(*opened.lock().unwrap(), [HERDR_TERMINAL]);
    }

    #[tokio::test]
    async fn sign_out_during_mcp_key_creation_saves_no_key_and_writes_no_config() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/keys"))
            .respond_with(
                ResponseTemplate::new(201)
                    .set_body_json(serde_json::json!({"key": "ibk_new"}))
                    .set_delay(Duration::from_millis(600)),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/machines"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"machines": [], "nextCursor": null})),
            )
            .mount(&server)
            .await;
        let base = Url::parse(&server.uri()).unwrap();
        let home = tempfile::tempdir().unwrap();
        let keys = Arc::new(MemoryKeyStore::default());
        let (core, _) = core_full(
            base.clone(),
            keys.clone(),
            Arc::new(MemorySshStore::default()),
            home.path().to_path_buf(),
        );
        sign_in_as(&core, &base, 7);
        let c = core.clone();
        let call = tokio::spawn(async move { c.add_mcp(McpClient::Codex).await });
        tokio::time::sleep(Duration::from_millis(200)).await;
        core.sign_out().await.unwrap();
        let _ = call.await.unwrap();
        assert_eq!(keys.load().unwrap(), None, "no key saved");
        assert!(
            !home.path().join(".codex").exists(),
            "no client config written"
        );
    }

    #[tokio::test]
    async fn sign_out_during_ssh_token_creation_saves_no_token_and_opens_no_terminal() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/machines/m1/ssh-tokens"))
            .respond_with(
                ResponseTemplate::new(201)
                    .set_body_json(serde_json::json!(
                        {"id":"i","name":"n","host":"ssh.example.test","port":2222,
                         "token":"ibssh_x","command":"ssh","expiresAt":"2099-01-01T00:00:00Z"}
                    ))
                    .set_delay(Duration::from_millis(600)),
            )
            .mount(&server)
            .await;
        let base = Url::parse(&server.uri()).unwrap();
        let ssh = Arc::new(MemorySshStore::default());
        let (core, _) = core_full(
            base.clone(),
            Arc::new(MemoryKeyStore::default()),
            ssh.clone(),
            PathBuf::from("/nonexistent"),
        );
        sign_in_as(&core, &base, 7);
        let c = core.clone();
        // The token step fails before any Terminal launch, so this opens nothing.
        let call = tokio::spawn(async move { c.open_ssh("m1").await });
        tokio::time::sleep(Duration::from_millis(200)).await;
        core.sign_out().await.unwrap();
        assert!(call.await.unwrap().is_err());
        assert_eq!(
            ssh.load("m1", Kind::Ordinary).unwrap(),
            None,
            "no token saved"
        );
    }

    // ---- account boundary: forced interleavings ----

    /// The failed checks of a test with several cases, so one run reports them all.
    #[derive(Default)]
    struct Checks(Vec<String>);

    impl Checks {
        fn check(&mut self, ok: bool, what: impl Into<String>) {
            if !ok {
                self.0.push(what.into());
            }
        }

        fn done(self) {
            assert!(self.0.is_empty(), "failed checks:\n{}", self.0.join("\n"));
        }
    }

    /// Runs `f` once, the first time a flow of `core` reaches the step `name`.
    fn at_step(
        core: &Arc<Core>,
        name: &'static str,
        f: impl Fn(&Arc<Core>) + Send + Sync + 'static,
    ) {
        let weak = Arc::downgrade(core);
        let done = std::sync::atomic::AtomicBool::new(false);
        *core.hook.lock().unwrap() = Some(Arc::new(move |step: &str| {
            if step == name
                && !done.swap(true, std::sync::atomic::Ordering::SeqCst)
                && let Some(c) = weak.upgrade()
            {
                f(&c);
            }
        }));
    }

    #[tokio::test]
    async fn an_account_switch_right_before_settle_leaves_the_new_account_alone() {
        let base = Url::parse("http://127.0.0.1:9").unwrap();
        // A failure of the old account's flow is not shown to the new one.
        let core = core_on(base.clone(), Arc::new(MemoryConnectedStore::default()));
        sign_in_as(&core, &base, 7);
        let old = core.account();
        let b = base.clone();
        at_step(&core, "settle", move |c| sign_in_as(c, &b, 8));
        let mut c = Checks::default();
        c.check(
            core.settle_in::<()>(old, Err(ErrorView::other("boom")))
                .is_err(),
            "the caller gets its error",
        );
        c.check(core.account() != old, "the switch happened in between");
        c.check(
            core.state().error.is_none(),
            format!("the new account shows {:?}", core.state().error),
        );

        // A success of the old account's flow does not clear the new account's error.
        let core = core_on(base.clone(), Arc::new(MemoryConnectedStore::default()));
        sign_in_as(&core, &base, 7);
        let old = core.account();
        let b = base.clone();
        at_step(&core, "settle", move |c| {
            sign_in_as(c, &b, 8);
            c.update(|i| i.state.error = Some(ErrorView::other("the new account's")));
        });
        c.check(core.settle_in(old, Ok(())).is_ok(), "the caller gets Ok");
        c.check(
            core.state().error == Some(ErrorView::other("the new account's")),
            format!("the new account's error became {:?}", core.state().error),
        );
        c.done();
    }

    #[tokio::test]
    async fn an_account_switch_inside_finish_connect_leaves_the_new_account_alone() {
        let base = Url::parse("http://127.0.0.1:9").unwrap();
        let mut c = Checks::default();
        for ok in [true, false] {
            let store = Arc::new(MemoryConnectedStore::default());
            let opened = Arc::new(Mutex::new(Vec::<String>::new()));
            let sink = opened.clone();
            let core = Arc::new(Core::new(
                base.clone(),
                Arc::new(MemoryKeyStore::default()),
                Arc::new(MemorySshStore::default()),
                store.clone(),
                Arc::new(move |u: &str, app: Option<&Path>| {
                    sink.lock().unwrap().push(opened_as(u, app));
                    Ok(())
                }),
                Arc::new(|_| {}),
                PathBuf::from("/nonexistent"),
            ));
            sign_in_as(&core, &base, 7);
            let old = core.account();
            core.begin_progress("connect", "pairing", Some("t".into()), Some(Ade::T3), old);
            let new_progress = ProgressView {
                kind: "create".into(),
                step: "creating".into(),
                machine_id: None,
                ade: Some(Ade::T3),
                error: None,
            };
            let b = base.clone();
            let p = new_progress.clone();
            // The switch lands after finish_connect checked the account, before it writes.
            at_step(&core, "finish", move |c| {
                sign_in_as(c, &b, 8);
                c.update(|i| {
                    i.data.machines =
                        vec![machine("t", "org-me", "agent-browser", "running", true)];
                    i.data.connected_user = Some(8);
                    i.state.progress = Some(p.clone());
                    i.rebuild();
                });
            });
            let r = if ok {
                Ok(connected_result("t", "0.1.11"))
            } else {
                Err(SandboxError::Timeout)
            };
            let _ = core.finish_connect(Ade::T3, r, "0.1.11", old).await;
            c.check(core.account() != old, format!("ok={ok}: no switch"));
            let s = core.state();
            c.check(
                s.progress == Some(new_progress),
                format!(
                    "ok={ok}: the new account's progress became {:?}",
                    s.progress
                ),
            );
            c.check(
                s.error.is_none(),
                format!("ok={ok}: the new account shows {:?}", s.error),
            );
            let d = core.lock().data.clone();
            c.check(
                !d.ade_versions.contains_key("t"),
                format!("ok={ok}: a T3 version was recorded"),
            );
            c.check(
                !d.connected.contains("t"),
                format!("ok={ok}: marked connected"),
            );
            c.check(
                opened.lock().unwrap().is_empty(),
                format!("ok={ok}: a link was opened"),
            );
            c.check(
                store.load(7).is_empty() && store.load(8).is_empty(),
                format!("ok={ok}: saved to the connected list"),
            );
        }
        c.done();
    }

    /// A connected list whose add and remove wait for the test, then write or fail.
    struct HeldConnected {
        started: tokio::sync::Notify,
        go: Mutex<std::sync::mpsc::Receiver<()>>,
        fail: bool,
        written: Mutex<Vec<(i64, String)>>,
    }

    impl HeldConnected {
        fn new(fail: bool) -> (Arc<Self>, std::sync::mpsc::Sender<()>) {
            let (tx, rx) = std::sync::mpsc::channel();
            let store = Arc::new(HeldConnected {
                started: tokio::sync::Notify::new(),
                go: Mutex::new(rx),
                fail,
                written: Mutex::new(Vec::new()),
            });
            (store, tx)
        }

        /// Like a file store: reads (here: waits), then writes through the gate.
        fn op(&self, user: i64, id: &str, gate: &dyn Gate) -> Result<(), String> {
            self.started.notify_one();
            let _ = self.go.lock().unwrap().recv();
            if self.fail {
                return Err("disk full".into());
            }
            guarded(gate, || {
                self.written.lock().unwrap().push((user, id.to_string()))
            });
            Ok(())
        }
    }

    impl ConnectedStore for HeldConnected {
        fn load(&self, _: i64) -> HashSet<String> {
            HashSet::new()
        }
        fn add(&self, user: i64, id: &str, gate: &dyn Gate) -> Result<(), String> {
            self.op(user, id, gate)
        }
        fn remove(&self, user: i64, id: &str, gate: &dyn Gate) -> Result<(), String> {
            self.op(user, id, gate)
        }
    }

    #[tokio::test]
    async fn a_connected_list_change_held_up_across_an_account_switch_does_not_land() {
        let base = Url::parse("http://127.0.0.1:9").unwrap();
        let mut c = Checks::default();
        for (add, fail) in [(true, false), (false, false), (true, true), (false, true)] {
            let (store, go) = HeldConnected::new(fail);
            let core = core_on(base.clone(), store.clone());
            sign_in_as(&core, &base, 7);
            let old = core.account();
            let flow = core.clone();
            let task = tokio::spawn(async move {
                if add {
                    flow.remember("m1", old).await
                } else {
                    flow.forget("m1", old).await
                }
            });
            // The store operation is under way; now another account signs in.
            store.started.notified().await;
            sign_in_as(&core, &base, 8);
            go.send(()).unwrap();
            task.await.unwrap();
            let case = format!("add={add} fail={fail}");
            let written = store.written.lock().unwrap().clone();
            c.check(written.is_empty(), format!("{case}: landed {written:?}"));
            let error = core.state().error;
            c.check(error.is_none(), format!("{case}: banner {error:?}"));
            c.check(
                !core.lock().data.connected.contains("m1"),
                format!("{case}: in the new account's connected set"),
            );
        }
        c.done();
    }

    /// Holds a refresh call until the test lets it answer.
    struct HeldRefresh {
        arrived: Arc<tokio::sync::Notify>,
        go: Mutex<std::sync::mpsc::Receiver<()>>,
        reply: wiremock::ResponseTemplate,
    }

    impl wiremock::Respond for HeldRefresh {
        fn respond(&self, _: &wiremock::Request) -> wiremock::ResponseTemplate {
            self.arrived.notify_one();
            let _ = self.go.lock().unwrap().recv();
            self.reply.clone()
        }
    }

    fn tokens_of(access: &str, secs: i64) -> Tokens {
        Tokens {
            access_token: access.into(),
            access_expires_at: Utc::now() + chrono::Duration::seconds(secs),
            refresh_token: format!("ibr_{access}"),
            refresh_expires_at: Utc::now() + chrono::Duration::days(30),
        }
    }

    #[tokio::test]
    async fn an_old_sessions_refresh_after_another_sign_in_neither_saves_nor_clears() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let mut c = Checks::default();
        for status in [200u16, 401] {
            let server = MockServer::start().await;
            let arrived = Arc::new(tokio::sync::Notify::new());
            let (go, held) = std::sync::mpsc::channel();
            let reply = if status == 200 {
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::to_value(tokens_of("iba_a2", 3600)).unwrap())
            } else {
                ResponseTemplate::new(401)
            };
            Mock::given(method("POST"))
                .and(path("/v1/auth/refresh"))
                .respond_with(HeldRefresh {
                    arrived: arrived.clone(),
                    go: Mutex::new(held),
                    reply,
                })
                .mount(&server)
                .await;
            let base = Url::parse(&server.uri()).unwrap();
            // Account A, from the last run, with an MCP key and an SSH token.
            let store = Arc::new(MemoryStore::with(tokens_of("iba_a", 5)));
            let keys = Arc::new(MemoryKeyStore::default());
            keys.save("ibk_a").unwrap();
            let ssh = Arc::new(MemorySshStore::default());
            ssh.save("m1", Kind::Ordinary, &ssh_token()).unwrap();
            let core = Arc::new(Core::new(
                base.clone(),
                keys.clone(),
                ssh.clone(),
                Arc::new(MemoryConnectedStore::default()),
                Arc::new(|_, _| Ok(())),
                Arc::new(|_| {}),
                PathBuf::from("/nonexistent"),
            ));
            let a = Session::load_from(&base, None, store.clone()).unwrap();
            assert!(core.install_session(a, core.account()));
            // A call of account A refreshes; the server holds the answer.
            let a = core.lock().session.clone().unwrap();
            let call = tokio::spawn(async move { a.bearer().await });
            arrived.notified().await;
            // Meanwhile a device sign-in started under account A finishes for account B
            // (the sign-in screen shows while a call reports a signed-out error).
            let b = device_session(store.clone(), "iba_b").await;
            core.install_signed_in(b, core.account()).unwrap();
            // A's refresh answers only now.
            go.send(()).unwrap();
            let got = call.await.unwrap();
            let saved = store.load().unwrap().map(|t| t.access_token);
            c.check(
                saved.as_deref() == Some("iba_b"),
                format!("{status}: the store holds {saved:?}, not B's tokens"),
            );
            c.check(got.is_err(), format!("{status}: A's call got {got:?}"));
            let current = core.lock().session.clone().unwrap();
            let token = current.current_access_token();
            c.check(
                token.as_deref() == Some("iba_b"),
                format!("{status}: the current session has {token:?}"),
            );
            c.check(
                keys.load().unwrap().is_none(),
                format!("{status}: A's MCP key is still there"),
            );
            c.check(
                ssh.load("m1", Kind::Ordinary).unwrap().is_none(),
                format!("{status}: A's SSH token is still there"),
            );
        }
        c.done();
    }

    /// A device sign-in that finishes with `access`; its session saves into `store`.
    async fn device_session(store: Arc<MemoryStore>, access: &str) -> Session {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/device/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::to_value(tokens_of(access, 3600)).unwrap()),
            )
            .mount(&server)
            .await;
        let start = crate::api::DeviceStart {
            device_code: "ibd".into(),
            user_code: "ABCD".into(),
            verification_uri: "https://x/device".into(),
            verification_uri_complete: None,
            interval: 1,
            expires_in: 900,
        };
        let base = Url::parse(&server.uri()).unwrap();
        auth::poll_with(&base, &start, store, |_| std::future::ready(()))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_session_from_before_another_account_change_is_not_installed() {
        let base = Url::parse("http://127.0.0.1:9").unwrap();
        let store = Arc::new(MemoryStore::default());
        let core = core_on(base.clone(), Arc::new(MemoryConnectedStore::default()));
        let since = core.account();
        let a = device_session(store.clone(), "iba_a").await;
        let b = device_session(store.clone(), "iba_b").await;
        assert_eq!(store.load().unwrap(), None, "a poll saves nothing");
        core.install_signed_in(a, since).unwrap();
        // B's sign-in began before A's finished: it changes nothing.
        assert_eq!(core.install_signed_in(b, since), Err(account_changed()));
        let current = || core.lock().session.clone().unwrap().current_access_token();
        assert_eq!(store.load().unwrap().unwrap().access_token, "iba_a");
        assert_eq!(current().as_deref(), Some("iba_a"));
        // Nor does the session the last run saved, read while A signed in.
        let saved = Arc::new(MemoryStore::with(tokens_of("iba_old", 3600)));
        let old = Session::load_from(&base, None, saved).unwrap();
        assert!(!core.install_session(old, since));
        assert_eq!(current().as_deref(), Some("iba_a"));
        assert_eq!(store.load().unwrap().unwrap().access_token, "iba_a");
    }

    /// Waits for the sign-in screen, then checks that the account ended in full: a new
    /// account count and no tokens, MCP key or SSH token left.
    async fn assert_account_ended(
        core: &Core,
        before: u64,
        stores: (&MemoryStore, &MemoryKeyStore, &MemorySshStore),
        signed_out: &tokio::sync::Notify,
    ) {
        let (store, keys, ssh) = stores;
        let wait = tokio::time::timeout(Duration::from_secs(3), signed_out.notified());
        let mut c = Checks::default();
        c.check(wait.await.is_ok(), "the sign-in screen was never shown");
        c.check(!core.state().signed_in, "still signed in");
        c.check(core.account() != before, "the account count did not move");
        c.check(store.load().unwrap().is_none(), "tokens kept");
        c.check(keys.load().unwrap().is_none(), "its MCP key is still there");
        for kind in [Kind::Ordinary, Kind::Background] {
            c.check(
                ssh.load("a", kind).unwrap().is_none(),
                format!("its {kind:?} SSH token is still there"),
            );
        }
        c.done();
    }

    #[tokio::test]
    async fn a_refresh_401_during_agent_status_ends_the_account_on_its_own() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/machines/m1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(machine_json()))
            .mount(&server)
            .await;
        // The status exec gets 401, and so does the refresh it triggers.
        Mock::given(method("POST"))
            .and(path("/v1/machines/m1/exec"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/refresh"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let (core, store, keys, ssh, signed_out) = core_with_session(&server.uri(), tokens());
        let before = core.account();
        // The command turns the failed exec into "not signed in" and settles nothing.
        assert_eq!(core.agent_status("m1", Agent::Codex).await, Ok(false));
        assert_account_ended(&core, before, (&store, &keys, &ssh), &signed_out).await;
    }

    #[tokio::test]
    async fn a_replaced_sessions_late_401_leaves_the_new_session_alone() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        let arrived = Arc::new(tokio::sync::Notify::new());
        let (go, held) = std::sync::mpsc::channel();
        Mock::given(method("POST"))
            .and(path("/v1/auth/refresh"))
            .respond_with(HeldRefresh {
                arrived: arrived.clone(),
                go: Mutex::new(held),
                reply: ResponseTemplate::new(401),
            })
            .mount(&server)
            .await;
        let (core, store, keys, ssh, signed_out) =
            core_with_session(&server.uri(), tokens_of("iba_a", 5));
        let watcher_ran = Arc::new(tokio::sync::Notify::new());
        let w = watcher_ran.clone();
        at_step(&core, "session ended", move |_| w.notify_one());
        // A call of account A refreshes; the server holds the answer.
        let a = core.lock().session.clone().unwrap();
        let call = tokio::spawn(async move { a.bearer().await });
        arrived.notified().await;
        // Account B signs in, then gets its own MCP key and SSH token.
        let b = device_session(store.clone(), "iba_b").await;
        let after_b = core.install_signed_in(b, core.account()).unwrap();
        keys.save("ibk_b").unwrap();
        ssh.save("b1", Kind::Ordinary, &ssh_token()).unwrap();
        // A's watcher saw A end with the replacement.
        let ran = tokio::time::timeout(Duration::from_secs(3), watcher_ran.notified()).await;
        assert!(ran.is_ok(), "A's watcher never ran");
        // A's refresh gets its 401 only now.
        go.send(()).unwrap();
        assert!(call.await.unwrap().is_err());
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        let shown = tokio::time::timeout(Duration::from_millis(200), signed_out.notified()).await;
        let token = core.lock().session.clone().unwrap().current_access_token();
        let saved = store.load().unwrap().map(|t| t.access_token);
        let mut c = Checks::default();
        c.check(shown.is_err(), "the sign-in screen was shown");
        c.check(core.state().signed_in, "B was signed out");
        c.check(core.account() == after_b, "the account count moved");
        c.check(
            token.as_deref() == Some("iba_b"),
            format!("session has {token:?}"),
        );
        c.check(
            saved.as_deref() == Some("iba_b"),
            format!("store holds {saved:?}"),
        );
        c.check(keys.load().unwrap().is_some(), "B's MCP key is gone");
        c.check(
            ssh.load("b1", Kind::Ordinary).unwrap().is_some(),
            "B's SSH token is gone",
        );
        c.done();
    }

    #[tokio::test]
    async fn a_refresh_401_during_a_background_refresh_ends_the_account_on_its_own() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/auth/refresh"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        // The token is about to expire: the machines read refreshes first and gets 401.
        let mut tok = tokens();
        tok.access_expires_at = Utc::now() + chrono::Duration::seconds(5);
        let (core, store, keys, ssh, signed_out) = core_with_session(&server.uri(), tok);
        let before = core.account();
        // What the refresh loop runs; no command is running.
        core.refresh(false).await;
        assert_account_ended(&core, before, (&store, &keys, &ssh), &signed_out).await;
    }

    // ---- Orca: an SSH host the app imports ----

    /// The Orca app the test computer has.
    #[derive(Clone, Copy, PartialEq)]
    enum OrcaHere {
        /// Orca's identifier and a signature that passes.
        Real,
        /// Orca's identifier, but the signature check fails.
        BadSignature,
        Missing,
    }

    /// A signed-in core with a recording opener, its own home and an Orca.app as `here` says.
    /// The app is a real bundle on disk, checked by the production identifier check; only
    /// codesign is replaced by `here`. Its token store keeps the hosts in step, as in the app.
    struct OrcaCore {
        core: Arc<Core>,
        opened: Arc<Mutex<Vec<String>>>,
        app: PathBuf,
        home: tempfile::TempDir,
        _dir: tempfile::TempDir,
    }

    fn orca_core(base: &Url, here: OrcaHere) -> OrcaCore {
        let dir = tempfile::tempdir().unwrap();
        let app = ade::orca::test_bundle(dir.path(), ade::orca::BUNDLE_ID, "1.4.269");
        orca_core_with(
            base,
            dir,
            app,
            here != OrcaHere::Missing,
            Arc::new(move |ade, path| {
                ade == Ade::Orca && ade::orca::verify_with(path, |_| here != OrcaHere::BadSignature)
            }),
        )
    }

    /// `orca_core` with the app at `app` (found when `found`) and the check `verify`.
    fn orca_core_with(
        base: &Url,
        dir: tempfile::TempDir,
        app: PathBuf,
        found: bool,
        verify: Verifier,
    ) -> OrcaCore {
        let home = tempfile::tempdir().unwrap();
        let (core, opened) = recording_core(
            base.clone(),
            Arc::new(MemoryKeyStore::default()),
            Arc::new(ssh_config::SyncedStore::new(
                MemorySshStore::default(),
                home.path().to_path_buf(),
            )),
            home.path().to_path_buf(),
        );
        let at = app.clone();
        let core = Arc::new(core.with_apps(
            Arc::new(move |ade| (found && ade == Ade::Orca).then(|| at.clone())),
            verify,
        ));
        sign_in_as(&core, base, 7);
        OrcaCore {
            core,
            opened,
            app,
            home,
            _dir: dir,
        }
    }

    const ORCA_NOTICE: &str = "In Orca, open Settings, then SSH Hosts. Click Import if \
                               instabox-dev-1 is not listed, then connect it and open /data/home.";

    fn hosts_of(home: &Path) -> String {
        std::fs::read_to_string(ssh_config::hosts_path(home)).unwrap_or_default()
    }

    #[tokio::test]
    async fn open_in_orca_writes_the_host_and_starts_the_verified_app() {
        let (server, base) =
            cmux_server(serde_json::json!({"instabox-bridge": "orca", "instabox-ades": "orca"}))
                .await;
        let o = orca_core(&base, OrcaHere::Real);
        o.core.refresh(false).await;
        let notice = o.core.open_in_ade("m1", Some(Ade::Orca)).await.unwrap();
        assert_eq!(notice.as_deref(), Some(ORCA_NOTICE));
        // The app itself starts, with no link: Orca imports the host from the SSH config.
        assert_eq!(*o.opened.lock().unwrap(), [o.app.display().to_string()]);
        let hosts = hosts_of(o.home.path());
        assert!(
            hosts.contains(
                "# instabox-sandbox m1\nHost instabox-dev-1\n  HostName ssh.example.test\n  \
                 Port 2222\n  User ibssh_new\n  StrictHostKeyChecking accept-new\n  \
                 ForwardAgent no\n"
            ),
            "{hosts}"
        );
        let cfg = std::fs::read_to_string(ssh_config::config_path(o.home.path())).unwrap();
        assert_eq!(cfg, format!("{}\n", ssh_config::INCLUDE));
        o.core.probes_settled().await;
        let r = requests(&server).await;
        assert!(!r.iter().any(|x| x.starts_with("PATCH")), "{r:?}");
        assert_eq!(r.iter().filter(|x| x.ends_with("/ssh-tokens")).count(), 1);
        // On the sandbox, Open only brings the probe up to date, in the background, and
        // records its hash.
        let probe = ade::orca::OPEN_PROBE;
        assert_eq!(exec_cmds(&server).await, [probe.script()]);
        let key = ("m1".to_string(), Ade::Orca);
        assert_eq!(
            o.core.lock().data.open_probes.get(&key),
            Some(&probe.hash())
        );
        // Again: the same host, the stored token, the app started once more.
        let again = o.core.open_in_ade("m1", Some(Ade::Orca)).await.unwrap();
        assert_eq!(again.as_deref(), Some(ORCA_NOTICE));
        assert_eq!(hosts_of(o.home.path()), hosts);
        assert_eq!(o.opened.lock().unwrap().len(), 2);
        o.core.probes_settled().await;
        let r = requests(&server).await;
        assert_eq!(r.iter().filter(|x| x.ends_with("/ssh-tokens")).count(), 1);
        // The same probe hash on record: nothing runs on the sandbox.
        assert_eq!(exec_cmds(&server).await.len(), 1);
        // Orca pairs nothing, so the sandbox does not join the T3 pairings from here.
        assert!(!o.core.lock().data.connected.contains("m1"));
    }

    #[tokio::test]
    async fn open_in_orca_of_a_sandbox_without_it_installs_the_probe_then_marks_it() {
        let (server, base) = cmux_server(serde_json::json!({"instabox-bridge": "t3"})).await;
        let o = orca_core(&base, OrcaHere::Real);
        o.core
            .update(|i| i.state.set_desktop(Ade::Orca, Some("1.4.269".into())));
        o.core.refresh(false).await;
        let notice = o.core.open_in_ade("m1", Some(Ade::Orca)).await.unwrap();
        assert_eq!(notice.as_deref(), Some(ORCA_NOTICE));
        assert_eq!(*o.opened.lock().unwrap(), [o.app.display().to_string()]);
        o.core.probes_settled().await;
        let reqs = server.received_requests().await.unwrap();
        // Orca installs its own server: the one command is the setup with its activity probe.
        // This Open went through Install, so no install in the background follows it.
        let execs: Vec<_> = reqs
            .iter()
            .enumerate()
            .filter(|(_, r)| r.url.path().ends_with("/exec"))
            .collect();
        assert_eq!(execs.len(), 1, "one exec");
        let (at, exec) = execs[0];
        let body: serde_json::Value = serde_json::from_slice(&exec.body).unwrap();
        assert_eq!(body["cmd"], ade::orca::setup_script("1.4.269"));
        let mark = reqs
            .iter()
            .position(|r| r.method.as_str() == "PATCH")
            .unwrap();
        assert!(at < mark, "the mark comes after the setup");
        let patch = &reqs[mark];
        let body: serde_json::Value = serde_json::from_slice(&patch.body).unwrap();
        assert_eq!(body["metadata"]["instabox-bridge"], "t3");
        assert_eq!(body["metadata"]["instabox-ades"], "t3,orca");
        assert!(hosts_of(o.home.path()).contains("Host instabox-dev-1\n"));
        let s = o.core.state();
        assert_eq!((s.progress, s.error), (None, None));
        assert!(!o.core.lock().data.connected.contains("m1"));
    }

    #[tokio::test]
    async fn a_new_sandbox_in_orca_mode_gets_its_host_and_the_notice() {
        let (_server, base) = cmux_server(serde_json::json!({})).await;
        let o = orca_core(&base, OrcaHere::Real);
        o.core
            .update(|i| i.state.set_desktop(Ade::Orca, Some("1.4.269".into())));
        let notice = o
            .core
            .create_sandbox(CreateArgs {
                name: "dev 1".into(),
                snapshot_id: None,
                vcpus: 2,
                mem_mib: 4096,
                auto_suspend_after_seconds: Some(1800),
                ade: Some(Ade::Orca),
            })
            .await
            .unwrap();
        assert_eq!(notice.as_deref(), Some(ORCA_NOTICE));
        assert!(hosts_of(o.home.path()).contains("Host instabox-dev-1\n"));
    }

    #[tokio::test]
    async fn orca_gets_no_host_and_no_token_without_its_verified_app() {
        for (here, message) in [
            (
                OrcaHere::Missing,
                "Orca is not installed. Install it first.",
            ),
            (
                OrcaHere::BadSignature,
                "Could not verify the Orca app. Install it again from its official download.",
            ),
        ] {
            let (server, base) = cmux_server(
                serde_json::json!({"instabox-bridge": "orca", "instabox-ades": "orca"}),
            )
            .await;
            let o = orca_core(&base, here);
            o.core.refresh(false).await;
            let e = o.core.open_in_ade("m1", Some(Ade::Orca)).await.unwrap_err();
            assert_eq!(e.message, message);
            assert!(o.opened.lock().unwrap().is_empty(), "nothing started");
            let r = requests(&server).await;
            assert!(!r.iter().any(|x| x.ends_with("/ssh-tokens")), "{r:?}");
            assert!(
                !o.home.path().join(".ssh").exists(),
                "no SSH config written"
            );
        }
    }

    #[tokio::test]
    async fn deleting_a_sandbox_removes_its_orca_host_only() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};
        let (server, base) =
            cmux_server(serde_json::json!({"instabox-bridge": "orca", "instabox-ades": "orca"}))
                .await;
        Mock::given(method("DELETE"))
            .and(path("/v1/machines/m1"))
            .respond_with(ResponseTemplate::new(202).set_body_json(serde_json::json!({
                "id": "op9", "machineId": "m1", "type": "delete", "state": "succeeded",
                "machine": null
            })))
            .mount(&server)
            .await;
        let o = orca_core(&base, OrcaHere::Real);
        o.core.refresh(false).await;
        o.core.open_in_ade("m1", Some(Ade::Orca)).await.unwrap();
        // Another sandbox's host, and a block of the user's own.
        ssh_config::write_host(o.home.path(), "m2", "web", &ssh_token()).unwrap();
        let path = ssh_config::hosts_path(o.home.path());
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str("\nHost mine\n  User me\n");
        std::fs::write(&path, text).unwrap();
        o.core.delete("m1").await.unwrap();
        let hosts = hosts_of(o.home.path());
        assert!(!hosts.contains("instabox-dev-1"), "{hosts}");
        assert!(hosts.contains("Host instabox-web\n"), "{hosts}");
        assert!(hosts.contains("Host mine\n  User me\n"), "{hosts}");
    }

    #[tokio::test]
    async fn a_new_ordinary_token_leaves_the_orca_host_and_a_new_background_one_rewrites_it() {
        let (_server, base) =
            cmux_server(serde_json::json!({"instabox-bridge": "orca", "instabox-ades": "orca"}))
                .await;
        let home = tempfile::tempdir().unwrap();
        // The store the app uses: the keychain, wrapped to keep the hosts in step.
        let store =
            ssh_config::SyncedStore::new(MemorySshStore::default(), home.path().to_path_buf());
        let soon = chrono::Duration::hours(2);
        let old = token_left("ibssh_old", soon);
        let bg = token_left("ibssh_bg", soon);
        ssh_config::write_host(home.path(), "m1", "dev 1", &bg).unwrap();
        store.save("m1", Kind::Ordinary, &old).unwrap();
        store.save("m1", Kind::Background, &bg).unwrap();
        let o = orca_core(&base, OrcaHere::Real);
        let client = o.core.client().unwrap();
        // The SSH command or an agent sign-in asks for a token, and the stored one runs out soon.
        let open = &crate::gate::Open;
        let t = ssh::token(&client, &store, "m1", Kind::Ordinary, open)
            .await
            .unwrap();
        assert_eq!((t.token.as_str(), t.background), ("ibssh_new", false));
        let hosts = hosts_of(home.path());
        assert!(hosts.contains("  User ibssh_bg\n"), "{hosts}");
        assert!(!hosts.contains("ibssh_new"), "{hosts}");
        // A new background token, as Open in Orca or the renewal makes, goes into the host.
        let t = ssh::token(&client, &store, "m1", Kind::Background, open)
            .await
            .unwrap();
        assert_eq!((t.token.as_str(), t.background), ("ibssh_new", true));
        let hosts = hosts_of(home.path());
        assert!(hosts.contains("Host instabox-dev-1\n"), "{hosts}");
        assert!(hosts.contains("  User ibssh_new\n"), "{hosts}");
        assert!(!hosts.contains("ibssh_bg"), "{hosts}");
    }

    // ---- Orca: renewing the hosts' tokens ----

    /// A background token of `m1` named `token` with `left` until it runs out.
    fn token_left(token: &str, left: chrono::Duration) -> crate::api::SshToken {
        crate::api::SshToken {
            token: token.into(),
            command: format!("ssh {token}@ssh.example.test"),
            expires_at: Some(Utc::now() + left),
            background: true,
            ..ssh_token()
        }
    }

    /// Writes the host of `id` with `t` and keeps `t` as the core's background token.
    fn host_with(o: &OrcaCore, id: &str, t: &crate::api::SshToken) {
        ssh_config::write_host(o.home.path(), id, "dev 1", t).unwrap();
        o.core.ssh.save(id, Kind::Background, t).unwrap();
    }

    fn token_requests(r: &[String]) -> usize {
        r.iter().filter(|x| x.ends_with("/ssh-tokens")).count()
    }

    #[tokio::test]
    async fn a_host_token_with_under_7_days_left_is_renewed_and_its_user_rewritten() {
        let (server, base) = cmux_server(serde_json::json!({})).await;
        let o = orca_core(&base, OrcaHere::Real);
        let old = token_left(
            "ibssh_old",
            chrono::Duration::days(7) - chrono::Duration::hours(1),
        );
        host_with(&o, "m1", &old);
        o.core.renew_ssh_hosts(None).await;
        let hosts = hosts_of(o.home.path());
        assert!(hosts.contains("  User ibssh_new\n"), "{hosts}");
        assert!(!hosts.contains("ibssh_old"), "{hosts}");
        let kept = o.core.ssh.load("m1", Kind::Background).unwrap().unwrap();
        assert_eq!((kept.token.as_str(), kept.background), ("ibssh_new", true));
        assert_eq!(o.core.ssh.load("m1", Kind::Ordinary).unwrap(), None);
        let r = requests(&server).await;
        assert_eq!(token_requests(&r), 1, "{r:?}");
        // A token is an API call only: the sandbox is not read, woken or run on.
        assert_eq!(r, ["POST /v1/machines/m1/ssh-tokens"], "{r:?}");
        assert_eq!(o.core.state().error, None);
        // The new token has 30 days: the next pass leaves it alone.
        o.core.renew_ssh_hosts(None).await;
        assert_eq!(token_requests(&requests(&server).await), 1);
    }

    #[tokio::test]
    async fn a_host_token_with_7_days_or_more_left_is_left_alone() {
        let (server, base) = cmux_server(serde_json::json!({})).await;
        let o = orca_core(&base, OrcaHere::Real);
        let kept = token_left(
            "ibssh_kept",
            chrono::Duration::days(7) + chrono::Duration::hours(1),
        );
        host_with(&o, "m1", &kept);
        let before = hosts_of(o.home.path());
        o.core.renew_ssh_hosts(None).await;
        assert_eq!(hosts_of(o.home.path()), before);
        assert!(requests(&server).await.is_empty());
    }

    #[tokio::test]
    async fn a_host_whose_token_is_missing_gets_a_new_one() {
        let (server, base) = cmux_server(serde_json::json!({})).await;
        let o = orca_core(&base, OrcaHere::Real);
        // The host is there, the keychain lost the token.
        let gone = token_left("ibssh_lost", chrono::Duration::days(20));
        ssh_config::write_host(o.home.path(), "m1", "dev 1", &gone).unwrap();
        o.core.renew_ssh_hosts(None).await;
        assert!(hosts_of(o.home.path()).contains("  User ibssh_new\n"));
        assert_eq!(token_requests(&requests(&server).await), 1);
    }

    #[tokio::test]
    async fn a_sandbox_without_a_host_gets_no_renewal() {
        let (server, base) = cmux_server(serde_json::json!({})).await;
        let o = orca_core(&base, OrcaHere::Real);
        // Tokens about to run out, with no host: the SSH command renews the ordinary one, and
        // Open the background one, when they need it.
        let old = token_left("ibssh_old", chrono::Duration::hours(30));
        for kind in [Kind::Ordinary, Kind::Background] {
            o.core.ssh.save("m1", kind, &old).unwrap();
        }
        o.core.renew_ssh_hosts(None).await;
        assert!(requests(&server).await.is_empty());
        for kind in [Kind::Ordinary, Kind::Background] {
            let stored = o.core.ssh.load("m1", kind).unwrap().unwrap();
            assert_eq!(stored.token, "ibssh_old");
        }
        assert!(!o.home.path().join(".ssh").exists());
    }

    #[tokio::test]
    async fn no_renewal_while_signed_out() {
        let (server, base) = cmux_server(serde_json::json!({})).await;
        let o = orca_core(&base, OrcaHere::Real);
        let old = token_left("ibssh_old", chrono::Duration::days(1));
        host_with(&o, "m1", &old);
        let before = hosts_of(o.home.path());
        // Signed out without the store's sign-out, so the host is still there to tempt it.
        o.core.update(|i| i.client = None);
        o.core.renew_ssh_hosts(None).await;
        assert_eq!(hosts_of(o.home.path()), before);
        assert!(requests(&server).await.is_empty());
    }

    #[tokio::test]
    async fn no_renewal_lands_after_an_account_change() {
        let (_server, base) = cmux_server(serde_json::json!({})).await;
        let o = orca_core(&base, OrcaHere::Real);
        host_with(
            &o,
            "m1",
            &token_left("ibssh_old", chrono::Duration::days(1)),
        );
        let b = base.clone();
        at_step(&o.core, "renew", move |c| sign_in_as(c, &b, 8));
        let before = o.core.account();
        o.core.renew_ssh_hosts(None).await;
        assert_ne!(
            o.core.account(),
            before,
            "the switch happened during the pass"
        );
        // The account change removed the hosts; the old account's pass wrote nothing after it.
        let hosts = hosts_of(o.home.path());
        assert!(!hosts.contains("ibssh_new"), "{hosts}");
        assert!(!hosts.contains("Host instabox-dev-1"), "{hosts}");
        assert_eq!(o.core.ssh.load("m1", Kind::Background).unwrap(), None);
    }

    #[tokio::test]
    async fn a_failed_renewal_shows_no_error_and_runs_again_on_the_next_pass() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};
        let (server, base) = cmux_server(serde_json::json!({})).await;
        Mock::given(method("POST"))
            .and(path("/v1/machines/m1/ssh-tokens"))
            .respond_with(ResponseTemplate::new(500))
            .up_to_n_times(1)
            .with_priority(1)
            .mount(&server)
            .await;
        let o = orca_core(&base, OrcaHere::Real);
        let old = token_left("ibssh_old", chrono::Duration::days(2));
        host_with(&o, "m1", &old);
        let before = hosts_of(o.home.path());
        o.core.renew_ssh_hosts(None).await;
        assert_eq!(
            hosts_of(o.home.path()),
            before,
            "the host keeps the old token"
        );
        assert_eq!(
            o.core.state().error,
            None,
            "a background renewal shows nothing"
        );
        o.core.renew_ssh_hosts(None).await;
        assert!(hosts_of(o.home.path()).contains("  User ibssh_new\n"));
        assert_eq!(token_requests(&requests(&server).await), 2);
    }

    #[tokio::test]
    async fn a_sandbox_the_api_no_longer_has_loses_its_host() {
        let (server, base) = cmux_server(serde_json::json!({})).await;
        let o = orca_core(&base, OrcaHere::Real);
        // m9 is gone: the mock answers 404 for its token and for the sandbox.
        host_with(
            &o,
            "m9",
            &token_left("ibssh_old9", chrono::Duration::days(1)),
        );
        host_with(
            &o,
            "m1",
            &token_left("ibssh_old", chrono::Duration::days(1)),
        );
        o.core.renew_ssh_hosts(None).await;
        let hosts = hosts_of(o.home.path());
        assert!(!hosts.contains("instabox-sandbox m9"), "{hosts}");
        assert!(hosts.contains("# instabox-sandbox m1\n"), "{hosts}");
        assert!(hosts.contains("  User ibssh_new\n"), "{hosts}");
        let r = requests(&server).await;
        assert!(r.contains(&"GET /v1/machines/m9".to_string()), "{r:?}");
        assert_eq!(o.core.state().error, None);
    }

    #[tokio::test]
    async fn a_token_404_alone_keeps_the_host() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};
        let (server, base) = cmux_server(serde_json::json!({})).await;
        // The token call fails with 404, but the sandbox is still there.
        Mock::given(method("POST"))
            .and(path("/v1/machines/m1/ssh-tokens"))
            .respond_with(ResponseTemplate::new(404))
            .with_priority(1)
            .mount(&server)
            .await;
        let o = orca_core(&base, OrcaHere::Real);
        host_with(
            &o,
            "m1",
            &token_left("ibssh_old", chrono::Duration::days(1)),
        );
        let before = hosts_of(o.home.path());
        o.core.renew_ssh_hosts(None).await;
        assert_eq!(hosts_of(o.home.path()), before);
    }

    #[tokio::test]
    async fn one_renewal_pass_at_a_time() {
        let (server, base) = cmux_server(serde_json::json!({})).await;
        let o = orca_core(&base, OrcaHere::Real);
        host_with(
            &o,
            "m1",
            &token_left("ibssh_old", chrono::Duration::days(1)),
        );
        o.core
            .renewing
            .store(true, std::sync::atomic::Ordering::SeqCst);
        o.core.renew_ssh_hosts(None).await;
        assert!(
            requests(&server).await.is_empty(),
            "another pass is running"
        );
        o.core
            .renewing
            .store(false, std::sync::atomic::Ordering::SeqCst);
        o.core.renew_ssh_hosts(None).await;
        assert_eq!(token_requests(&requests(&server).await), 1);
        assert!(
            !o.core.renewing.load(std::sync::atomic::Ordering::SeqCst),
            "a finished pass lets the next one run"
        );
    }

    #[tokio::test]
    async fn a_full_refresh_renews_the_host_tokens_and_a_poll_does_not() {
        let (_server, base) = cmux_server(serde_json::json!({})).await;
        let o = orca_core(&base, OrcaHere::Real);
        host_with(
            &o,
            "m1",
            &token_left("ibssh_old", chrono::Duration::days(1)),
        );
        o.core.refresh_and_sync(false).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(hosts_of(o.home.path()).contains("  User ibssh_old\n"));
        o.core.refresh_and_sync(true).await;
        let deadline = Instant::now() + Duration::from_secs(5);
        while !hosts_of(o.home.path()).contains("  User ibssh_new\n") {
            assert!(Instant::now() < deadline, "{}", hosts_of(o.home.path()));
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    // ---- the app is checked before the sandbox is touched ----

    #[tokio::test]
    async fn orca_install_checks_the_app_before_it_wakes_creates_or_marks() {
        for (here, message) in [
            (
                OrcaHere::Missing,
                "Orca is not installed. Install it first.",
            ),
            (
                OrcaHere::BadSignature,
                "Could not verify the Orca app. Install it again from its official download.",
            ),
        ] {
            let (server, base) = cmux_server(serde_json::json!({})).await;
            let o = orca_core(&base, here);
            // A version found earlier, from an app that is gone or replaced since.
            o.core
                .update(|i| i.state.set_desktop(Ade::Orca, Some("1.4.269".into())));
            let e = o
                .core
                .connect_sandbox("m1", Some(Ade::Orca))
                .await
                .unwrap_err();
            assert_eq!(e.message, message);
            let e = o
                .core
                .create_sandbox(CreateArgs {
                    name: "dev 1".into(),
                    snapshot_id: None,
                    vcpus: 2,
                    mem_mib: 4096,
                    auto_suspend_after_seconds: Some(1800),
                    ade: Some(Ade::Orca),
                })
                .await
                .unwrap_err();
            assert_eq!(e.message, message);
            // Open on a sandbox without Orca goes through the same check.
            let e = o.core.open_in_ade("m1", Some(Ade::Orca)).await.unwrap_err();
            assert_eq!(e.message, message);
            let r = requests(&server).await;
            assert!(
                r.is_empty(),
                "nothing read, woken, created or marked: {r:?}"
            );
            assert_eq!(o.core.state().progress, None);
            assert!(o.opened.lock().unwrap().is_empty());
            assert!(!o.home.path().join(".ssh").exists());
        }
    }

    #[tokio::test]
    async fn cmux_install_checks_the_app_before_it_wakes_or_marks() {
        let (server, base) = cmux_server(serde_json::json!({})).await;
        let CmuxCore {
            core,
            opened,
            app: _,
            _dir,
        } = cmux_core(
            &base,
            Arc::new(MemorySshStore::default()),
            CmuxHere::BadSignature,
        );
        core.update(|i| i.state.set_desktop(Ade::Cmux, Some("0.64.25".into())));
        let e = core
            .connect_sandbox("m1", Some(Ade::Cmux))
            .await
            .unwrap_err();
        assert_eq!(
            e.message,
            "Could not verify the cmux app. Install it again from its official download."
        );
        let r = requests(&server).await;
        assert!(r.is_empty(), "no prep, no mark: {r:?}");
        assert!(opened.lock().unwrap().is_empty());
    }

    // ---- Paseo: an SSH host the user adds by its address, to a daemon InstaADE runs ----

    /// A signed-in core like `orca_core`, with a Paseo.app as `here` says, Paseo 0.17.4 found
    /// earlier, and a clipboard that records what was copied (or refuses every copy, as there
    /// is no clipboard on Windows yet).
    struct PaseoCore {
        core: Arc<Core>,
        opened: Arc<Mutex<Vec<String>>>,
        copied: Arc<Mutex<Vec<String>>>,
        app: PathBuf,
        home: tempfile::TempDir,
        _dir: tempfile::TempDir,
    }

    /// A clipboard that takes nothing.
    struct NoClipboard;

    impl crate::clipboard::Clipboard for NoClipboard {
        fn read(&self) -> Option<String> {
            None
        }
        fn write(&self, _: &str) -> Result<(), String> {
            Err("Copying to the clipboard is only on macOS for now".to_string())
        }
    }

    fn paseo_core(base: &Url, here: OrcaHere, clipboard: bool) -> PaseoCore {
        let dir = tempfile::tempdir().unwrap();
        let app = ade::paseo::test_bundle(dir.path(), ade::paseo::BUNDLE_ID, "0.17.4");
        let home = tempfile::tempdir().unwrap();
        let (core, opened) = recording_core(
            base.clone(),
            Arc::new(MemoryKeyStore::default()),
            Arc::new(ssh_config::SyncedStore::new(
                MemorySshStore::default(),
                home.path().to_path_buf(),
            )),
            home.path().to_path_buf(),
        );
        let copied = Arc::new(Mutex::new(Vec::<String>::new()));
        let board: Arc<dyn crate::clipboard::Clipboard> = if clipboard {
            Arc::new(RecordingClipboard(copied.clone()))
        } else {
            Arc::new(NoClipboard)
        };
        let found = app.clone();
        let core = Arc::new(
            core.with_apps(
                Arc::new(move |ade| {
                    (here != OrcaHere::Missing && ade == Ade::Paseo).then(|| found.clone())
                }),
                Arc::new(move |ade, path| {
                    ade == Ade::Paseo
                        && ade::paseo::verify_with(path, |_| here != OrcaHere::BadSignature)
                }),
            )
            .with_clipboard(SecretClipboard::new(
                board,
                Arc::new(|_| Box::pin(std::future::pending())),
            )),
        );
        sign_in_as(&core, base, 7);
        core.update(|i| i.state.set_desktop(Ade::Paseo, Some("0.17.4".into())));
        PaseoCore {
            core,
            opened,
            copied,
            app,
            home,
            _dir: dir,
        }
    }

    const PASEO_NOTICE: &str = "In Paseo, finish or skip its setup if it shows one. Then add \
                                this sandbox once: open Settings, click Switch host, Add host, \
                                then Remote SSH, and enter ssh://instabox-dev-1.";

    /// The commands the sandbox ran.
    async fn exec_cmds(server: &wiremock::MockServer) -> Vec<String> {
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|r| r.url.path().ends_with("/exec"))
            .map(|r| {
                let body: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
                body["cmd"].as_str().unwrap().to_string()
            })
            .collect()
    }

    #[tokio::test]
    async fn installing_paseo_sets_up_its_daemon_then_writes_the_host_and_copies_its_address() {
        let (server, base) = cmux_server(serde_json::json!({"instabox-bridge": "t3"})).await;
        let p = paseo_core(&base, OrcaHere::Real, true);
        p.core.refresh(false).await;
        let notice = p.core.open_in_ade("m1", Some(Ade::Paseo)).await.unwrap();
        assert_eq!(notice.as_deref(), Some(PASEO_NOTICE));
        // The setup at the desktop's version, then the mark, then the host and the app.
        assert_eq!(
            exec_cmds(&server).await,
            [ade::paseo::setup_script("0.17.4")]
        );
        let r = requests(&server).await;
        let exec = r.iter().position(|x| x.ends_with("/exec")).unwrap();
        let patch = r.iter().position(|x| x.starts_with("PATCH")).unwrap();
        let token = r.iter().position(|x| x.ends_with("/ssh-tokens")).unwrap();
        assert!(exec < patch && patch < token, "{r:?}");
        let reqs = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(
            &reqs
                .iter()
                .find(|r| r.method.as_str() == "PATCH")
                .unwrap()
                .body,
        )
        .unwrap();
        assert_eq!(body["metadata"]["instabox-bridge"], "t3");
        assert_eq!(body["metadata"]["instabox-ades"], "t3,paseo");
        assert!(
            hosts_of(p.home.path()).contains(
                "# instabox-sandbox m1\nHost instabox-dev-1\n  HostName ssh.example.test\n  \
                 Port 2222\n  User ibssh_new\n"
            ),
            "{}",
            hosts_of(p.home.path())
        );
        // The address holds no token: ssh takes the login from the host.
        assert_eq!(*p.copied.lock().unwrap(), ["ssh://instabox-dev-1"]);
        assert_eq!(*p.opened.lock().unwrap(), [p.app.display().to_string()]);
        let s = p.core.state();
        assert_eq!((s.progress, s.error), (None, None));
        // Paseo pairs nothing, so the sandbox does not join the T3 pairings from here.
        assert!(!p.core.lock().data.connected.contains("m1"));
    }

    #[tokio::test]
    async fn open_in_paseo_installs_only_the_probe_and_copies_the_address_again() {
        let (server, base) =
            cmux_server(serde_json::json!({"instabox-bridge": "paseo", "instabox-ades": "paseo"}))
                .await;
        let p = paseo_core(&base, OrcaHere::Real, true);
        p.core.refresh(false).await;
        for _ in 0..2 {
            let notice = p.core.open_in_ade("m1", Some(Ade::Paseo)).await.unwrap();
            assert_eq!(notice.as_deref(), Some(PASEO_NOTICE));
        }
        p.core.probes_settled().await;
        // The daemon runs already: no setup, only the probe, once.
        assert_eq!(exec_cmds(&server).await, [ade::paseo::OPEN_PROBE.script()]);
        let r = requests(&server).await;
        assert!(!r.iter().any(|x| x.starts_with("PATCH")), "{r:?}");
        assert_eq!(r.iter().filter(|x| x.ends_with("/ssh-tokens")).count(), 1);
        assert!(hosts_of(p.home.path()).contains("Host instabox-dev-1\n"));
        assert_eq!(
            *p.copied.lock().unwrap(),
            ["ssh://instabox-dev-1", "ssh://instabox-dev-1"]
        );
        assert_eq!(p.opened.lock().unwrap().len(), 2);
    }

    /// The probe hash Open recorded for sandbox m1 in Paseo.
    fn open_probe_of(p: &PaseoCore) -> Option<String> {
        p.core
            .lock()
            .data
            .open_probes
            .get(&("m1".to_string(), Ade::Paseo))
            .cloned()
    }

    #[tokio::test]
    async fn opening_paseo_installs_its_probe_once_per_sandbox_and_probe_hash() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};
        let (server, base) =
            cmux_server(serde_json::json!({"instabox-bridge": "paseo", "instabox-ades": "paseo"}))
                .await;
        let p = paseo_core(&base, OrcaHere::Real, true);
        p.core.refresh(false).await;
        let probe = ade::paseo::OPEN_PROBE;
        // The first Open installs it, and records its hash only once that succeeded.
        assert_eq!(open_probe_of(&p), None);
        p.core.open_in_ade("m1", Some(Ade::Paseo)).await.unwrap();
        p.core.probes_settled().await;
        assert_eq!(exec_cmds(&server).await, [probe.script()]);
        assert_eq!(open_probe_of(&p), Some(probe.hash()));
        // The same hash: nothing runs.
        p.core.open_in_ade("m1", Some(Ade::Paseo)).await.unwrap();
        p.core.probes_settled().await;
        assert_eq!(exec_cmds(&server).await.len(), 1);
        // An older probe on record (an earlier build's): this Open installs it again.
        p.core.update(|i| {
            i.data
                .open_probes
                .insert(("m1".into(), Ade::Paseo), "0000000000000000".into())
        });
        p.core.open_in_ade("m1", Some(Ade::Paseo)).await.unwrap();
        p.core.probes_settled().await;
        assert_eq!(exec_cmds(&server).await, [probe.script(), probe.script()]);
        assert_eq!(open_probe_of(&p), Some(probe.hash()));
        // A deleted sandbox loses its record.
        Mock::given(method("DELETE"))
            .and(path("/v1/machines/m1"))
            .respond_with(ResponseTemplate::new(202).set_body_json(serde_json::json!({
                "id": "op9", "machineId": "m1", "type": "delete", "state": "succeeded",
                "machine": null
            })))
            .mount(&server)
            .await;
        p.core.delete("m1").await.unwrap();
        assert_eq!(open_probe_of(&p), None);
    }

    #[tokio::test]
    async fn a_probe_install_that_fails_or_hangs_neither_slows_nor_fails_open_and_records_nothing()
    {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};
        let (server, base) =
            cmux_server(serde_json::json!({"instabox-bridge": "paseo", "instabox-ades": "paseo"}))
                .await;
        Mock::given(method("POST"))
            .and(path("/v1/machines/m1/exec"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!(
                        {"stdout": "", "stderr": "sudo: a password is required", "exitCode": 1}
                    ))
                    .set_delay(Duration::from_secs(3)),
            )
            .with_priority(1)
            .mount(&server)
            .await;
        let p = paseo_core(&base, OrcaHere::Real, true);
        p.core.refresh(false).await;
        let start = Instant::now();
        let notice = p.core.open_in_ade("m1", Some(Ade::Paseo)).await.unwrap();
        assert!(start.elapsed() < Duration::from_secs(2), "Open waited");
        assert_eq!(notice.as_deref(), Some(PASEO_NOTICE));
        assert_eq!(*p.opened.lock().unwrap(), [p.app.display().to_string()]);
        p.core.probes_settled().await;
        let s = p.core.state();
        assert_eq!((s.error, s.progress), (None, None), "the user sees nothing");
        assert_eq!(open_probe_of(&p), None);
        let logs = p.core.logs.lock().unwrap().clone();
        assert!(
            logs.contains(
                &"Paseo: could not install its idle probe on sandbox m1: exit 1: sudo: a \
                  password is required"
                    .to_string()
            ),
            "{logs:?}"
        );
        // Nothing on record: the next Open tries again.
        p.core.open_in_ade("m1", Some(Ade::Paseo)).await.unwrap();
        p.core.probes_settled().await;
        assert_eq!(exec_cmds(&server).await.len(), 2);
    }

    #[tokio::test]
    async fn no_probe_install_runs_beside_a_setup_of_the_same_sandbox_or_another_install() {
        let (server, base) =
            cmux_server(serde_json::json!({"instabox-bridge": "paseo", "instabox-ades": "paseo"}))
                .await;
        let p = paseo_core(&base, OrcaHere::Real, true);
        p.core.refresh(false).await;
        let account = p.core.account();
        let setup = |id: &str, error: Option<ErrorView>| ProgressView {
            kind: "connect".into(),
            step: "installing".into(),
            machine_id: Some(id.into()),
            ade: Some(Ade::Paseo),
            error,
        };
        // A setup of this sandbox runs: it installs the probe itself.
        p.core
            .update(|i| i.state.progress = Some(setup("m1", None)));
        p.core.refresh_probe(Ade::Paseo, "m1", account);
        p.core.probes_settled().await;
        assert!(exec_cmds(&server).await.is_empty());
        // A failed setup, or one of another sandbox, runs no longer.
        for progress in [setup("m1", Some(ErrorView::other("x"))), setup("m2", None)] {
            p.core.update(|i| {
                i.state.progress = Some(progress);
                i.data.open_probes.clear();
            });
            p.core.refresh_probe(Ade::Paseo, "m1", account);
            p.core.probes_settled().await;
        }
        assert_eq!(exec_cmds(&server).await.len(), 2);
        // One install at a time per sandbox and ADE.
        p.core.update(|i| i.data.open_probes.clear());
        p.core
            .probing
            .lock()
            .unwrap()
            .insert(("m1".into(), Ade::Paseo));
        p.core.refresh_probe(Ade::Paseo, "m1", account);
        p.core.probes_settled().await;
        assert_eq!(exec_cmds(&server).await.len(), 2);
        // An ADE with no probe for Open, or another account's flow, runs nothing.
        p.core.probing.lock().unwrap().clear();
        p.core.refresh_probe(Ade::Cmux, "m1", account);
        p.core.refresh_probe(Ade::Paseo, "m1", account + 1);
        p.core.probes_settled().await;
        assert_eq!(exec_cmds(&server).await.len(), 2);
    }

    #[tokio::test]
    async fn a_new_sandbox_in_paseo_mode_gets_the_daemon_the_host_and_the_notice() {
        let (server, base) = cmux_server(serde_json::json!({})).await;
        let p = paseo_core(&base, OrcaHere::Real, true);
        let notice = p
            .core
            .create_sandbox(CreateArgs {
                name: "dev 1".into(),
                snapshot_id: None,
                vcpus: 2,
                mem_mib: 4096,
                auto_suspend_after_seconds: Some(1800),
                ade: Some(Ade::Paseo),
            })
            .await
            .unwrap();
        assert_eq!(notice.as_deref(), Some(PASEO_NOTICE));
        assert_eq!(
            exec_cmds(&server).await,
            [ade::paseo::setup_script("0.17.4")]
        );
        assert!(hosts_of(p.home.path()).contains("Host instabox-dev-1\n"));
        assert_eq!(*p.copied.lock().unwrap(), ["ssh://instabox-dev-1"]);
    }

    #[tokio::test]
    async fn a_copy_that_fails_is_logged_and_paseo_still_opens() {
        let (_server, base) =
            cmux_server(serde_json::json!({"instabox-bridge": "paseo", "instabox-ades": "paseo"}))
                .await;
        let p = paseo_core(&base, OrcaHere::Real, false);
        p.core.refresh(false).await;
        let notice = p.core.open_in_ade("m1", Some(Ade::Paseo)).await.unwrap();
        // The notice names the address, so the user can type it.
        assert_eq!(notice.as_deref(), Some(PASEO_NOTICE));
        assert_eq!(*p.opened.lock().unwrap(), [p.app.display().to_string()]);
        assert_eq!(p.core.state().error, None);
        let logs = p.core.logs.lock().unwrap().clone();
        assert!(
            logs.contains(
                &"Paseo: the host address was not copied: Copying to the clipboard is only on \
                  macOS for now"
                    .to_string()
            ),
            "{logs:?}"
        );
    }

    #[tokio::test]
    async fn paseo_gets_no_setup_host_token_or_copy_without_its_verified_app() {
        for (here, message) in [
            (
                OrcaHere::Missing,
                "Paseo is not installed. Install it first.",
            ),
            (
                OrcaHere::BadSignature,
                "Could not verify the Paseo app. Install it again from its official download.",
            ),
        ] {
            for metadata in [
                serde_json::json!({}),
                serde_json::json!({"instabox-bridge": "paseo", "instabox-ades": "paseo"}),
            ] {
                let (server, base) = cmux_server(metadata).await;
                let p = paseo_core(&base, here, true);
                p.core.refresh(false).await;
                let before = requests(&server).await.len();
                let e = p
                    .core
                    .open_in_ade("m1", Some(Ade::Paseo))
                    .await
                    .unwrap_err();
                assert_eq!(e.message, message);
                let r = requests(&server).await;
                assert!(
                    !r[before..].iter().any(|x| x.ends_with("/exec")
                        || x.ends_with("/ssh-tokens")
                        || x.starts_with("PATCH")),
                    "nothing set up, marked or minted: {r:?}"
                );
                assert!(p.opened.lock().unwrap().is_empty(), "nothing started");
                assert!(p.copied.lock().unwrap().is_empty(), "nothing copied");
                assert!(
                    !p.home.path().join(".ssh").exists(),
                    "no SSH config written"
                );
            }
        }
    }

    // ---- a host that cannot take a new token ----

    #[tokio::test]
    async fn a_host_that_cannot_take_the_new_token_is_logged_and_tried_again() {
        let (server, base) = cmux_server(serde_json::json!({})).await;
        let o = orca_core(&base, OrcaHere::Real);
        host_with(
            &o,
            "m1",
            &token_left("ibssh_old", chrono::Duration::days(1)),
        );
        let path = ssh_config::hosts_path(o.home.path());
        let aside = path.with_extension("aside");
        let (p, a) = (path.clone(), aside.clone());
        // Right after the pass read the hosts, the file cannot be read or written.
        at_step(&o.core, "renew", move |_| {
            std::fs::rename(&p, &a).unwrap();
            std::fs::create_dir(&p).unwrap();
        });
        let logged = o.core.renew_ssh_hosts(None).await;
        assert!(
            logged
                .iter()
                .any(|l| l.contains("could not update the SSH host of sandbox m1")),
            "{logged:?}"
        );
        assert_eq!(
            o.core
                .ssh
                .load("m1", Kind::Background)
                .unwrap()
                .unwrap()
                .token,
            "ibssh_old",
            "the stored token stays behind the host"
        );
        assert_eq!(
            o.core.state().error,
            None,
            "a background renewal shows nothing"
        );
        std::fs::remove_dir(&path).unwrap();
        std::fs::rename(&aside, &path).unwrap();
        assert!(hosts_of(o.home.path()).contains("  User ibssh_old\n"));
        // The next pass makes another token and writes it to the host this time.
        let logged = o.core.renew_ssh_hosts(None).await;
        assert!(logged.is_empty(), "{logged:?}");
        assert!(hosts_of(o.home.path()).contains("  User ibssh_new\n"));
        let kept = o.core.ssh.load("m1", Kind::Background).unwrap().unwrap();
        assert_eq!(kept.token, "ibssh_new");
        assert_eq!(token_requests(&requests(&server).await), 2);
    }

    #[tokio::test]
    async fn a_host_behind_the_stored_token_gets_it_without_a_new_one() {
        let (server, base) = cmux_server(serde_json::json!({})).await;
        let o = orca_core(&base, OrcaHere::Real);
        // The store holds a token with 20 days; the host still has another one.
        o.core
            .ssh
            .save(
                "m1",
                Kind::Background,
                &token_left("ibssh_stored", chrono::Duration::days(20)),
            )
            .unwrap();
        ssh_config::write_host(
            o.home.path(),
            "m1",
            "dev 1",
            &token_left("ibssh_behind", chrono::Duration::days(20)),
        )
        .unwrap();
        let logged = o.core.renew_ssh_hosts(None).await;
        assert!(logged.is_empty(), "{logged:?}");
        let hosts = hosts_of(o.home.path());
        assert!(hosts.contains("  User ibssh_stored\n"), "{hosts}");
        assert!(requests(&server).await.is_empty(), "no new token");
    }

    // ---- the background token and the ordinary one ----

    /// The body that asks for an ordinary token: the one older releases sent.
    fn ordinary_body() -> serde_json::Value {
        serde_json::json!({"name": "Instabox", "ttlSeconds": 2592000})
    }

    fn background_body() -> serde_json::Value {
        serde_json::json!({"name": "Instabox", "ttlSeconds": 2592000, "background": true})
    }

    /// The bodies of the SSH token requests `server` got, in order.
    async fn token_bodies(server: &wiremock::MockServer) -> Vec<serde_json::Value> {
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|r| r.url.path().ends_with("/ssh-tokens"))
            .map(|r| serde_json::from_slice(&r.body).unwrap())
            .collect()
    }

    /// What an older release left for sandbox m1: its host, with the one token it kept for
    /// everything, which is now the ordinary one.
    fn older_release_host(home: &Path, store: &dyn SshTokenStore) -> crate::api::SshToken {
        let old = crate::api::SshToken {
            background: false,
            ..token_left("ibssh_old", chrono::Duration::days(20))
        };
        ssh_config::write_host(home, "m1", "dev 1", &old).unwrap();
        store.save("m1", Kind::Ordinary, &old).unwrap();
        old
    }

    #[tokio::test]
    async fn apps_that_stay_connected_get_the_background_token_and_logins_the_ordinary_one() {
        use wiremock::matchers::{body_json, method, path};
        use wiremock::{Mock, ResponseTemplate};
        let md = serde_json::json!({"instabox-bridge": "cmux", "instabox-ades": "cmux,orca,herdr"});
        let (server, base) = cmux_server(md).await;
        for (body, token, background) in [
            (background_body(), "ibssh_bg", true),
            (ordinary_body(), "ibssh_plain", false),
        ] {
            Mock::given(method("POST"))
                .and(path("/v1/machines/m1/ssh-tokens"))
                .and(body_json(body))
                .respond_with(ResponseTemplate::new(200).set_body_json(minted(token, background)))
                .with_priority(1)
                .mount(&server)
                .await;
        }
        let apps = tempfile::tempdir().unwrap();
        let cmux = ade::cmux::test_bundle(apps.path(), ade::cmux::BUNDLE_ID, "0.64.25", false);
        let orca = ade::orca::test_bundle(apps.path(), ade::orca::BUNDLE_ID, "1.4.269");
        let home = tempfile::tempdir().unwrap();
        let (core, opened) = recording_core(
            base.clone(),
            Arc::new(MemoryKeyStore::default()),
            Arc::new(ssh_config::SyncedStore::new(
                MemorySshStore::default(),
                home.path().to_path_buf(),
            )),
            home.path().to_path_buf(),
        );
        let (c, o) = (cmux.clone(), orca.clone());
        let core = Arc::new(core.with_apps(
            Arc::new(move |ade| match ade {
                Ade::Cmux => Some(c.clone()),
                Ade::Orca => Some(o.clone()),
                Ade::Herdr => Some(PathBuf::from(HERDR_SSH)),
                _ => None,
            }),
            Arc::new(|ade, path| match ade {
                Ade::Cmux => ade::cmux::verify_with(path, |_| true),
                Ade::Orca => ade::orca::verify_with(path, |_| true),
                _ => ade.verify(path),
            }),
        ));
        sign_in_as(&core, &base, 7);
        core.update(|i| {
            i.state
                .set_desktop(Ade::Herdr, Some("OpenSSH 10.3p1".into()))
        });
        core.refresh(false).await;
        for ade in [Ade::Cmux, Ade::Orca, Ade::Herdr] {
            core.open_in_ade("m1", Some(ade)).await.unwrap();
        }
        let command = core.ssh_command("m1").await.unwrap();
        core.open_ssh("m1").await.unwrap();
        core.probes_settled().await;
        assert_eq!(
            *opened.lock().unwrap(),
            [
                format!(
                    "{} in {}",
                    CMUX_LINK.replace("user=ibssh_new", "user=ibssh_bg"),
                    cmux.display()
                ),
                orca.display().to_string(),
                HERDR_TERMINAL.replace("ibssh_new", "ibssh_plain"),
                "terminal: ssh -p 2222 ibssh_plain@ssh.example.test".to_string(),
            ]
        );
        assert_eq!(command, "ssh -p 2222 ibssh_plain@ssh.example.test");
        // Orca's host holds the background token, and the ordinary one never reaches it.
        let hosts = hosts_of(home.path());
        assert!(hosts.contains("Host instabox-dev-1\n"), "{hosts}");
        assert!(hosts.contains("  User ibssh_bg\n"), "{hosts}");
        assert!(!hosts.contains("ibssh_plain"), "{hosts}");
        // One token of each kind, each made once and then reused.
        assert_eq!(
            token_bodies(&server).await,
            [background_body(), ordinary_body()]
        );
        let logs = core.logs.lock().unwrap().clone();
        assert!(
            !logs.iter().any(|l| l.contains("ordinary token where")),
            "{logs:?}"
        );
    }

    #[tokio::test]
    async fn open_in_orca_moves_a_host_an_older_release_wrote_to_the_background_token() {
        let (server, base) =
            cmux_server(serde_json::json!({"instabox-bridge": "orca", "instabox-ades": "orca"}))
                .await;
        let o = orca_core(&base, OrcaHere::Real);
        let old = older_release_host(o.home.path(), &*o.core.ssh);
        o.core.refresh(false).await;
        let notice = o.core.open_in_ade("m1", Some(Ade::Orca)).await.unwrap();
        o.core.probes_settled().await;
        // The same alias, so the host Orca imported keeps working: only its login changed.
        assert_eq!(notice.as_deref(), Some(ORCA_NOTICE));
        let hosts = hosts_of(o.home.path());
        assert_eq!(hosts.matches("# instabox-sandbox ").count(), 1, "{hosts}");
        assert!(
            hosts.contains(
                "# instabox-sandbox m1\nHost instabox-dev-1\n  HostName ssh.example.test\n  \
                 Port 2222\n  User ibssh_new\n"
            ),
            "{hosts}"
        );
        assert!(!hosts.contains("ibssh_old"), "{hosts}");
        assert_eq!(token_bodies(&server).await, [background_body()]);
        // The SSH command keeps the old token as the ordinary one: no new token, the host
        // stays as it is.
        assert_eq!(o.core.ssh_command("m1").await.unwrap(), old.command);
        assert_eq!(token_bodies(&server).await.len(), 1);
        assert_eq!(hosts_of(o.home.path()), hosts);
    }

    #[tokio::test]
    async fn open_in_paseo_moves_its_host_to_the_background_token_and_keeps_its_address() {
        let (server, base) =
            cmux_server(serde_json::json!({"instabox-bridge": "paseo", "instabox-ades": "paseo"}))
                .await;
        let p = paseo_core(&base, OrcaHere::Real, true);
        older_release_host(p.home.path(), &*p.core.ssh);
        p.core.refresh(false).await;
        let notice = p.core.open_in_ade("m1", Some(Ade::Paseo)).await.unwrap();
        p.core.probes_settled().await;
        // The address the user added in Paseo stays the same: nothing to add again.
        assert_eq!(notice.as_deref(), Some(PASEO_NOTICE));
        assert_eq!(*p.copied.lock().unwrap(), ["ssh://instabox-dev-1"]);
        let hosts = hosts_of(p.home.path());
        assert!(hosts.contains("Host instabox-dev-1\n"), "{hosts}");
        assert!(hosts.contains("  User ibssh_new\n"), "{hosts}");
        assert!(!hosts.contains("ibssh_old"), "{hosts}");
        assert_eq!(token_bodies(&server).await, [background_body()]);
    }

    #[tokio::test]
    async fn the_first_renewal_moves_a_host_an_older_release_wrote_to_the_background_token() {
        let (server, base) = cmux_server(serde_json::json!({})).await;
        let o = orca_core(&base, OrcaHere::Real);
        let old = older_release_host(o.home.path(), &*o.core.ssh);
        // The old token has 20 days left, but it is not a background token.
        let logged = o.core.renew_ssh_hosts(None).await;
        assert!(logged.is_empty(), "{logged:?}");
        let hosts = hosts_of(o.home.path());
        assert!(hosts.contains("Host instabox-dev-1\n"), "{hosts}");
        assert!(hosts.contains("  User ibssh_new\n"), "{hosts}");
        assert_eq!(token_bodies(&server).await, [background_body()]);
        let ordinary = o.core.ssh.load("m1", Kind::Ordinary).unwrap();
        assert_eq!(ordinary, Some(old), "the ordinary token stays");
        // The next pass leaves the new token alone.
        o.core.renew_ssh_hosts(None).await;
        assert_eq!(token_bodies(&server).await.len(), 1);
    }

    #[tokio::test]
    async fn an_ordinary_token_in_place_of_a_background_one_is_used_and_logged_once() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};
        let (server, base) =
            cmux_server(serde_json::json!({"instabox-bridge": "cmux", "instabox-ades": "cmux"}))
                .await;
        // An older server ignores `background` and makes an ordinary token.
        Mock::given(method("POST"))
            .and(path("/v1/machines/m1/ssh-tokens"))
            .respond_with(ResponseTemplate::new(200).set_body_json(minted("ibssh_new", false)))
            .with_priority(1)
            .mount(&server)
            .await;
        let ssh = Arc::new(MemorySshStore::default());
        let CmuxCore {
            core,
            opened,
            app,
            _dir,
        } = cmux_core(&base, ssh.clone(), CmuxHere::Real);
        core.refresh(false).await;
        core.open_in_ade("m1", Some(Ade::Cmux)).await.unwrap();
        // The link has it all the same, and it is kept as the background token.
        assert_eq!(*opened.lock().unwrap(), [cmux_opened(&app)]);
        let kept = ssh.load("m1", Kind::Background).unwrap().unwrap();
        assert_eq!((kept.token.as_str(), kept.background), ("ibssh_new", false));
        // Another one later (the stored one was lost) is not logged again.
        ssh.clear_all().unwrap();
        core.open_in_ade("m1", Some(Ade::Cmux)).await.unwrap();
        assert_eq!(
            token_bodies(&server).await,
            [background_body(), background_body()]
        );
        let logs = core.logs.lock().unwrap().clone();
        let said = logs
            .iter()
            .filter(|l| l.contains("ordinary token where a background one was asked for"))
            .count();
        assert_eq!(said, 1, "{logs:?}");
    }

    // ---- the tray's actions ----

    #[tokio::test]
    async fn the_tray_open_in_orca_hands_its_notice_to_the_popover() {
        let (_server, base) =
            cmux_server(serde_json::json!({"instabox-bridge": "orca", "instabox-ades": "orca"}))
                .await;
        let o = orca_core(&base, OrcaHere::Real);
        o.core.refresh(false).await;
        let got = Mutex::new(Vec::<String>::new());
        // Open opens the sandbox's own ADE, Orca here.
        crate::tray::run_action(&o.core, "open", "m1", |n| got.lock().unwrap().push(n)).await;
        assert_eq!(*got.lock().unwrap(), [ORCA_NOTICE]);
        assert_eq!(*o.opened.lock().unwrap(), [o.app.display().to_string()]);
        // Pause and resume have no notice to hand over.
        for verb in ["pause", "resume", "unknown"] {
            crate::tray::run_action(&o.core, verb, "m1", |n| got.lock().unwrap().push(n)).await;
        }
        assert_eq!(got.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_failed_tray_open_shows_its_error_and_no_notice() {
        let (_server, base) =
            cmux_server(serde_json::json!({"instabox-bridge": "orca", "instabox-ades": "orca"}))
                .await;
        let o = orca_core(&base, OrcaHere::Missing);
        o.core.refresh(false).await;
        let got = Mutex::new(Vec::<String>::new());
        crate::tray::run_action(&o.core, "open", "m1", |n| got.lock().unwrap().push(n)).await;
        assert!(got.lock().unwrap().is_empty());
        assert_eq!(
            o.core.state().error.map(|e| e.message).as_deref(),
            Some("Orca is not installed. Install it first.")
        );
    }

    // ---- the app checked again after the wake, and the Windows check ----

    /// Answers the read of `m1` that wakes it, and notes that the wake ran.
    struct WakeThen {
        machine: serde_json::Value,
        woke: Arc<std::sync::atomic::AtomicBool>,
    }

    impl wiremock::Respond for WakeThen {
        fn respond(&self, _: &wiremock::Request) -> wiremock::ResponseTemplate {
            self.woke.store(true, std::sync::atomic::Ordering::SeqCst);
            wiremock::ResponseTemplate::new(200).set_body_json(self.machine.clone())
        }
    }

    #[tokio::test]
    async fn an_orca_app_that_turns_invalid_during_the_wake_gets_no_token_host_or_launch() {
        use wiremock::Mock;
        use wiremock::matchers::{method, path};
        let mark = serde_json::json!({"instabox-bridge": "orca", "instabox-ades": "orca"});
        let (server, base) = cmux_server(mark.clone()).await;
        let woke = Arc::new(std::sync::atomic::AtomicBool::new(false));
        Mock::given(method("GET"))
            .and(path("/v1/machines/m1"))
            .respond_with(WakeThen {
                machine: serde_json::json!({
                    "id": "m1", "name": "dev 1", "state": "running",
                    "template": {"id": "agent-browser"}, "spec": {"vcpus": 2, "memMiB": 4096},
                    "metadata": mark, "orgId": "org-me"
                }),
                woke: woke.clone(),
            })
            .with_priority(1)
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let app = ade::orca::test_bundle(dir.path(), ade::orca::BUNDLE_ID, "1.4.269");
        // The app passes until the wake, then it is replaced by one that fails the check.
        let replaced = woke.clone();
        let o = orca_core_with(
            &base,
            dir,
            app,
            true,
            Arc::new(move |ade, path| {
                ade == Ade::Orca
                    && !replaced.load(std::sync::atomic::Ordering::SeqCst)
                    && ade::orca::verify_with(path, |_| true)
            }),
        );
        o.core.refresh(false).await;
        let e = o.core.open_in_ade("m1", Some(Ade::Orca)).await.unwrap_err();
        assert_eq!(
            e.message,
            "Could not verify the Orca app. Install it again from its official download."
        );
        assert!(
            woke.load(std::sync::atomic::Ordering::SeqCst),
            "the wake ran"
        );
        assert_eq!(o.core.state().error, Some(e));
        let r = requests(&server).await;
        assert!(
            !r.iter().any(|x| x.ends_with("/ssh-tokens")),
            "no token: {r:?}"
        );
        assert!(!o.home.path().join(".ssh").exists(), "no host written");
        assert!(o.opened.lock().unwrap().is_empty(), "nothing started");
    }

    #[tokio::test]
    async fn an_orca_exe_that_fails_the_windows_check_is_refused_at_action_time() {
        let (server, base) = cmux_server(serde_json::json!({})).await;
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("Orca.exe");
        std::fs::write(&exe, ade::orca::test_exe([1, 4, 269, 0])).unwrap();
        // The Windows check, run here on whatever platform the test runs on.
        let o = orca_core_with(
            &base,
            dir,
            exe.clone(),
            true,
            Arc::new(|ade, path| ade == Ade::Orca && ade::orca::verify_exe(path)),
        );
        // A version found when the file was still good.
        o.core
            .update(|i| i.state.set_desktop(Ade::Orca, Some("1.4.269".into())));
        let good = ade::orca::test_exe([1, 4, 269, 0]);
        for (what, bytes) in [
            ("truncated", good[..0x250].to_vec()),
            (
                "another program",
                ade::orca::test_exe_named([1, 4, 269, 0], "Notepad"),
            ),
        ] {
            std::fs::write(&exe, &bytes).unwrap();
            for result in [
                o.core.connect_sandbox("m1", Some(Ade::Orca)).await,
                o.core.open_in_ade("m1", Some(Ade::Orca)).await,
            ] {
                assert_eq!(
                    result.unwrap_err().message,
                    "Could not verify the Orca app. Install it again from its official download.",
                    "{what}"
                );
            }
        }
        assert!(
            requests(&server).await.is_empty(),
            "nothing woken, marked or made"
        );
        assert!(o.opened.lock().unwrap().is_empty());
        assert!(!o.home.path().join(".ssh").exists());
        // The real file again: read fresh, it passes and that exe starts.
        std::fs::write(&exe, &good).unwrap();
        let notice = o.core.connect_sandbox("m1", Some(Ade::Orca)).await.unwrap();
        assert_eq!(notice.as_deref(), Some(ORCA_NOTICE));
        assert_eq!(*o.opened.lock().unwrap(), [exe.display().to_string()]);
    }

    // ---- hosts of sandboxes no longer listed ----

    /// A list answer with the sandboxes `ids`, each like m1.
    fn listing(ids: &[&str]) -> serde_json::Value {
        let machines: Vec<serde_json::Value> = ids
            .iter()
            .map(|id| {
                serde_json::json!({
                    "id": id, "name": format!("box {id}"), "state": "running",
                    "template": {"id": "agent-browser"}, "spec": {"vcpus": 2, "memMiB": 4096},
                    "metadata": {}, "orgId": "org-me"
                })
            })
            .collect();
        serde_json::json!({"machines": machines, "nextCursor": null})
    }

    async fn list_answers(server: &wiremock::MockServer, body: wiremock::ResponseTemplate) {
        use wiremock::Mock;
        use wiremock::matchers::{method, path};
        Mock::given(method("GET"))
            .and(path("/v1/machines"))
            .respond_with(body)
            .with_priority(1)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn a_failed_delete_cleanup_is_logged_and_the_next_full_refresh_removes_the_host() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};
        let (server, base) =
            cmux_server(serde_json::json!({"instabox-bridge": "orca", "instabox-ades": "orca"}))
                .await;
        Mock::given(method("DELETE"))
            .and(path("/v1/machines/m1"))
            .respond_with(ResponseTemplate::new(202).set_body_json(serde_json::json!({
                "id": "op9", "machineId": "m1", "type": "delete", "state": "succeeded",
                "machine": null
            })))
            .mount(&server)
            .await;
        let o = orca_core(&base, OrcaHere::Real);
        o.core.refresh(false).await;
        o.core.open_in_ade("m1", Some(Ade::Orca)).await.unwrap();
        // The hosts file cannot be written while the sandbox is deleted.
        let hosts = ssh_config::hosts_path(o.home.path());
        let aside = hosts.with_extension("aside");
        std::fs::rename(&hosts, &aside).unwrap();
        std::fs::create_dir(&hosts).unwrap();
        o.core.delete("m1").await.unwrap();
        assert_eq!(o.core.state().error, None, "the delete itself succeeded");
        let logs = o.core.logs.lock().unwrap().clone();
        assert!(
            logs.iter()
                .any(|l| l.contains("SSH host of sandbox m1: could not remove it")),
            "{logs:?}"
        );
        std::fs::remove_dir(&hosts).unwrap();
        std::fs::rename(&aside, &hosts).unwrap();
        assert!(hosts_of(o.home.path()).contains("# instabox-sandbox m1\n"));
        // The sandbox is gone from the list: the next full refresh removes its host.
        list_answers(
            &server,
            ResponseTemplate::new(200).set_body_json(listing(&[])),
        )
        .await;
        o.core.refresh_and_sync(true).await;
        let deadline = Instant::now() + Duration::from_secs(5);
        while hosts_of(o.home.path()).contains("instabox-sandbox m1") {
            assert!(Instant::now() < deadline, "{}", hosts_of(o.home.path()));
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Answers the machine list with `m1` only, and meanwhile writes the host of `m7`: a
    /// sandbox made while the list was read, whose host is newer than the list.
    struct ListWhileCreating {
        home: PathBuf,
    }

    impl wiremock::Respond for ListWhileCreating {
        fn respond(&self, _: &wiremock::Request) -> wiremock::ResponseTemplate {
            let t = token_left("ibssh_m7", chrono::Duration::days(20));
            ssh_config::write_host(&self.home, "m7", "new", &t).unwrap();
            wiremock::ResponseTemplate::new(200).set_body_json(listing(&["m1"]))
        }
    }

    #[tokio::test]
    async fn a_host_of_an_unlisted_sandbox_goes_whatever_its_token_and_a_listed_one_stays() {
        use wiremock::Mock;
        use wiremock::matchers::{method, path};
        let (server, base) = cmux_server(serde_json::json!({})).await;
        let o = orca_core(&base, OrcaHere::Real);
        Mock::given(method("GET"))
            .and(path("/v1/machines"))
            .respond_with(ListWhileCreating {
                home: o.home.path().to_path_buf(),
            })
            .with_priority(1)
            .mount(&server)
            .await;
        // Both tokens have 20 days: no renewal is due.
        host_with(
            &o,
            "m1",
            &token_left("ibssh_m1", chrono::Duration::days(20)),
        );
        host_with(
            &o,
            "m9",
            &token_left("ibssh_m9", chrono::Duration::days(20)),
        );
        // m7's token is kept already; its host comes while the list is read.
        o.core
            .ssh
            .save(
                "m7",
                Kind::Background,
                &token_left("ibssh_m7", chrono::Duration::days(20)),
            )
            .unwrap();
        let gone = o.core.refresh_and_find_gone(true).await;
        o.core.renew_ssh_hosts(gone).await;
        let hosts = hosts_of(o.home.path());
        assert!(hosts.contains("# instabox-sandbox m1\n"), "listed: {hosts}");
        assert!(
            !hosts.contains("instabox-sandbox m9"),
            "not listed: {hosts}"
        );
        assert!(
            hosts.contains("# instabox-sandbox m7\n"),
            "newer than the list: {hosts}"
        );
        assert_eq!(token_requests(&requests(&server).await), 0);
    }

    #[tokio::test]
    async fn a_failed_or_partial_machine_list_removes_no_host() {
        use wiremock::matchers::{method, path, query_param};
        use wiremock::{Mock, ResponseTemplate};
        for partial in [false, true] {
            let (server, base) = cmux_server(serde_json::json!({})).await;
            if partial {
                // The first page comes back, the second fails.
                Mock::given(method("GET"))
                    .and(path("/v1/machines"))
                    .and(query_param("cursor", "c2"))
                    .respond_with(ResponseTemplate::new(500))
                    .with_priority(1)
                    .mount(&server)
                    .await;
                let mut first = listing(&["m2"]);
                first["nextCursor"] = serde_json::json!("c2");
                Mock::given(method("GET"))
                    .and(path("/v1/machines"))
                    .respond_with(ResponseTemplate::new(200).set_body_json(first))
                    .with_priority(2)
                    .mount(&server)
                    .await;
            } else {
                list_answers(&server, ResponseTemplate::new(500)).await;
            }
            let o = orca_core(&base, OrcaHere::Real);
            host_with(
                &o,
                "m9",
                &token_left("ibssh_m9", chrono::Duration::days(20)),
            );
            let gone = o.core.refresh_and_find_gone(true).await;
            assert!(gone.is_none(), "partial: {partial}");
            o.core.renew_ssh_hosts(gone).await;
            assert!(
                hosts_of(o.home.path()).contains("# instabox-sandbox m9\n"),
                "partial: {partial}"
            );
        }
    }

    #[tokio::test]
    async fn a_list_of_another_account_removes_no_host() {
        let (server, base) = cmux_server(serde_json::json!({})).await;
        list_answers(
            &server,
            wiremock::ResponseTemplate::new(200).set_body_json(listing(&[])),
        )
        .await;
        let o = orca_core(&base, OrcaHere::Real);
        host_with(
            &o,
            "m9",
            &token_left("ibssh_m9", chrono::Duration::days(20)),
        );
        let gone = o.core.refresh_and_find_gone(true).await;
        assert_eq!(
            gone.as_ref().map(|g| g.ids.clone()),
            Some(vec!["m9".to_string()])
        );
        // The account changed before the pass: the old account's list says nothing now.
        let gone = gone.map(|g| Gone {
            account: g.account + 100,
            ids: g.ids,
        });
        o.core.renew_ssh_hosts(gone).await;
        assert!(hosts_of(o.home.path()).contains("# instabox-sandbox m9\n"));
    }
}
