//! InstaADE's own: the proxies of the sandboxes T3 Code paired through, one per sandbox on a
//! fixed localhost port, and the pairing itself.
//!
//! Upstream, the proxy uses a DPoP session of its own (`dpop`): a P-256 key per sandbox, a
//! pairing credential from `t3 auth pairing create` run through the instabox exec API, and the
//! one-hour token T3 trades it for. Key and token are kept together in the keychain
//! (`t3-proxy-tokens`). A T3 without DPoP (older than 0.1.11) gets the earlier bearer session
//! (`t3 auth session issue`) only with `INSTAADE_T3_PROXY=any`.

use super::dpop::{self, Credential, DpopKey};
use super::runtime::{self, Control, ControlError, Record, Runtime, Tuning};
use crate::ade::t3;
use crate::api::{Client, ExecRequest, Machine};
use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// The first port a proxy may take. Each sandbox keeps the one it got: T3 Code saves it.
pub const BASE_PORT: u16 = 37800;
const PORT_RANGE: u16 = 1000;

/// Issues a bearer session for the proxy on a T3 without DPoP, printed alone on stdout. The base
/// directory is the `t3-instabox` service's (see `t3::PAIR_SCRIPT`).
pub const ISSUE_TOKEN_SCRIPT: &str = r#"t3 auth session issue --base-dir "$HOME/.t3-instabox/$(cat /etc/machine-id)" --token-only --ttl 3650d --label instaade-proxy"#;
const ISSUE_TIMEOUT_SECS: u32 = 60;

/// The instabox client of the account signed in now, if any.
pub type ClientSource = Arc<dyn Fn() -> Option<Client> + Send + Sync>;

/// The proxy's instabox side: reads of the sandbox and `sandbox::ensure_running`.
pub struct Instabox {
    client: ClientSource,
}

impl Instabox {
    pub fn new(client: ClientSource) -> Self {
        Instabox { client }
    }
}

#[async_trait]
impl Control for Instabox {
    async fn machine(&self, id: &str) -> Result<Machine, ControlError> {
        let client = (self.client)().ok_or_else(|| ControlError::Other("signed out".into()))?;
        client.machine(id).await.map_err(|e| {
            if e.status == 404 {
                ControlError::Gone
            } else {
                ControlError::Other(e.message)
            }
        })
    }

    async fn wake(&self, id: &str) -> Result<(), String> {
        let client = (self.client)().ok_or("You are not signed in")?;
        crate::sandbox::ensure_running(&client, id)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    async fn pairing_credential(&self, id: &str) -> Result<String, String> {
        let client = (self.client)().ok_or("You are not signed in")?;
        let r = client
            .exec(
                id,
                &ExecRequest {
                    cmd: dpop::PAIRING_SCRIPT.to_string(),
                    timeout_seconds: Some(ISSUE_TIMEOUT_SECS),
                },
            )
            .await
            .map_err(|e| e.message)?;
        if r.exit_code != 0 || r.timed_out {
            return Err(format!(
                "t3 auth pairing create failed: {}",
                crate::sandbox::connect::tail_lines(&r.stderr)
            ));
        }
        dpop::parse_pairing_credential(&r.stdout)
            .ok_or_else(|| "T3 printed no pairing credential".to_string())
    }
}

/// Where the proxy's upstream credentials (`dpop::Credential`, key and token) live between
/// runs, one per sandbox.
pub trait TokenStore: Send + Sync {
    fn load(&self, id: &str) -> Result<Option<String>, String>;
    fn save(&self, id: &str, token: &str) -> Result<(), String>;
    fn remove(&self, id: &str) -> Result<(), String>;
}

/// The OS keychain, next to the app's other secrets: one entry, account `t3-proxy-tokens`,
/// holding a JSON map from sandbox id to token.
pub struct KeyringTokenStore;

const KEYCHAIN_ACCOUNT: &str = "t3-proxy-tokens";
/// Serializes read-change-write of the one entry.
static KEYCHAIN_LOCK: Mutex<()> = Mutex::new(());

impl KeyringTokenStore {
    fn entry() -> Result<keyring::Entry, String> {
        keyring::Entry::new(crate::keychain_service(), KEYCHAIN_ACCOUNT).map_err(|e| e.to_string())
    }

