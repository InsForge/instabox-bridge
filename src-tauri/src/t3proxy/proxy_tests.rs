//! The proxy end to end, against a fake T3 server and a fake instabox: pairing through the
//! proxy, the release of an idle sandbox, what a sleeping sandbox answers or wakes for, and the
//! proxy's DPoP session upstream (proofs checked as T3 checks them, renewal, refusal).

use super::auth::token_hash;
use super::dpop::{self, Credential, DpopKey};
use super::hub::{Hub, MemoryTokenStore, TokenStore};
use super::runtime::{Control, ControlError, Power, Record, Runtime, Tuning};
use crate::api::{Client, Credentials, Machine};
use async_trait::async_trait;
use axum::extract::ws::{Message as WsMsg, WebSocketUpgrade};
use axum::extract::{RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

/// The bearer session of a T3 without DPoP.
const UP_TOKEN: &str = "up-token-0123456789";
const CLIENT_TOKEN: &str = "client-token";

fn now_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

/// A token T3 issued for a DPoP key.
struct Issued {
    thumbprint: String,
    expires_at: i64,
}

/// A T3 server: answers every request on its socket, counts its sockets, and checks every
/// credential as T3 does.
struct FakeT3 {
    base: Mutex<String>,
    /// Takes DPoP sessions (0.1.11 and later); else bearer only.
    dpop: bool,
    connections: AtomicUsize,
    open: AtomicUsize,
    tags: Mutex<Vec<String>>,
    queries: Mutex<Vec<String>>,
    /// The shell shows an agent at work.
    working: AtomicBool,
    /// Lifetime of the DPoP tokens it issues, in seconds.
    expires_in: AtomicI64,
    pairings: Mutex<HashSet<String>>,
    tokens: Mutex<HashMap<String, Issued>>,
    jtis: Mutex<HashSet<String>>,
    exchanges: AtomicUsize,
    renewals: AtomicUsize,
    refuse_renewal: AtomicBool,
    /// Requests that came with a bearer token although DPoP was in use.
    bearer_seen: AtomicUsize,
    /// Reads of the shell over HTTP (the capture's), and a gate a test holds to keep one open.
    shell_reads: AtomicUsize,
    shell_gate: tokio::sync::Mutex<()>,
    /// Device hub streams (MJPEG, events) still sending.
    streams_open: Arc<AtomicUsize>,
}

impl FakeT3 {
    fn new(dpop: bool) -> Self {
        FakeT3 {
            base: Mutex::new(String::new()),
            dpop,
            connections: AtomicUsize::new(0),
            open: AtomicUsize::new(0),
            tags: Mutex::new(Vec::new()),
            queries: Mutex::new(Vec::new()),
            working: AtomicBool::new(false),
            expires_in: AtomicI64::new(3600),
            pairings: Mutex::new(HashSet::new()),
            tokens: Mutex::new(HashMap::new()),
            jtis: Mutex::new(HashSet::new()),
            exchanges: AtomicUsize::new(0),
            renewals: AtomicUsize::new(0),
            refuse_renewal: AtomicBool::new(false),
            bearer_seen: AtomicUsize::new(0),
            shell_reads: AtomicUsize::new(0),
            shell_gate: tokio::sync::Mutex::new(()),
            streams_open: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// `t3 auth pairing create`.
    fn mint_pairing(&self) -> String {
        let credential = super::auth::random_token();
        self.pairings.lock().unwrap().insert(credential.clone());
        credential
    }

    fn issue(&self, thumbprint: String) -> (String, i64) {
        let token = super::auth::random_token();
        let expires_in = self.expires_in.load(Ordering::SeqCst);
        self.tokens.lock().unwrap().insert(
            token.clone(),
            Issued {
                thumbprint,
                expires_at: now_secs() + expires_in,
            },
        );
        (token, expires_in)
    }

    /// A proof for `method` on `path`, as `dpop.ts` verifies it: its thumbprint.
    fn proof(
        &self,
        proof: Option<&str>,
        method: &str,
        path: &str,
        token: Option<&str>,
    ) -> Option<String> {
        let parts: Vec<&str> = proof?.split('.').collect();
        if parts.len() != 3 {
            return None;
        }
        let header: Value = serde_json::from_slice(&B64.decode(parts[0]).ok()?).ok()?;
        let payload: Value = serde_json::from_slice(&B64.decode(parts[1]).ok()?).ok()?;
        if header["typ"] != "dpop+jwt"
            || header["alg"] != "ES256"
            || header["jwk"].get("d").is_some()
        {
            return None;
        }
        let (x, y) = (header["jwk"]["x"].as_str()?, header["jwk"]["y"].as_str()?);
        let mut point = vec![4u8];
        point.extend(B64.decode(x).ok()?);
        point.extend(B64.decode(y).ok()?);
        ring::signature::UnparsedPublicKey::new(&ring::signature::ECDSA_P256_SHA256_FIXED, point)
            .verify(
                format!("{}.{}", parts[0], parts[1]).as_bytes(),
                &B64.decode(parts[2]).ok()?,
            )
            .ok()?;
        let htu = format!("{}{path}", self.base.lock().unwrap());
        let fresh = (payload["iat"].as_i64()? - now_secs()).abs() <= 300;
        let ath_ok = token.is_none_or(|t| payload["ath"] == dpop::ath(t));
        if payload["htm"] != method || payload["htu"] != htu.as_str() || !fresh || !ath_ok {
            return None;
        }
        // Each proof once.
        if !self
            .jtis
            .lock()
            .unwrap()
            .insert(payload["jti"].as_str()?.to_owned())
        {
            return None;
        }
        let thumbprint_input = format!(r#"{{"crv":"P-256","kty":"EC","x":"{x}","y":"{y}"}}"#);
        Some(B64.encode(
            ring::digest::digest(&ring::digest::SHA256, thumbprint_input.as_bytes()).as_ref(),
        ))
    }

    /// The request's credential is good: a live DPoP token with its key's proof, or (on a T3
    /// without DPoP) the bearer session.
    fn authorized(&self, method: &str, path: &str, headers: &HeaderMap) -> bool {
        let auth = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        if let Some(token) = auth.strip_prefix("Bearer ") {
            if self.dpop {
                self.bearer_seen.fetch_add(1, Ordering::SeqCst);
                return false;
            }
            return token == UP_TOKEN;
        }
        let Some(token) = auth.strip_prefix("DPoP ") else {
            return false;
        };
        let proof = headers.get("dpop").and_then(|v| v.to_str().ok());
        let Some(thumbprint) = self.proof(proof, method, path, Some(token)) else {
            return false;
        };
        self.tokens
            .lock()
            .unwrap()
            .get(token)
            .is_some_and(|i| i.thumbprint == thumbprint && i.expires_at > now_secs())
    }
}

async fn up_descriptor() -> Json<Value> {
    Json(json!({"environmentId": "env-1", "label": "upstream"}))
}

async fn up_session(State(t3): State<Arc<FakeT3>>, headers: HeaderMap) -> Response {
    if !headers.contains_key("authorization") {
        let methods = if t3.dpop {
            json!(["bearer-access-token", "dpop-access-token"])
        } else {
            json!(["bearer-access-token"])
        };
        return Json(json!({"authenticated": false, "auth": {"sessionMethods": methods}}))
            .into_response();
    }
    if !t3.authorized("GET", "/api/auth/session", &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Json(json!({
        "authenticated": true,
        "scopes": ["orchestration:read", "orchestration:operate"],
        "permissions": {"orchestration": true}
    }))
    .into_response()
}

async fn up_token(
    State(t3): State<Arc<FakeT3>>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let refused = || {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"code": "invalid_credential"})),
        )
            .into_response()
    };
    let proof = headers.get("dpop").and_then(|v| v.to_str().ok());
    let Some(thumbprint) = t3.proof(proof, "POST", "/oauth/token", None) else {
        return refused();
    };
    let subject = form.get("subject_token").cloned().unwrap_or_default();
    let issued = match form.get("subject_token_type").map(String::as_str) {
        Some(dpop::BOOTSTRAP_TOKEN_TYPE) => {
            if !t3.pairings.lock().unwrap().remove(&subject) {
                return refused();
            }
            t3.exchanges.fetch_add(1, Ordering::SeqCst);
            t3.issue(thumbprint)
        }
        Some(dpop::ACCESS_TOKEN_TYPE) => {
            t3.renewals.fetch_add(1, Ordering::SeqCst);
            // An expired token renews too; the key must be the one it is bound to.
            let bound = t3
                .tokens
                .lock()
                .unwrap()
                .get(&subject)
                .is_some_and(|i| i.thumbprint == thumbprint);
            if !bound || t3.refuse_renewal.load(Ordering::SeqCst) {
                return refused();
            }
            t3.issue(thumbprint)
        }
        _ => return refused(),
    };
    Json(json!({
        "access_token": issued.0,
        "issued_token_type": dpop::ACCESS_TOKEN_TYPE,
        "token_type": "DPoP",
        "expires_in": issued.1,
        "scope": "orchestration:read orchestration:operate"
    }))
    .into_response()
}

async fn up_shell(State(t3): State<Arc<FakeT3>>, headers: HeaderMap) -> Response {
    if !t3.authorized("GET", "/api/orchestration/shell", &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    t3.shell_reads.fetch_add(1, Ordering::SeqCst);
    drop(t3.shell_gate.lock().await);
    let threads = if t3.working.load(Ordering::SeqCst) {
        json!([{"id": "t1", "activeRunId": "r1"}])
    } else {
        json!([{"id": "t1", "activeRunId": null}])
    };
    Json(json!({"snapshotSequence": 5, "threads": threads})).into_response()
}

/// Counts a device hub stream as open until its body is dropped.
struct OpenStream(Arc<AtomicUsize>);

impl Drop for OpenStream {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// `/api/device-hub/*`: a stream that never ends, as the hub's MJPEG and events do.
async fn up_device_stream(
    State(t3): State<Arc<FakeT3>>,
    uri: axum::http::Uri,
    headers: HeaderMap,
) -> Response {
    if !t3.authorized("GET", uri.path(), &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    t3.streams_open.fetch_add(1, Ordering::SeqCst);
    let open = OpenStream(t3.streams_open.clone());
    let frames = futures_util::stream::unfold(open, |open| async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        Some((
            Ok::<_, std::convert::Infallible>(bytes::Bytes::from_static(b"--frame\r\n")),
            open,
        ))
    });
    (
        [("content-type", "multipart/x-mixed-replace; boundary=frame")],
        axum::body::Body::from_stream(frames),
    )
        .into_response()
}

/// `/api/preview/tunnel` to a port where nothing listens, as T3 0.1.13 answers it.
async fn up_tunnel(
    State(t3): State<Arc<FakeT3>>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    if !t3.authorized("GET", "/api/preview/tunnel", &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    t3.queries.lock().unwrap().push(query.unwrap_or_default());
    (
        StatusCode::FORBIDDEN,
        [("x-t3-tunnel-port-refused", "1")],
        "Nothing accepted a connection on loopback port 9.",
    )
        .into_response()
}

async fn up_ws(
    State(t3): State<Arc<FakeT3>>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    // The upgrade carries its own proof, for `GET` on the socket's URL without its query.
    if !t3.authorized("GET", "/ws", &headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    t3.queries.lock().unwrap().push(query.unwrap_or_default());
    upgrade.on_upgrade(move |socket| async move {
        t3.connections.fetch_add(1, Ordering::SeqCst);
        t3.open.fetch_add(1, Ordering::SeqCst);
        let (mut tx, mut rx) = socket.split();
        loop {
            let text = match rx.next().await {
                Some(Ok(WsMsg::Text(t))) => t,
                Some(Ok(_)) => continue,
                _ => break,
            };
            for msg in super::session::messages(text.as_str()).unwrap() {
                if msg["_tag"] != "Request" {
                    continue;
                }
                let tag = msg["tag"].as_str().unwrap_or_default().to_owned();
                t3.tags.lock().unwrap().push(tag.clone());
                let id = msg["id"].clone();
                let reply = if tag.contains("subscribe") {
                    json!({"_tag": "Chunk", "requestId": id, "values": [{"kind": "snapshot"}]})
                } else {
                    json!({"_tag": "Exit", "requestId": id, "exit": {"_tag": "Success", "value": {"ok": tag}}})
                };
                if tx.send(WsMsg::text(reply.to_string())).await.is_err() {
                    break;
                }
            }
        }
        t3.open.fetch_sub(1, Ordering::SeqCst);
    })
}

async fn fake_t3(dpop: bool) -> (Arc<FakeT3>, String) {
    let t3 = Arc::new(FakeT3::new(dpop));
    let app = Router::new()
        .route("/.well-known/t3/environment", get(up_descriptor))
        .route("/api/auth/session", get(up_session))
        .route("/oauth/token", post(up_token))
        .route("/api/orchestration/shell", get(up_shell))
        .route("/ws", get(up_ws))
        .route("/api/preview/tunnel", get(up_tunnel))
        .route("/api/device-hub/{*path}", get(up_device_stream))
        .with_state(t3.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    *t3.base.lock().unwrap() = url.clone();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (t3, url)
}

/// instabox: the sandbox's state, wakes counted, and pairing credentials from T3 on it.
struct FakeControl {
    state: Mutex<String>,
    auto_suspended: bool,
    wakes: AtomicUsize,
    revision: AtomicI64,
    t3: Arc<FakeT3>,
    pairings_asked: AtomicUsize,
    refuse_pairing: AtomicBool,
}

impl FakeControl {
    fn new(state: &str, auto_suspended: bool, t3: Arc<FakeT3>) -> Arc<Self> {
        Arc::new(FakeControl {
            state: Mutex::new(state.into()),
            auto_suspended,
            wakes: AtomicUsize::new(0),
            revision: AtomicI64::new(1),
            t3,
            pairings_asked: AtomicUsize::new(0),
            refuse_pairing: AtomicBool::new(false),
        })
    }

    fn wakes(&self) -> usize {
        self.wakes.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Control for FakeControl {
    async fn machine(&self, id: &str) -> Result<Machine, ControlError> {
        let state = self.state.lock().unwrap().clone();
        let suspended =
            (state != "running" && self.auto_suspended).then_some("2026-11-27T10:00:00Z");
        Ok(serde_json::from_value(json!({
            "id": id, "name": "qa", "state": state,
            "template": {"id": "agent-browser"}, "spec": {"vcpus": 2, "memMiB": 4096},
            "revision": self.revision.load(Ordering::SeqCst), "autoSuspendedAt": suspended
        }))
        .unwrap())
    }

    async fn wake(&self, _id: &str) -> Result<(), String> {
        self.wakes.fetch_add(1, Ordering::SeqCst);
        self.revision.fetch_add(1, Ordering::SeqCst);
        *self.state.lock().unwrap() = "running".into();
        Ok(())
    }

    async fn pairing_credential(&self, _id: &str) -> Result<String, String> {
        self.pairings_asked.fetch_add(1, Ordering::SeqCst);
        if self.refuse_pairing.load(Ordering::SeqCst) {
            return Err("t3 auth pairing create failed".into());
        }
        Ok(self.t3.mint_pairing())
    }
}

fn tuning(idle_after: Duration) -> Tuning {
    Tuning {
        idle_after,
        poll: Duration::from_millis(50),
        tick: Duration::from_millis(20),
        busy_recheck: Duration::from_millis(100),
        renew_before: Duration::from_secs(1),
    }
}

/// A DPoP session on `t3`, paired the way the hub pairs it.
async fn paired(t3: &FakeT3, base: &str) -> Credential {
    let key = DpopKey::generate().unwrap();
    let pairing = t3.mint_pairing();
    let (token, expires_at) = dpop::exchange(
        &reqwest::Client::new(),
        base,
        &key,
        &pairing,
        dpop::BOOTSTRAP_TOKEN_TYPE,
    )
    .await
    .unwrap();
    Credential::Dpop {
        key: key.pkcs8().to_owned(),
        token,
        expires_at,
    }
}

struct Proxy {
    t3: Arc<FakeT3>,
    control: Arc<FakeControl>,
    rt: Arc<Runtime>,
    tokens: Arc<MemoryTokenStore>,
    port: u16,
    _dir: tempfile::TempDir,
}

/// A proxy on an ephemeral port for a sandbox in `state`, paired with DPoP, its runtime
/// running. `expires_in`: the lifetime of T3's tokens, in seconds.
async fn proxy_with(
    state: &str,
    auto_suspended: bool,
    idle_after: Duration,
    expires_in: i64,
) -> Proxy {
    let (t3, upstream_url) = fake_t3(true).await;
    t3.expires_in.store(expires_in, Ordering::SeqCst);
    let control = FakeControl::new(state, auto_suspended, t3.clone());
    let dir = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let credential = paired(&t3, &upstream_url).await;
    let record = Record {
        id: "m_test".into(),
        name: "qa".into(),
        port,
        environment_id: "env-1".into(),
        upstream_url,
        client_tokens: vec![token_hash(CLIENT_TOKEN)],
        switch_guide_done: false,
    };
    let tokens = Arc::new(MemoryTokenStore::default());
    let rt = Arc::new(Runtime::new(
        dir.path().join("m_test"),
        record,
        credential,
        tokens.clone(),
        control.clone(),
        tuning(idle_after),
    ));
    if state == "running" {
        rt.prime().await.unwrap();
    }
    tokio::spawn(super::serve(listener, rt.clone()));
    tokio::spawn(rt.clone().run());
    Proxy {
        t3,
        control,
        rt,
        tokens,
        port,
        _dir: dir,
    }
}

async fn proxy(state: &str, auto_suspended: bool, idle_after: Duration) -> Proxy {
    proxy_with(state, auto_suspended, idle_after, 3600).await
}

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn t3_code(port: u16) -> Ws {
    let mut req = format!("ws://127.0.0.1:{port}/ws?clientSurface=desktop")
        .into_client_request()
        .unwrap();
    req.headers_mut().insert(
        "authorization",
        format!("Bearer {CLIENT_TOKEN}").parse().unwrap(),
    );
    tokio_tungstenite::connect_async(req).await.unwrap().0
}

async fn send(ws: &mut Ws, msg: Value) {
    ws.send(Message::text(msg.to_string())).await.unwrap();
}

fn request(id: &str, tag: &str, payload: Value) -> Value {
    json!({"_tag": "Request", "id": id, "tag": tag, "payload": payload, "headers": []})
}

/// The next text frame as JSON, or the close code as `{"close": code}`.
async fn next(ws: &mut Ws) -> Value {
    let read = async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Text(t))) => return serde_json::from_str(t.as_str()).unwrap(),
                Some(Ok(Message::Close(frame))) => {
                    return json!({"close": frame.map(|f| u16::from(f.code))});
                }
                Some(Ok(_)) => continue,
                other => panic!("socket ended: {other:?}"),
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(10), read)
        .await
        .expect("an answer within 10 s")
}

async fn eventually(what: &str, check: impl Fn() -> bool) {
    for _ in 0..300 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("never: {what}");
}

fn count(n: &AtomicUsize) -> usize {
    n.load(Ordering::SeqCst)
}

#[tokio::test]
async fn an_idle_sandbox_is_released_and_the_next_action_takes_it_back() {
    let p = proxy("running", true, Duration::from_millis(400)).await;
    let mut ws = t3_code(p.port).await;
    send(
        &mut ws,
        request("1", "orchestration.subscribeShell", json!({})),
    )
    .await;
    let first = next(&mut ws).await;
    assert_eq!(
        (first["_tag"].as_str(), first["requestId"].as_str()),
        (Some("Chunk"), Some("1"))
    );
    assert_eq!(count(&p.t3.open), 1);

    // No action for `idle_after`: the upstream socket closes, so the port lease ends.
    eventually("the upstream socket closes", || count(&p.t3.open) == 0).await;
    assert!(p.rt.released());
    assert_eq!(p.rt.power(), Power::Stopped);
    assert_eq!(p.control.wakes(), 0);
    assert!(p.rt.fresh_capture().is_some(), "captured as it let go");

    // T3 Code's own upkeep is answered here and takes nothing back.
    send(&mut ws, json!({"_tag": "Ping"})).await;
    assert_eq!(next(&mut ws).await, json!({"_tag": "Pong"}));
    send(
        &mut ws,
        request("2", "server.reportClientActivity", json!({"focused": true})),
    )
    .await;
    let report = next(&mut ws).await;
    assert_eq!(report["requestId"], "2");
    assert_eq!(report["exit"]["_tag"], "Success");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(count(&p.t3.connections), 1);
    assert_eq!(p.control.wakes(), 0);

    // A person acts: the proxy takes the sandbox back and the action goes through.
    let archive = json!({"type": "thread.archive", "commandId": "c1", "threadId": "t1"});
    send(
        &mut ws,
        request("3", "orchestration.dispatchCommand", archive),
    )
    .await;
    let done = next(&mut ws).await;
    assert_eq!(done["requestId"], "3");
    assert_eq!(done["exit"]["value"]["ok"], "orchestration.dispatchCommand");
    // Then the socket closes so T3 Code resubscribes against the live server.
    assert_eq!(next(&mut ws).await, json!({"close": 1012}));
    assert_eq!(p.control.wakes(), 1);
    assert_eq!(count(&p.t3.connections), 2);
    assert!(!p.rt.released());
    assert_eq!(count(&p.t3.bearer_seen), 0, "DPoP all the way");
}

#[tokio::test]
async fn an_orchestration_event_during_the_capture_calls_off_the_release() {
    let p = proxy("running", true, Duration::from_millis(400)).await;
    // The capture starts once the sandbox is idle; its shell read is held open.
    let gate = p.t3.shell_gate.lock().await;
    let mut ws = t3_code(p.port).await;
    send(
        &mut ws,
        request("1", "orchestration.subscribeShell", json!({})),
    )
    .await;
    assert_eq!(next(&mut ws).await["requestId"], "1");
    assert_eq!(count(&p.t3.open), 1);
    eventually("the capture reads the shell", || {
        count(&p.t3.shell_reads) == 1
    })
    .await;
    // An agent starts meanwhile: T3 sends an orchestration event.
    p.rt.state_moved();
    drop(gate);
    eventually("the capture ends", || {
        p.rt.cache.lock().unwrap().capture.is_some()
    })
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !p.rt.released(),
        "released on a capture older than the event"
    );
    assert_eq!(p.rt.power(), Power::Running);
    assert_eq!(count(&p.t3.open), 1, "the upstream stays open");

    // Quiet for `idle_after` again: the next capture lets it go.
    eventually("released later", || p.rt.released()).await;
    assert_eq!(count(&p.t3.shell_reads), 2);
    eventually("the upstream socket closes", || count(&p.t3.open) == 0).await;
}

#[tokio::test]
async fn a_long_http_stream_ends_when_the_sandbox_is_released() {
    let p = proxy("running", true, Duration::from_millis(400)).await;
    // T3 Code's device view: an MJPEG stream over plain HTTP, which never ends by itself.
    let mut res = reqwest::Client::new()
        .get(format!(
            "http://127.0.0.1:{}/api/device-hub/helper/d1/stream.mjpeg",
            p.port
        ))
        .header("authorization", format!("Bearer {CLIENT_TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(res.chunk().await.unwrap().is_some());
    assert_eq!(count(&p.t3.streams_open), 1);

    // Watching is not using: the sandbox is released, and the stream to it ends with that.
    eventually("released", || p.rt.released()).await;
    eventually("the upstream stream ends", || {
        count(&p.t3.streams_open) == 0
    })
    .await;
    let rest = tokio::time::timeout(Duration::from_secs(5), async {
        while let Ok(Some(_)) = res.chunk().await {}
    })
    .await;
    assert!(rest.is_ok(), "T3 Code's end of the stream closes too");
    assert_eq!(p.control.wakes(), 0);
}

#[tokio::test]
async fn a_saved_bearer_session_starts_only_when_bearer_is_allowed() {
    let (t3, upstream_url) = fake_t3(false).await;
    let control = FakeControl::new("running", true, t3.clone());
    let dir = tempfile::tempdir().unwrap();
    let free_port = || {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    };
    let save = |id: &str, port: u16, upstream_url: &str| {
        let record = Record {
            id: id.into(),
            name: "qa".into(),
            port,
            environment_id: "env-1".into(),
            upstream_url: upstream_url.into(),
            client_tokens: vec![],
            switch_guide_done: false,
        };
        let at = dir.path().join(id);
        std::fs::create_dir_all(&at).unwrap();
        std::fs::write(
            super::runtime::record_path(&at),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
    };
    let port = free_port();
    save("m_test", port, &upstream_url);
    let tokens = Arc::new(MemoryTokenStore::default());
    let bearer = Credential::Bearer {
        token: UP_TOKEN.into(),
    };
    tokens.save("m_test", &bearer.to_stored()).unwrap();
    let hub = |allow_bearer| {
        Hub::new(
            dir.path().to_path_buf(),
            control.clone(),
            tokens.clone(),
            tuning(Duration::from_secs(60)),
            allow_bearer,
        )
    };

    // Paired with INSTAADE_T3_PROXY=any, started again with =1: not served, kept for `any`.
    let strict = hub(false);
    strict.start_saved().await;
    assert!(strict.runtime("m_test").await.is_none());
    assert!(
        tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_err()
    );
    assert_eq!(
        tokens
            .load("m_test")
            .unwrap()
            .map(|s| Credential::parse(&s)),
        Some(bearer)
    );

    // A DPoP session still comes back with =1.
    let (dpop_t3, dpop_url) = fake_t3(true).await;
    save("m_dpop", free_port(), &dpop_url);
    tokens
        .save("m_dpop", &paired(&dpop_t3, &dpop_url).await.to_stored())
        .unwrap();
    let strict = hub(false);
    strict.start_saved().await;
    assert!(strict.runtime("m_dpop").await.is_some());
    assert!(strict.runtime("m_test").await.is_none());

    // With =any the bearer session comes back too.
    let any = hub(true);
    any.start_saved().await;
    assert!(any.runtime("m_test").await.is_some());
}

#[tokio::test]
async fn a_sandbox_with_an_agent_working_is_held() {
    let p = proxy("running", true, Duration::from_millis(200)).await;
    p.t3.working.store(true, Ordering::SeqCst);
    let mut ws = t3_code(p.port).await;
    send(
        &mut ws,
        request("1", "orchestration.subscribeShell", json!({})),
    )
    .await;
    next(&mut ws).await;
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(count(&p.t3.open), 1, "held while the agent works");
    assert!(!p.rt.released());
    p.t3.working.store(false, Ordering::SeqCst);
    eventually("released once the run ends", || count(&p.t3.open) == 0).await;
}

#[tokio::test]
async fn a_sleeping_sandbox_answers_upkeep_here_and_wakes_for_the_unknown() {
    let p = proxy("suspended", true, Duration::from_secs(60)).await;
    let mut ws = t3_code(p.port).await;
    send(&mut ws, json!({"_tag": "Ping"})).await;
    assert_eq!(next(&mut ws).await, json!({"_tag": "Pong"}));
    for (id, tag) in [
        ("1", "server.reportClientActivity"),
        ("2", "preview.reportStatus"),
        ("3", "pullRequests.invalidate"),
    ] {
        send(&mut ws, request(id, tag, json!({}))).await;
        let answer = next(&mut ws).await;
        assert_eq!(
            (
                answer["requestId"].as_str(),
                answer["exit"]["_tag"].as_str()
            ),
            (Some(id), Some("Success")),
            "{tag}"
        );
    }
    send(&mut ws, request("4", "server.probe", json!({}))).await;
    assert_eq!(next(&mut ws).await["requestId"], "4");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(p.control.wakes(), 0);
    assert_eq!(count(&p.t3.connections), 0);

    // An RPC the proxy does not know wakes the sandbox, which is always safe.
    send(&mut ws, request("5", "something.new", json!({}))).await;
    let answer = next(&mut ws).await;
    assert_eq!(answer["requestId"], "5");
    assert_eq!(answer["exit"]["value"]["ok"], "something.new");
    assert_eq!(p.control.wakes(), 1);
    assert!(
        p.t3.tags
            .lock()
            .unwrap()
            .contains(&"something.new".to_string())
    );
}

/// T3 Code's locality probe: a WebSocket upgrade of `/api/preview/tunnel`, which T3 refuses for
/// a port where nothing listens. A plain client reads the whole answer.
async fn tunnel_probe(port: u16) -> reqwest::Response {
    reqwest::Client::new()
        .get(format!("http://127.0.0.1:{port}/api/preview/tunnel?port=9"))
        .header("authorization", format!("Bearer {CLIENT_TOKEN}"))
        .header("connection", "upgrade")
        .header("upgrade", "websocket")
        .header("sec-websocket-version", "13")
        .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn a_refused_tunnel_reaches_t3_code_with_its_headers_and_body() {
    let p = proxy("running", true, Duration::from_secs(60)).await;
    let res = tunnel_probe(p.port).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        res.headers()
            .get("x-t3-tunnel-port-refused")
            .map(|v| v.as_bytes()),
        Some(&b"1"[..])
    );
    assert_eq!(
        res.text().await.unwrap(),
        "Nothing accepted a connection on loopback port 9."
    );
    assert!(p.t3.queries.lock().unwrap().contains(&"port=9".to_string()));
    assert_eq!(p.control.wakes(), 0);
}

/// A person's action: archiving a chat.
fn archive(id: &str) -> Value {
    request(
        id,
        "orchestration.dispatchCommand",
        json!({"type": "thread.archive", "commandId": "c1", "threadId": "t1"}),
    )
}

#[tokio::test]
async fn a_sandbox_that_paused_itself_wakes_for_an_action_and_for_the_tunnel() {
    let p = proxy("suspended", true, Duration::from_secs(60)).await;
    let mut ws = t3_code(p.port).await;
    send(&mut ws, archive("1")).await;
    let answer = next(&mut ws).await;
    assert_eq!(answer["requestId"], "1");
    assert_eq!(answer["exit"]["_tag"], "Success");
    assert_eq!(p.control.wakes(), 1);

    let p = proxy("suspended", true, Duration::from_secs(60)).await;
    let res = tunnel_probe(p.port).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert!(res.headers().contains_key("x-t3-tunnel-port-refused"));
    assert_eq!(p.control.wakes(), 1);
}

/// instabox's rule for any use: a sandbox its owner paused or stopped is not woken, and an
/// action gets "Sandbox offline" with what to do. `state` is how it was left.
async fn an_owner_s_pause_holds(state: &str, says: &str) {
    let p = proxy(state, false, Duration::from_secs(60)).await;
    let mut ws = t3_code(p.port).await;
    // What T3 Code asks on its own as it connects waits, unanswered.
    send(
        &mut ws,
        request("1", "orchestration.subscribeShell", json!({})),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    // A person's action fails with what to do; what waited still waits.
    send(&mut ws, archive("2")).await;
    let answer = next(&mut ws).await;
    assert_eq!(answer["requestId"], "2");
    let text = answer["exit"]["cause"][0]["defect"].as_str().unwrap();
    assert!(text.starts_with("Sandbox offline: qa "), "{text}");
    assert!(text.contains(says), "{text}");
    // So does a message, which a sandbox that paused itself takes in to send after its wake.
    let message = json!({"type": "message.dispatch", "commandId": "c2", "threadId": "t1"});
    send(
        &mut ws,
        request("3", "orchestration.dispatchCommand", message),
    )
    .await;
    let answer = next(&mut ws).await;
    assert_eq!(answer["requestId"], "3");
    assert_eq!(answer["exit"]["cause"][0]["defect"].as_str(), Some(text));
    assert!(p.rt.cache.lock().unwrap().deferred_commands.is_empty());
    // The tunnel answers as instabox does for such a sandbox's public port.
    let res = tunnel_probe(p.port).await;
    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(res.text().await.unwrap(), text);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(p.control.wakes(), 0);
    assert_eq!(count(&p.t3.connections), 0);

    // Open in T3 Code is its owner's start: it resumes it, and what waited goes upstream.
    p.rt.ensure_awake_for_open().await.unwrap();
    assert_eq!(p.control.wakes(), 1);
    assert_eq!(next(&mut ws).await["requestId"], "1");
}

#[tokio::test]
async fn a_sandbox_its_owner_paused_stays_paused() {
    an_owner_s_pause_holds("suspended", "Resume it first: instabox resume qa").await;
}

#[tokio::test]
async fn a_sandbox_its_owner_stopped_stays_stopped() {
    an_owner_s_pause_holds("stopped", "Start it first: instabox start qa").await;
}

#[tokio::test]
async fn the_dpop_token_is_renewed_before_it_runs_out_while_held() {
    // Tokens for 2 s, renewed with 1 s left: about every second while the proxy holds it.
    let p = proxy_with("running", true, Duration::from_secs(60), 2).await;
    let first = p.rt.credential().await;
    eventually("a renewal", || count(&p.t3.renewals) >= 2).await;
    assert_ne!(p.rt.credential().await, first, "a new token");
    // The renewed token is kept, and it works.
    let kept = Credential::parse(&p.tokens.load("m_test").unwrap().unwrap());
    assert!(kept.is_dpop() && kept != first);
    p.rt.prime().await.unwrap();
    assert_eq!(count(&p.t3.exchanges), 1, "renewed, not paired again");
    assert_eq!(count(&p.t3.bearer_seen), 0);
}

#[tokio::test]
async fn a_released_sandbox_is_not_renewed_and_its_expired_token_renews_on_the_next_use() {
    let p = proxy_with("running", true, Duration::from_millis(200), 2).await;
    eventually("released", || p.rt.released()).await;
    let at_release = count(&p.t3.renewals);
    // A renewal is a request to the sandbox: it would wake it, so none while released.
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(
        count(&p.t3.renewals),
        at_release,
        "no renewal while released"
    );
    // The next action takes the sandbox back; its expired token renews first.
    let mut ws = t3_code(p.port).await;
    let archive = json!({"type": "thread.archive", "commandId": "c1", "threadId": "t1"});
    send(
        &mut ws,
        request("1", "orchestration.dispatchCommand", archive),
    )
    .await;
    assert_eq!(next(&mut ws).await["requestId"], "1");
    assert!(count(&p.t3.renewals) > at_release);
    assert_eq!(count(&p.t3.exchanges), 1);
}

#[tokio::test]
async fn a_refused_renewal_pairs_again_through_the_sandbox() {
    let p = proxy_with("running", true, Duration::from_secs(60), 2).await;
    p.t3.refuse_renewal.store(true, Ordering::SeqCst);
    eventually("paired again", || count(&p.t3.exchanges) == 2).await;
    assert_eq!(count(&p.control.pairings_asked), 1);
    assert!(p.rt.credential().await.is_dpop());
    p.t3.refuse_renewal.store(false, Ordering::SeqCst);
    p.rt.prime().await.unwrap();
    assert_eq!(count(&p.t3.bearer_seen), 0);
}

#[tokio::test]
async fn a_session_refused_for_good_fails_with_a_clear_error_and_never_falls_back_to_bearer() {
    let p = proxy_with("running", true, Duration::from_secs(60), 2).await;
    p.t3.refuse_renewal.store(true, Ordering::SeqCst);
    p.control.refuse_pairing.store(true, Ordering::SeqCst);
    eventually("pairing again was tried", || {
        count(&p.control.pairings_asked) == 1
    })
    .await;
    let url = format!("{}/api/orchestration/shell", p.rt.upstream_url());
    let e = p.rt.upstream_headers("GET", &url).await.unwrap_err();
    assert!(e.downcast_ref::<dpop::Refused>().is_some(), "{e:#}");
    assert!(e.to_string().contains("Open in T3 Code"), "{e}");
    // Through the proxy, the same message, and T3 is not asked again each time.
    let res = reqwest::Client::new()
        .get(format!(
            "http://127.0.0.1:{}/api/orchestration/shell",
            p.port
        ))
        .header("authorization", format!("Bearer {CLIENT_TOKEN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 503);
    assert!(res.text().await.unwrap().contains("Open in T3 Code"));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(count(&p.control.pairings_asked), 1);
    assert_eq!(count(&p.t3.bearer_seen), 0);
    // A new pairing (Open in T3 Code) ends it.
    p.t3.refuse_renewal.store(false, Ordering::SeqCst);
    p.rt.set_credential(paired(&p.t3, &p.rt.upstream_url()).await)
        .await;
    p.rt.prime().await.unwrap();
}

struct Creds;

#[async_trait]
impl Credentials for Creds {
    async fn bearer(&self) -> crate::api::Result<String> {
        Ok("iba_x".into())
    }
    async fn refreshed(&self) -> crate::api::Result<String> {
        Ok("iba_x".into())
    }
}

/// The instabox API for `hub.pair_link`: the published port, and `exec` answering `stdout`.
async fn instabox_api(
    upstream_url: &str,
    exec_stdout: Option<&str>,
) -> (wiremock::MockServer, Client) {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let api = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/machines/m_test/ports"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"url": upstream_url})))
        .mount(&api)
        .await;
    let exec = Mock::given(method("POST")).and(path("/v1/machines/m_test/exec"));
    match exec_stdout {
        Some(out) => exec
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"stdout": out, "stderr": "", "exitCode": 0})),
            )
            .expect(1),
        None => exec.respond_with(ResponseTemplate::new(500)).expect(0),
    }
    .mount(&api)
    .await;
    let client = Client::new(url::Url::parse(&api.uri()).unwrap(), Arc::new(Creds));
    (api, client)
}

#[tokio::test]
async fn the_switch_guide_is_due_until_t3_code_pairs_or_it_is_finished() {
    let (t3, upstream_url) = fake_t3(true).await;
    let (_api, client) = instabox_api(&upstream_url, None).await;
    let tokens = Arc::new(MemoryTokenStore::default());
    let control = FakeControl::new("running", true, t3.clone());
    let hub = |dir: &std::path::Path| {
        Hub::new(
            dir.to_path_buf(),
            control.clone(),
            tokens.clone(),
            tuning(Duration::from_secs(60)),
            false,
        )
    };
    let dir = tempfile::tempdir().unwrap();
    let first = hub(dir.path());
    assert_eq!(first.switch_guide("m_test").await, None, "no proxy yet");
    first.pair_link(&client, "m_test", "qa").await.unwrap();
    // Paired through the proxy, but T3 Code has not used it yet: due.
    let guide = first.switch_guide("m_test").await.unwrap();
    assert_eq!(guide.old_address, upstream_url);
    // What the guide copies is what T3 Code's Add route takes: a pairing URL of the proxy,
    // whose code makes T3 Code a client of it.
    let url = first.pairing_url("m_test").await.unwrap();
    let (base, code) = url.split_once("/pair#token=").unwrap();
    assert_eq!(base, guide.new_address);
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
    assert_eq!(res.status(), 200);
    assert_eq!(first.switch_guide("m_test").await, None, "T3 Code paired");

    // Finished or turned off: not due, and kept in the record for the next start.
    let dir = tempfile::tempdir().unwrap();
    let second = hub(dir.path());
    second.pair_link(&client, "m_test", "qa").await.unwrap();
    assert!(second.switch_guide("m_test").await.is_some());
    second.finish_switch_guide("m_test").await;
    assert_eq!(second.switch_guide("m_test").await, None);
    let saved: Record = serde_json::from_slice(
        &std::fs::read(super::runtime::record_path(&dir.path().join("m_test"))).unwrap(),
    )
    .unwrap();
    assert!(saved.switch_guide_done);
}

#[tokio::test]
async fn pairing_goes_through_the_proxy() {
    let (t3, upstream_url) = fake_t3(true).await;
    // A T3 with DPoP: no bearer session is issued (no exec of `t3 auth session issue`).
    let (_api, client) = instabox_api(&upstream_url, None).await;
    let dir = tempfile::tempdir().unwrap();
    let tokens = Arc::new(MemoryTokenStore::default());
    let control = FakeControl::new("running", true, t3.clone());
    let hub = Hub::new(
        dir.path().to_path_buf(),
        control.clone(),
        tokens.clone(),
        tuning(Duration::from_secs(60)),
        false,
    );

    let link = hub.pair_link(&client, "m_test", "qa").await.unwrap();
    // The upstream session is a DPoP one, kept with its key.
    let stored = Credential::parse(&tokens.load("m_test").unwrap().unwrap());
    assert!(stored.is_dpop());
    assert_eq!(count(&t3.exchanges), 1);
    assert_eq!(count(&control.pairings_asked), 1);
    let pairing: String =
        url::form_urlencoded::parse(link.strip_prefix("t3code://pair?").unwrap().as_bytes())
            .find(|(k, _)| k == "pairingUrl")
            .unwrap()
            .1
            .into_owned();
    let (base, code) = pairing.split_once("/pair#token=").unwrap();
    let port: u16 = base
        .strip_prefix("http://localhost:")
        .unwrap()
        .parse()
        .unwrap();
    assert!(port >= super::hub::BASE_PORT);
    let at = |p: &str| format!("http://127.0.0.1:{port}{p}");
    let http = reqwest::Client::new();

    // What T3 Code reads before it shows Confirm: the sandbox's environment, under its name.
    let d: Value = http
        .get(at("/.well-known/t3/environment"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        (d["environmentId"].as_str(), d["label"].as_str()),
        (Some("env-1"), Some("qa"))
    );

    // Confirm: the code becomes the proxy's own token, once.
    let form = [
        (
            "grant_type",
            "urn:ietf:params:oauth:grant-type:token-exchange",
        ),
        ("subject_token", code),
    ];
    let res = http
        .post(at("/oauth/token"))
        .form(&form)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let token: Value = res.json().await.unwrap();
    let access = token["access_token"].as_str().unwrap().to_owned();
    assert_eq!(token["scope"], "orchestration:read orchestration:operate");
    let again = http
        .post(at("/oauth/token"))
        .form(&form)
        .send()
        .await
        .unwrap();
    assert_eq!(again.status(), 400, "a code works once");

    // The token opens the proxy, as `Bearer` or as `DPoP` (T3 Code 0.1.11 and later).
    for scheme in ["Bearer", "DPoP"] {
        let res = http
            .get(at("/api/auth/session"))
            .header("authorization", format!("{scheme} {access}"))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200, "{scheme}");
    }
    let none = http.get(at("/api/auth/session")).send().await.unwrap();
    assert_eq!(none.status(), 401);
    // Only requests addressed to this computer by name: no DNS rebinding.
    let rebound = http
        .get(at("/.well-known/t3/environment"))
        .header("host", "evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(rebound.status(), 403);

    // The socket opens with a ticket; T3 Code's DPoP proof stays here, the proxy proves its own.
    let ticket: Value = http
        .post(at("/api/auth/websocket-ticket/rpc"))
        .header("authorization", format!("DPoP {access}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let url = format!(
        "ws://127.0.0.1:{port}/ws?wsTicket={}&dpop=proof.for.the.proxy&clientSurface=desktop",
        ticket["ticket"].as_str().unwrap()
    );
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    send(&mut ws, request("1", "server.getConfig", json!({}))).await;
    assert_eq!(next(&mut ws).await["requestId"], "1");
    let queries = t3.queries.lock().unwrap().clone();
    assert_eq!(
        queries,
        vec!["orchestrationProtocol=2&clientSurface=desktop".to_string()]
    );

    // What is kept on disk holds no secret.
    let record = std::fs::read_to_string(dir.path().join("m_test").join("record.json")).unwrap();
    let Credential::Dpop { key, token: up, .. } = &stored else {
        unreachable!()
    };
    assert!(
        !record.contains(&access)
            && !record.contains(up.as_str())
            && !record.contains(key.as_str())
    );
    assert!(record.contains(&token_hash(&access)));

    // Open again: the same port and the same session.
    let link2 = hub.pair_link(&client, "m_test", "qa").await.unwrap();
    assert!(link2.contains(&format!("localhost%3A{port}%2Fpair")));
    assert_eq!(count(&t3.exchanges), 1);
    assert_eq!(count(&t3.bearer_seen), 0);
}

#[tokio::test]
async fn a_t3_without_dpop_gets_a_bearer_session_only_when_allowed() {
    let (t3, upstream_url) = fake_t3(false).await;
    let control = FakeControl::new("running", true, t3.clone());
    let dir = tempfile::tempdir().unwrap();
    // Without `INSTAADE_T3_PROXY=any`: a clear refusal, and nothing is issued.
    let (_api, client) = instabox_api(&upstream_url, None).await;
    let tokens = Arc::new(MemoryTokenStore::default());
    let hub = Hub::new(
        dir.path().to_path_buf(),
        control.clone(),
        tokens.clone(),
        tuning(Duration::from_secs(60)),
        false,
    );
    let e = hub.pair_link(&client, "m_test", "qa").await.unwrap_err();
    assert!(e.to_string().contains("INSTAADE_T3_PROXY=any"), "{e}");
    assert_eq!(tokens.load("m_test").unwrap(), None);
    // With it: the bearer session `t3 auth session issue` prints.
    let (_api, client) = instabox_api(&upstream_url, Some(&format!("{UP_TOKEN}\n"))).await;
    let hub = Hub::new(
        dir.path().to_path_buf(),
        control.clone(),
        tokens.clone(),
        tuning(Duration::from_secs(60)),
        true,
    );
    hub.pair_link(&client, "m_test", "qa").await.unwrap();
    assert_eq!(
        Credential::parse(&tokens.load("m_test").unwrap().unwrap()),
        Credential::Bearer {
            token: UP_TOKEN.into()
        }
    );
    assert_eq!(count(&control.pairings_asked), 0);
}