    fn read() -> Result<HashMap<String, String>, String> {
        match Self::entry()?.get_password() {
            Ok(json) => Ok(serde_json::from_str(&json).unwrap_or_default()),
            Err(keyring::Error::NoEntry) => Ok(HashMap::new()),
            Err(e) => Err(e.to_string()),
        }
    }

    fn write(all: &HashMap<String, String>) -> Result<(), String> {
        if all.is_empty() {
            return match Self::entry()?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(e.to_string()),
            };
        }
        let json = serde_json::to_string(all).map_err(|e| e.to_string())?;
        Self::entry()?
            .set_password(&json)
            .map_err(|e| e.to_string())
    }
}

impl TokenStore for KeyringTokenStore {
    fn load(&self, id: &str) -> Result<Option<String>, String> {
        let _g = KEYCHAIN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        Ok(Self::read()?.remove(id))
    }

    fn save(&self, id: &str, token: &str) -> Result<(), String> {
        let _g = KEYCHAIN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut all = Self::read()?;
        all.insert(id.to_string(), token.to_string());
        Self::write(&all)
    }

    fn remove(&self, id: &str) -> Result<(), String> {
        let _g = KEYCHAIN_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut all = Self::read()?;
        if all.remove(id).is_some() {
            Self::write(&all)?;
        }
        Ok(())
    }
}

/// In-memory store, for tests.
#[derive(Default)]
pub struct MemoryTokenStore(Mutex<HashMap<String, String>>);

impl TokenStore for MemoryTokenStore {
    fn load(&self, id: &str) -> Result<Option<String>, String> {
        Ok(self.0.lock().unwrap().get(id).cloned())
    }
    fn save(&self, id: &str, token: &str) -> Result<(), String> {
        self.0.lock().unwrap().insert(id.into(), token.into());
        Ok(())
    }
    fn remove(&self, id: &str) -> Result<(), String> {
        self.0.lock().unwrap().remove(id);
        Ok(())
    }
}

/// The token `ISSUE_TOKEN_SCRIPT` printed: its last line, when it looks like one.
pub fn parse_token(stdout: &str) -> Option<String> {
    let line = stdout.lines().map(str::trim).rfind(|l| !l.is_empty())?;
    let ok = line.len() >= 16
        && line.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~' | '+' | '/' | '=')
        });
    ok.then(|| line.to_string())
}

/// A sandbox id fit to name a folder.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

struct Running {
    runtime: Arc<Runtime>,
    server: JoinHandle<()>,
}

pub struct Hub {
    dir: PathBuf,
    control: Arc<dyn Control>,
    tokens: Arc<dyn TokenStore>,
    tuning: Tuning,
    /// `INSTAADE_T3_PROXY=any`: a T3 without DPoP gets a bearer session.
    allow_bearer: bool,
    http: reqwest::Client,
    running: tokio::sync::Mutex<HashMap<String, Running>>,
}

impl Hub {
    pub fn new(
        dir: PathBuf,
        control: Arc<dyn Control>,
        tokens: Arc<dyn TokenStore>,
        tuning: Tuning,
        allow_bearer: bool,
    ) -> Arc<Hub> {
        Arc::new(Hub {
            dir,
            control,
            tokens,
            tuning,
            allow_bearer,
            http: reqwest::Client::new(),
            running: tokio::sync::Mutex::new(HashMap::new()),
        })
    }

    fn records(&self) -> Vec<Record> {
        std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| std::fs::read(runtime::record_path(&e.path())).ok())
            .filter_map(|b| serde_json::from_slice::<Record>(&b).ok())
            .filter(|r| valid_id(&r.id))
            .collect()
    }

    /// Starts the proxies of the sandboxes paired through it before, so T3 Code finds them
    /// after InstaADE restarts. One whose token or port is gone, or whose bearer session this
    /// start does not allow, is skipped and logged.
    pub async fn start_saved(self: &Arc<Self>) {
        for record in self.records() {
            let id = record.id.clone();
            let credential = match self.tokens.load(&id) {
                Ok(Some(t)) => Credential::parse(&t),
                Ok(None) => {
                    log_warn!(
                        "{id}: no upstream token in the keychain; Open in T3 Code pairs it again"
                    );
                    continue;
                }
                Err(e) => {
                    log_warn!("{id}: reading the keychain: {e}");
                    continue;
                }
            };
            // A bearer session is for INSTAADE_T3_PROXY=any only, here as at pairing. It stays
            // in the keychain for a later start with `any`: removing it would not end the
            // session on the sandbox, and Open in T3 Code replaces it with a DPoP session once
            // T3 there has them.
            if !credential.is_dpop() && !self.allow_bearer {
                log_warn!(
                    "{id}: saved with a bearer session, which needs INSTAADE_T3_PROXY=any; not started"
                );
                continue;
            }
            if let Err(e) = self.start(record, credential).await {
                log_warn!("{id}: {e:#}");
            }
        }
    }

    /// The runtime of sandbox `id`, if its proxy runs.
    pub async fn runtime(&self, id: &str) -> Option<Arc<Runtime>> {
        self.running.lock().await.get(id).map(|r| r.runtime.clone())
    }

    /// Serves `record` on its port and follows its sandbox.
    async fn start(
        self: &Arc<Self>,
        record: Record,
        credential: Credential,
    ) -> Result<Arc<Runtime>> {
        let id = record.id.clone();
        let listener = TcpListener::bind(("127.0.0.1", record.port))
            .await
            .with_context(|| format!("port {} is taken", record.port))?;
        let rt = Arc::new(Runtime::new(
            self.dir.join(&id),
            record,
            credential,
            self.tokens.clone(),
            self.control.clone(),
            self.tuning,
        ));
        // Saved: the port is the sandbox's from here on.
        rt.update_record(|_| {});
        let server = {
            let rt = rt.clone();
            tokio::spawn(async move {
                if let Err(e) = super::serve(listener, rt.clone()).await {
                    log_warn!("{}: the proxy stopped: {e:#}", rt.id());
                }
            })
        };
        {
            let (hub, rt) = (self.clone(), rt.clone());
            tokio::spawn(async move {
                rt.clone().run().await;
                // Deleted: nothing is left to serve.
                hub.forget(&rt.id()).await;
            });
        }
        log_info!("{id}: serving on 127.0.0.1:{}", rt.record().port);
        self.running.lock().await.insert(
            id,
            Running {
                runtime: rt.clone(),
                server,
            },
        );
        Ok(rt)
    }

    /// Stops the proxy of a deleted sandbox and removes what it kept: its record, cache, chats,
    /// images and upstream token.
    async fn forget(&self, id: &str) {
        if let Some(r) = self.running.lock().await.remove(id) {
            r.server.abort();
        }
        let _ = std::fs::remove_dir_all(self.dir.join(id));
        if let Err(e) = self.tokens.remove(id) {
            log_warn!("{id}: removing its token: {e}");
        }
        log_info!("{id}: the sandbox is gone; its proxy stopped");
    }

    /// A free port for a new sandbox, bound: the first in the range no record has.
    async fn free_port(&self) -> Result<u16> {
        let taken: Vec<u16> = self.records().iter().map(|r| r.port).collect();
        for port in BASE_PORT..BASE_PORT + PORT_RANGE {
            if taken.contains(&port) {
                continue;
            }
            if TcpListener::bind(("127.0.0.1", port)).await.is_ok() {
                return Ok(port);
            }
        }
        bail!("no free port from {BASE_PORT}")
    }

    async fn environment_id(&self, upstream: &str) -> Result<String> {
        let v: Value = self
            .http
            .get(format!("{upstream}/.well-known/t3/environment"))
            .timeout(Duration::from_secs(10))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        v["environmentId"]
            .as_str()
            .map(str::to_owned)
            .context("T3 on the sandbox sent no environment id")
    }

    async fn issue_token(&self, client: &Client, id: &str) -> Result<Credential> {
        let r = client
            .exec(
                id,
                &ExecRequest {
                    cmd: ISSUE_TOKEN_SCRIPT.to_string(),
                    timeout_seconds: Some(ISSUE_TIMEOUT_SECS),
                },
            )
            .await
            .map_err(|e| anyhow!(e.message))?;
        if r.exit_code != 0 || r.timed_out {
            bail!(
                "T3 did not issue a session for the proxy: {}",
                crate::sandbox::connect::tail_lines(&r.stderr)
            );
        }
        let token = parse_token(&r.stdout).context("T3 printed no session token")?;
        let credential = Credential::Bearer { token };
        self.tokens
            .save(id, &credential.to_stored())
            .map_err(|e| anyhow!(e))?;
        Ok(credential)
    }

    /// A new DPoP session on T3 at `upstream`: a new key, a pairing credential issued on the
    /// sandbox, traded for a token bound to the key.
    async fn pair_dpop(&self, id: &str, upstream: &str) -> Result<Credential> {
        let key = DpopKey::generate()?;
        let pairing = self
            .control
            .pairing_credential(id)
            .await
            .map_err(|e| anyhow!(e))?;
        let (token, expires_at) = dpop::exchange(
            &self.http,
            upstream,
            &key,
            &pairing,
            dpop::BOOTSTRAP_TOKEN_TYPE,
        )
        .await?;
        let credential = Credential::Dpop {
            key: key.pkcs8().to_owned(),
            token,
            expires_at,
        };
        self.tokens
            .save(id, &credential.to_stored())
            .map_err(|e| anyhow!(e))?;
        log_info!("{id}: paired with a DPoP session");
        Ok(credential)
    }

    /// The proxy's credential for T3 at `upstream`: the saved one when it fits, else a new one.
    /// DPoP whenever T3 takes it; a bearer session only for a T3 without DPoP, and only with
    /// `allow_bearer`.
    async fn credential_for(
        &self,
        client: &Client,
        id: &str,
        upstream: &str,
    ) -> Result<Credential> {
        let saved = self
            .tokens
            .load(id)
            .map_err(|e| anyhow!(e))?
            .map(|s| Credential::parse(&s));
        if dpop::supports_dpop(&self.http, upstream).await? {
            return match saved {
                Some(c) if c.is_dpop() => Ok(c),
                _ => self.pair_dpop(id, upstream).await,
            };
        }
        if !self.allow_bearer {
            bail!(
                "T3 on this sandbox has no DPoP sessions (T3 older than 0.1.11). Use Open in T3 Code, which updates it to this T3 Code's version while its agent is idle, or start InstaADE with INSTAADE_T3_PROXY=any to use a bearer session."
            );
        }
        match saved {
            Some(c @ Credential::Bearer { .. }) => Ok(c),
            _ => self.issue_token(client, id).await,
        }
    }

    /// Pairs T3 Code with the proxy of running sandbox `id`: publishes its T3 port, makes sure
    /// the proxy has a T3 session there, starts the proxy on its port, and returns the
    /// `t3code://pair` link to `http://localhost:<port>`. The sandbox must be running.
    pub async fn pair_link(
        self: &Arc<Self>,
        client: &Client,
        id: &str,
        name: &str,
    ) -> Result<String> {
        if !valid_id(id) {
            bail!("unexpected sandbox id");
        }
        let upstream = client
            .publish_port(id, t3::PORT)
            .await
            .map_err(|e| anyhow!(e.message))?
            .url
            .trim_end_matches('/')
            .to_owned();
        let environment_id = self.environment_id(&upstream).await?;
        let credential = self.credential_for(client, id, &upstream).await?;
        let existing = self.runtime(id).await;
        let rt = match existing {
            Some(rt) => {
                rt.update_record(|r| {
                    r.name = name.to_string();
                    r.upstream_url = upstream.clone();
                    r.environment_id = environment_id.clone();
                });
                rt.set_credential(credential.clone()).await;
                rt
            }
            None => {
                let saved = std::fs::read(runtime::record_path(&self.dir.join(id)))
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Record>(&b).ok());
                let port = match saved.as_ref().map(|r| r.port) {
                    Some(p) => p,
                    None => self.free_port().await?,
                };
                let saved = saved.unwrap_or_default();
                let record = Record {
                    id: id.to_string(),
                    name: name.to_string(),
                    port,
                    environment_id,
                    upstream_url: upstream.clone(),
                    client_tokens: saved.client_tokens,
                    switch_guide_done: saved.switch_guide_done,
                };
                self.start(record, credential.clone()).await?
            }
        };
        // Open is its owner: the sandbox is held again, released or not, and resumed or started
        // whatever paused it.
        rt.user_action();
        if let Err(e) = rt.ensure_awake_for_open().await {
            log_warn!("{id}: {e:#}");
        }
        // T3 refuses a session whose data was reset: one new session, then give up.
        if rt.prime().await.is_err() {
            let fresh = match credential {
                Credential::Dpop { .. } => self.pair_dpop(id, &upstream).await?,
                Credential::Bearer { .. } => self.issue_token(client, id).await?,
            };
            rt.set_credential(fresh).await;
            rt.prime().await?;
        }
        let code = rt.auth.new_pairing_code();
        Ok(t3::pairing_link(
            &format!("http://localhost:{}", rt.record().port),
            &code,
        ))
    }

    /// The guide that moves T3 Code's saved environment of sandbox `id` to the proxy, when it
    /// is due: the proxy runs, T3 Code never paired with it (it holds no token of the proxy),
    /// and the user did not finish the guide or turn it off. Whether T3 Code saved the sandbox
    /// with its published port is the caller's to judge.
    pub async fn switch_guide(&self, id: &str) -> Option<SwitchGuide> {
        let r = self.runtime(id).await?.record();
        (r.client_tokens.is_empty() && !r.switch_guide_done).then(|| SwitchGuide {
            old_address: r.upstream_url.clone(),
            new_address: format!("http://localhost:{}", r.port),
        })
    }

    /// The user finished the guide of sandbox `id`, or turned it off: it is not due again.
    pub async fn finish_switch_guide(&self, id: &str) {
        if let Some(rt) = self.runtime(id).await {
            rt.update_record(|r| r.switch_guide_done = true);
        }
    }

    /// A new pairing URL of sandbox `id`'s proxy, the one T3 Code's Add route takes:
    /// `http://localhost:<port>/pair#token=<code>`. Its code replaces the one before and works
    /// once, for 15 minutes.
    pub async fn pairing_url(&self, id: &str) -> Option<String> {
        let rt = self.runtime(id).await?;
        let code = rt.auth.new_pairing_code();
        Some(t3::pairing_url(
            &format!("http://localhost:{}", rt.record().port),
            &code,
        ))
    }
}

/// Where T3 Code reaches a sandbox before and after it moves to the proxy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwitchGuide {
    /// The published port T3 Code saved the sandbox with.
    pub old_address: String,
    /// The proxy, `http://localhost:<port>`.
    pub new_address: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_token_is_the_last_line_when_it_looks_like_one() {
        assert_eq!(
            parse_token("t3_abcdefghijklmnop.QRST-uv_w\n").as_deref(),
            Some("t3_abcdefghijklmnop.QRST-uv_w")
        );
        assert_eq!(
            parse_token("warning: x\nabcdefghijklmnopq\n\n").as_deref(),
            Some("abcdefghijklmnopq")
        );
        assert_eq!(parse_token(""), None);
        assert_eq!(parse_token("short"), None);
        assert_eq!(parse_token("has a space in it ok"), None);
        assert_eq!(parse_token("abcdefghijklmnop;rm -rf /"), None);
    }

    #[test]
    fn the_issue_script_uses_the_service_base_dir() {
        assert!(
            ISSUE_TOKEN_SCRIPT
                .contains(r#"--base-dir "$HOME/.t3-instabox/$(cat /etc/machine-id)""#)
        );
        assert!(
            t3::PAIR_SCRIPT.contains(r#"--base-dir "$HOME/.t3-instabox/$(cat /etc/machine-id)""#)
        );
        assert!(ISSUE_TOKEN_SCRIPT.contains("--token-only"));
    }

    #[test]
    fn ids_name_folders_only_when_plain() {
        assert!(valid_id("m_abc-123"));
        for bad in ["", "..", "a/b", "a b", &"x".repeat(65)] {
            assert!(!valid_id(bad), "{bad}");
        }
    }

    #[test]
    fn memory_tokens_round_trip() {
        let s = MemoryTokenStore::default();
        assert_eq!(s.load("a").unwrap(), None);
        s.save("a", "t").unwrap();
        assert_eq!(s.load("a").unwrap().as_deref(), Some("t"));
        s.remove("a").unwrap();
        assert_eq!(s.load("a").unwrap(), None);
    }
}
