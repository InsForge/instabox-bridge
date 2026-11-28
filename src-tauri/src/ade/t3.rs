//! T3 Code. On this computer: find the T3 CLI and read its version. On a sandbox: shell
//! scripts run over the API's exec, and parsers for their output. The sandbox layout matches
//! the instabox t3 template.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::ade::{Pairing, Platform, Probe};
use crate::process::{RunError, command, run_bounded};

pub const ID: &str = "t3";
pub const NAME: &str = "T3 Code";
pub const SHORT_NAME: &str = "T3";
pub const PLATFORMS: &[Platform] = &[Platform::MacOs, Platform::Windows];
pub const DOWNLOAD_URL: &str = "https://github.com/pingdotgg/t3code/releases/latest";
pub const URL_SCHEME: &str = "t3code";

// ---- this computer ----

/// A stuck `t3 --version` counts as not found after this long, so it cannot hang startup.
const LOCAL_TIMEOUT: Duration = Duration::from_secs(5);

/// Path of the T3 CLI, if it exists.
pub fn locate() -> Option<PathBuf> {
    let bin = dirs::home_dir()?.join(".t3").join("bin");
    let names: &[&str] = if cfg!(windows) {
        &["t3.cmd", "t3.exe"]
    } else {
        &["t3"]
    };
    names.iter().map(|n| bin.join(n)).find(|p| p.is_file())
}

/// Parse `t3 v0.1.10` into `0.1.10`.
pub fn parse_version(out: &str) -> Option<String> {
    out.split_whitespace().find_map(|token| {
        let v = token.strip_prefix('v').unwrap_or(token);
        let core = v.split(['-', '+']).next().unwrap_or("");
        let parts: Vec<&str> = core.split('.').collect();
        let ok = parts.len() == 3
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
        ok.then(|| v.to_string())
    })
}

/// Run `<cli> --version` and parse the result. Gives up after 5 s.
pub fn version(cli: &Path) -> Result<String> {
    version_within(cli, LOCAL_TIMEOUT)
}

fn version_within(cli: &Path, timeout: Duration) -> Result<String> {
    let mut cmd = command(cli);
    cmd.arg("--version");
    let (code, stdout, _) = match run_bounded(cmd, timeout) {
        Ok(c) => c,
        Err(RunError::Spawn(e)) => bail!("failed to run {}: {e}", cli.display()),
        Err(RunError::TimedOut) => bail!(
            "{} --version did not answer within {} s",
            cli.display(),
            timeout.as_secs_f32()
        ),
    };
    if code != Some(0) {
        bail!("{} --version exited with {code:?}", cli.display());
    }
    let text = String::from_utf8_lossy(&stdout);
    parse_version(&text).with_context(|| format!("unrecognized version output: {}", text.trim()))
}

// ---- the sandbox ----

/// The activity probe, written to the sandbox by the setup script.
pub const PROBE: Probe = Probe {
    name: "t3",
    script: include_str!("../../assets/t3-probe"),
};

const PROBE_EOF: &str = "INSTABOX_PROBE_EOF";

/// The preload `t3 serve` loads through NODE_OPTIONS, written to the sandbox by the setup. The
/// instabox port proxy sends `Host: 127.0.0.1:3774` and the public host in `X-Forwarded-Host`;
/// T3 checks DPoP proofs against a URL built from Host, so this puts the public host back in
/// Host for requests that came through the proxy.
pub const FORWARDED_HOST: &str = include_str!("../../assets/t3-forwarded-host.cjs");

const FORWARDED_HOST_EOF: &str = "INSTABOX_PRELOAD_EOF";

/// The port `t3 serve` listens on, published for pairing.
pub const PORT: u16 = 3774;

pub const PAIR_SCRIPT: &str =
    r#"t3 pair --base-dir "$HOME/.t3-instabox/$(cat /etc/machine-id)" --label 'Instabox'"#;
/// Prints T3's version, then `setup <id>` with the setup the running server was started with
/// (`setup_line`), when its preload is in place.
pub const VERSION_SCRIPT: &str = r#"t3 --version 2>/dev/null || true; d="$HOME/.local/share/t3-instabox"; [ -f "$d/forwarded-host.cjs" ] && sed 's/^/setup /' "$d/setup" 2>/dev/null || true"#;
// The setup script starts an inactive service and waits for it, so stop it first.
pub const STOP_SCRIPT: &str =
    "sudo systemctl stop t3-instabox; sudo systemctl reset-failed t3-instabox 2>/dev/null || true";

/// The first T3 whose desktop app pairs with DPoP and whose server asks for it (it advertises
/// `accessTokenRenewal`). Both changes came in this nightly, which sorts before 0.1.11.
pub const DPOP_SINCE: &str = "0.1.11-nightly.20261118.4903";

/// True when the desktop app at `desktop` pairing with the server at `server` uses DPoP: both
/// are `DPOP_SINCE` or later. Only then must the server run the preload (`FORWARDED_HOST`).
/// A version that does not parse counts as no.
pub fn pairs_with_dpop(desktop: &str, server: Option<&str>) -> bool {
    let since = semver::Version::parse(DPOP_SINCE).expect("DPOP_SINCE is a version");
    let at_least = |v: &str| semver::Version::parse(v).is_ok_and(|v| v >= since);
    at_least(desktop) && server.is_some_and(at_least)
}

pub const PAIRING: Pairing = Pairing {
    script: PAIR_SCRIPT,
    port: PORT,
    code: parse_pair_code,
    link: pairing_link,
};

/// Downloads T3 `$v` to /opt/t3 when it is not there. The setup's first part.
const INSTALL_TEMPLATE: &str = r##"v=__VERSION__
name="t3-$v-linux-x64.tar.gz"
base="https://github.com/pingdotgg/t3code/releases/download/v$v"
if [ ! -x "/opt/t3/$v/t3" ]; then
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  curl -fsSL -o "$tmp/$name" "$base/$name"
  curl -fsSL -o "$tmp/SHA256SUMS" "$base/SHA256SUMS"
  awk -v f="$name" '$2 == f || $2 == "*" f' "$tmp/SHA256SUMS" > "$tmp/want"
  [ -s "$tmp/want" ] || { echo "no checksum for $name in SHA256SUMS" >&2; exit 1; }
  (cd "$tmp" && sha256sum -c --quiet want)
  sudo install -d -m 0755 "/opt/t3/$v"
  sudo tar -xzf "$tmp/$name" -C "/opt/t3/$v" --strip-components=1 --no-same-owner
  rm -rf "$tmp"
fi
"##;

/// The probe, the preload, the launcher, the unit and the service. With `$v` set (the setup)
/// it also links T3 `$v`; without it (the refresh) T3 keeps its version. It stages every file
/// in a temporary directory first. When one differs from what is in place, or T3 must be
/// linked, a running T3 must be idle by the staged probe: else it exits 3 before it writes
/// anything (the setup's download only adds an unused `/opt/t3/<v>`). Then it writes them
/// all, and restarts T3 when something changed. The `setup` file records the setup the server
/// was started with.
const SERVICE_TEMPLATE: &str = r##"probe=/usr/local/lib/instabox/activity.d/t3
dir="$HOME/.local/share/t3-instabox"
new=$(mktemp -d)
trap 'rm -rf "$new"' EXIT
cat > "$new/t3-probe" <<'__PROBE_EOF__'
__PROBE__
__PROBE_EOF__
cat > "$new/forwarded-host.cjs" <<'__PRELOAD_EOF__'
__PRELOAD__
__PRELOAD_EOF__
cat > "$new/t3-instabox.service" <<U
[Unit]
Description=T3 Code on Instabox
After=network-online.target instabox-data-init.service
RequiresMountsFor=/data
StartLimitIntervalSec=300
StartLimitBurst=10
[Service]
User=$(id -u)
Environment=HOME=$HOME LANG=en_US.UTF-8 XDG_RUNTIME_DIR=/run/user/$(id -u)
Environment=NODE_OPTIONS=--require=$dir/forwarded-host.cjs
WorkingDirectory=/data/workspace
ExecStart=$dir/start
Restart=on-failure
RestartSec=3
LogFilterPatterns=~^Token:
LogFilterPatterns=~^Pairing URL:
LogFilterPatterns=~pairingUrl:
LogFilterPatterns=~^\\s*(\\xe2\\x96[\\x80\\x84\\x88]\\s*)+\$
[Install]
WantedBy=multi-user.target
U
relink=0
if [ -n "${v:-}" ] && [ "$(readlink /usr/local/bin/t3)" != "/opt/t3/$v/t3" ]; then relink=1; fi
changed=0
cmp -s "$new/t3-probe" "$probe" || changed=1
cmp -s "$new/forwarded-host.cjs" "$dir/forwarded-host.cjs" || changed=1
cmp -s "$new/t3-instabox.service" /etc/systemd/system/t3-instabox.service || changed=1
[ "$(cat "$dir/setup" 2>/dev/null)" = __SETUP_ID__ ] || changed=1
# Up to here only "$new" is written (and by the setup an unused /opt/t3/$v): a busy T3 keeps
# everything it has. The check runs the staged probe, so a sandbox without one is covered.
if [ "$relink" = 1 ] || [ "$changed" = 1 ]; then
  if systemctl is-active -q t3-instabox; then
    answer=$(python3 "$new/t3-probe" 2>/dev/null || true)
    case "$answer" in
      *'"busy": false'*|*'"busy":false'*) ;;
      *) echo "T3 is busy, update deferred" >&2; exit 3 ;;
    esac
  fi
fi
sudo install -d -m 0755 /usr/local/lib/instabox/activity.d
sudo install -m 0755 "$new/t3-probe" "$probe"
if [ "$relink" = 1 ]; then sudo ln -sfn "/opt/t3/$v/t3" /usr/local/bin/t3; fi
first=0
[ -e /etc/systemd/system/t3-instabox.service ] || first=1
(umask 077
mkdir -p "$dir"
[ -e "$dir/start" ] || cat > "$dir/start" <<'S'
#!/bin/sh
set -eu
export PATH="$HOME/.local/bin:/usr/local/bin:/usr/bin:/bin"
identity=$(cat /etc/machine-id)
case "$identity" in ''|*[!a-f0-9]*) exit 1;; esac
exec t3 serve --host 0.0.0.0 --port 3774 --base-dir "$HOME/.t3-instabox/$identity" /data/workspace
S
chmod 0700 "$dir/start"
cp "$new/forwarded-host.cjs" "$dir/forwarded-host.cjs.new"
mv -f "$dir/forwarded-host.cjs.new" "$dir/forwarded-host.cjs")
sudo install -m 0644 "$new/t3-instabox.service" /etc/systemd/system/t3-instabox.service
sudo systemctl daemon-reload
# Every run: a service whose first install was cut short still starts at boot.
sudo systemctl enable -q t3-instabox
if [ "$first" = 1 ]; then
  sudo systemctl start t3-instabox
elif [ "$relink" = 1 ] || [ "$changed" = 1 ] || ! systemctl is-active -q t3-instabox; then
  sudo systemctl restart t3-instabox
fi
# t3 pair finds the server by this file, written after the port opens.
state="$HOME/.t3-instabox/$(cat /etc/machine-id)/userdata/server-runtime.json"
for _ in $(seq 60); do
  if [ -s "$state" ] && curl -s -o /dev/null http://127.0.0.1:3774/; then
    echo __SETUP_ID__ > "$dir/setup"
    exit 0
  fi
  sleep 1
done
echo "T3 server did not start" >&2; exit 1"##;

/// True when `v` is safe to put in a script: `[0-9A-Za-z.+-]+`.
pub fn valid_version(v: &str) -> bool {
    !v.is_empty()
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-'))
}

/// The service part with the probe and the preload in it, and `__SETUP_ID__` still open.
fn service_template() -> String {
    for (text, eof) in [
        (PROBE.script, PROBE_EOF),
        (FORWARDED_HOST, FORWARDED_HOST_EOF),
    ] {
        assert!(!text.contains(eof), "{eof} is in the text it ends");
    }
    let probe = PROBE.script.strip_suffix('\n').unwrap_or(PROBE.script);
    let preload = FORWARDED_HOST.strip_suffix('\n').unwrap_or(FORWARDED_HOST);
    SERVICE_TEMPLATE
        .replace("__PROBE_EOF__", PROBE_EOF)
        .replace("__PROBE__", probe)
        .replace("__PRELOAD_EOF__", FORWARDED_HOST_EOF)
        .replace("__PRELOAD__", preload)
}

/// Names this build's service part: the first 16 hex digits of its SHA-256. Any change to the
/// probe, the preload, the launcher or the unit gives a new id.
pub fn setup_id() -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, service_template().as_bytes());
    digest.as_ref()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The line `VERSION_SCRIPT` prints for a server started with this build's setup.
pub fn setup_line() -> String {
    format!("setup {}", setup_id())
}

/// True when the output of `VERSION_SCRIPT` shows a server started with this build's setup.
pub fn setup_current(version_output: &str) -> bool {
    let want = setup_line();
    version_output.lines().any(|l| l.trim() == want)
}

fn service_script() -> String {
    service_template().replace("__SETUP_ID__", &setup_id())
}

/// Script that installs T3 `version`, installs the probe and the preload, and starts the
/// service. A version outside `[0-9A-Za-z.+-]+` gives a script that only fails.
pub fn setup_script(version: &str) -> String {
    if !valid_version(version) {
        return "echo 'invalid T3 version' >&2; exit 1".to_string();
    }
    format!(
        "set -eu\n{}{}",
        INSTALL_TEMPLATE.replace("__VERSION__", version),
        service_script()
    )
}

/// Script that brings the service of the T3 on the sandbox to this build's setup, at the
/// version it has. It exits 3 when T3 is busy and a restart must wait.
pub fn refresh_script() -> String {
    format!("set -eu\n{}", service_script())
}

/// The code from the `Token: <code>` line of `t3 pair`.
pub fn parse_pair_code(stdout: &str) -> Option<String> {
    stdout.lines().find_map(|l| {
        let code = l.trim().strip_prefix("Token:")?.trim();
        (!code.is_empty()).then(|| code.to_string())
    })
}

/// The pairing URL T3 Code takes for a server at `port_url` and a pairing code.
pub fn pairing_url(port_url: &str, code: &str) -> String {
    format!("{}/pair#token={}", port_url.trim_end_matches('/'), code)
}

/// The `t3code://pair` link for a published port URL and a pairing code.
pub fn pairing_link(port_url: &str, code: &str) -> String {
    let pairing = pairing_url(port_url, code);
    let mut out = String::from("t3code://pair?pairingUrl=");
    for b in pairing.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ade::SETUP_EXIT_BUSY;

    #[test]
    fn parses_stable() {
        assert_eq!(parse_version("t3 v0.1.10\n").as_deref(), Some("0.1.10"));
    }

    #[test]
    fn parses_nightly() {
        assert_eq!(
            parse_version("t3 v0.1.11-nightly.20261119.4918").as_deref(),
            Some("0.1.11-nightly.20261119.4918")
        );
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_version("garbage"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn version_errors_on_missing_binary() {
        assert!(version(Path::new("/nonexistent/t3")).is_err());
    }

    #[cfg(unix)]
    fn fake_t3(body: &str) -> (tempfile::TempDir, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let cli = dir.path().join("t3");
        std::fs::write(&cli, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
        (dir, cli)
    }

    #[cfg(unix)]
    #[test]
    fn a_stuck_t3_counts_as_not_found() {
        let (_dir, cli) = fake_t3("exec sleep 10");
        let start = std::time::Instant::now();
        let e = version_within(&cli, Duration::from_millis(200)).unwrap_err();
        assert!(start.elapsed() < Duration::from_secs(3));
        assert!(e.to_string().contains("did not answer"), "{e}");
        let (_dir, cli) = fake_t3("echo 't3 v0.1.10'");
        assert_eq!(version_within(&cli, LOCAL_TIMEOUT).unwrap(), "0.1.10");
    }

    #[test]
    fn download_url_is_latest_release() {
        assert_eq!(
            DOWNLOAD_URL,
            "https://github.com/pingdotgg/t3code/releases/latest"
        );
    }

    #[test]
    fn setup_script_pins_version() {
        let s = setup_script("0.1.10");
        for want in [
            "/opt/t3/$v/t3",
            "v=0.1.10",
            "releases/download/v$v",
            "t3-$v-linux-x64.tar.gz",
            "SHA256SUMS",
            "sha256sum -c",
            "activity.d/t3",
            "0755",
            "systemctl restart t3-instabox",
            "LogFilterPatterns=~^Token:",
        ] {
            assert!(s.contains(want), "missing {want}");
        }
        assert!(s.contains("#!/usr/bin/env python3"));
    }

    #[test]
    fn setup_script_checks_probe_before_link_change() {
        let s = setup_script("0.1.10");
        let probe = s.find("answer=$(python3 \"$new/t3-probe\"").unwrap();
        let exit3 = s.find("exit 3").unwrap();
        let link = s.find("ln -sfn").unwrap();
        assert!(probe < exit3 && exit3 < link);
        assert_eq!(SETUP_EXIT_BUSY, 3);
    }

    #[test]
    fn setup_script_enables_the_service_on_every_run() {
        let s = setup_script("0.1.10");
        let enable = s.find("\nsudo systemctl enable -q t3-instabox\n").unwrap();
        let first = s.find("if [ \"$first\" = 1 ]; then").unwrap();
        assert!(
            enable < first,
            "enable runs before and outside the first-install branch"
        );
        assert_eq!(s.matches("systemctl enable").count(), 1);
        // The restart rules stay: start on a first install, restart on a change or when down.
        assert!(s.contains(
            "elif [ \"$relink\" = 1 ] || [ \"$changed\" = 1 ] || ! systemctl is-active -q t3-instabox; then\n  sudo systemctl restart t3-instabox"
        ));
    }

    #[test]
    fn the_refresh_is_the_setup_without_the_version() {
        let setup = setup_script("0.1.12");
        let refresh = refresh_script();
        let body = refresh.strip_prefix("set -eu\n").unwrap();
        assert!(setup.starts_with("set -eu\nv=0.1.12\n"));
        assert!(setup.ends_with(body), "the setup ends with the refresh");
        // The refresh downloads nothing and keeps the version: `$v` is never set there.
        for absent in ["v=", "curl -fsSL", "releases/download"] {
            assert!(!refresh.contains(absent), "refresh has {absent}");
        }
        assert!(refresh.contains(
            "if [ -n \"${v:-}\" ] && [ \"$(readlink /usr/local/bin/t3)\" != \"/opt/t3/$v/t3\" ]; then relink=1; fi"
        ));
        assert!(refresh.contains("if [ \"$relink\" = 1 ]; then sudo ln -sfn"));
    }

    #[test]
    fn setup_writes_the_preload_and_the_service_loads_it() {
        let s = refresh_script();
        let preload = FORWARDED_HOST.strip_suffix('\n').unwrap();
        assert!(s.contains(&format!(
            "cat > \"$new/forwarded-host.cjs\" <<'{FORWARDED_HOST_EOF}'\n{preload}\n{FORWARDED_HOST_EOF}\n"
        )));
        assert!(s.contains("mv -f \"$dir/forwarded-host.cjs.new\" \"$dir/forwarded-host.cjs\""));
        assert!(s.contains("\nEnvironment=NODE_OPTIONS=--require=$dir/forwarded-host.cjs\n"));
        assert!(s.contains("\nExecStart=$dir/start\n"));
        // The launcher is the template's, written only when missing: the unit carries the preload.
        assert!(s.contains("[ -e \"$dir/start\" ] || cat > \"$dir/start\" <<'S'"));
        let launcher = &s[s.find("<<'S'\n").unwrap()..s.find("\nS\n").unwrap()];
        assert!(!launcher.contains("NODE_OPTIONS"));
        assert!(launcher.ends_with(&format!(
            "exec t3 serve --host 0.0.0.0 --port {PORT} --base-dir \"$HOME/.t3-instabox/$identity\" /data/workspace"
        )));
    }

    /// The script without the bodies of its here-documents (the probe, the preload, the unit).
    fn shell_lines(script: &str) -> String {
        let mut out = String::new();
        let mut end: Option<String> = None;
        for line in script.lines() {
            if let Some(e) = &end {
                if line == e {
                    end = None;
                }
                continue;
            }
            if let Some((_, marker)) = line.rsplit_once("<<") {
                end = Some(marker.trim_matches('\'').to_string());
            }
            out.push_str(line);
            out.push('\n');
        }
        assert!(end.is_none(), "an unended here-document");
        out
    }

    #[test]
    fn a_change_waits_for_an_idle_t3_before_anything_is_written() {
        let s = setup_script("0.1.12");
        let at = |needle: &str| s.find(needle).unwrap_or_else(|| panic!("missing {needle}"));
        // What is in place: the probe, the preload, the unit and the stamp of the server.
        let checks = [
            "cmp -s \"$new/t3-probe\" \"$probe\" || changed=1",
            "cmp -s \"$new/forwarded-host.cjs\" \"$dir/forwarded-host.cjs\" || changed=1",
            "cmp -s \"$new/t3-instabox.service\" /etc/systemd/system/t3-instabox.service || changed=1",
            "[ \"$(cat \"$dir/setup\" 2>/dev/null)\" = __SETUP_ID__ ] || changed=1",
        ];
        let id = setup_id();
        let checks: Vec<String> = checks
            .iter()
            .map(|c| c.replace("__SETUP_ID__", &id))
            .collect();
        let gate = at(
            "if [ \"$relink\" = 1 ] || [ \"$changed\" = 1 ]; then\n  if systemctl is-active -q t3-instabox; then\n    answer=$(",
        );
        for c in &checks {
            assert!(at(c) < gate, "{c} comes before the busy check");
        }
        let exit3 = at("exit 3");
        for write in [
            "sudo install -d -m 0755 /usr/local/lib/instabox/activity.d",
            "sudo install -m 0755 \"$new/t3-probe\" \"$probe\"",
            "sudo ln -sfn",
            "cat > \"$dir/start\"",
            "mv -f \"$dir/forwarded-host.cjs.new\"",
            "sudo install -m 0644 \"$new/t3-instabox.service\" /etc/systemd/system/t3-instabox.service",
            "sudo systemctl daemon-reload",
            "systemctl restart t3-instabox",
        ] {
            assert!(
                gate < exit3 && exit3 < at(write),
                "{write} after the busy check"
            );
        }
        // Before the check, `sudo` only unpacks the setup's new release, and files go only to
        // the staging directory.
        let refresh = refresh_script();
        for (script, sudo) in [(&s, 2), (&refresh, 0)] {
            let before = shell_lines(
                &script[..script
                    .find("if [ \"$relink\" = 1 ] || [ \"$changed\" = 1 ]")
                    .unwrap()],
            );
            let lines: Vec<&str> = before.lines().filter(|l| l.contains("sudo ")).collect();
            assert_eq!(lines.len(), sudo, "{lines:?}");
            assert!(
                lines.iter().all(|l| l.contains("\"/opt/t3/$v\"")),
                "{lines:?}"
            );
            for l in before
                .lines()
                .filter(|l| l.contains(" > ") || l.starts_with("cat >"))
            {
                assert!(l.contains("\"$new/") || l.contains("\"$tmp/"), "{l}");
            }
            assert!(!before.contains("mv ") && !before.contains("ln -s"));
        }
        // The stamp is written only once the server answers, and is what VERSION_SCRIPT reads.
        let stamp = at(&format!("echo {id} > \"$dir/setup\"\n    exit 0"));
        assert!(at("curl -s -o /dev/null http://127.0.0.1:3774/") < stamp);
        assert!(VERSION_SCRIPT.contains("\"$d/setup\""));
        assert!(VERSION_SCRIPT.contains("[ -f \"$d/forwarded-host.cjs\" ]"));
    }

    #[test]
    fn setup_id_names_the_service_part() {
        let id = setup_id();
        assert_eq!(id.len(), 16);
        assert!(
            id.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
        assert_eq!(id, setup_id(), "the same build gives the same id");
        assert!(!service_template().contains(&id));
        assert_eq!(setup_line(), format!("setup {id}"));
        // Both scripts check the stamp and write it.
        for s in [setup_script("0.1.10"), refresh_script()] {
            assert_eq!(s.matches(&id).count(), 2);
            assert!(!s.contains("__SETUP_ID__"));
        }
    }

    #[test]
    fn pairing_uses_dpop_only_when_both_sides_have_it() {
        assert!(pairs_with_dpop(DPOP_SINCE, Some(DPOP_SINCE)));
        assert!(pairs_with_dpop("0.1.12", Some("0.1.12")));
        assert!(pairs_with_dpop(
            "0.1.11",
            Some("0.1.13-nightly.20261125.5258")
        ));
        // An older side pairs with a bearer token, which an old setup takes.
        assert!(!pairs_with_dpop("0.1.10", Some("0.1.12")));
        assert!(!pairs_with_dpop("0.1.12", Some("0.1.10")));
        assert!(!pairs_with_dpop(
            "0.1.11-nightly.20261118.4890",
            Some("0.1.12")
        ));
        assert!(!pairs_with_dpop(
            "0.1.12",
            Some("0.1.11-nightly.20261117.4851")
        ));
        // Unknown is no: the pairing goes ahead as before.
        assert!(!pairs_with_dpop("0.1.12", None));
        assert!(!pairs_with_dpop("junk", Some("0.1.12")));
    }

    #[test]
    fn setup_current_reads_the_version_script_output() {
        let line = setup_line();
        assert!(setup_current(&format!("t3 v0.1.12\n{line}\n")));
        assert!(setup_current(&format!("{line}\r\n")));
        assert!(!setup_current("t3 v0.1.12\n"));
        assert!(!setup_current("t3 v0.1.12\nsetup 0000000000000000\n"));
        assert!(!setup_current(""));
        // The setup line never reads as a version.
        assert_eq!(parse_version(&format!("{line}\n")), None);
        assert_eq!(
            parse_version(&format!("t3 v0.1.12\n{line}\n")).as_deref(),
            Some("0.1.12")
        );
    }

    #[test]
    fn preload_asset_is_for_the_t3_port() {
        assert!(FORWARDED_HOST.contains(&format!("const PORT = {PORT};")));
        // A CRLF checkout (Windows) would put \r into the file on the sandbox.
        assert!(!FORWARDED_HOST.contains('\r'));
    }

    #[test]
    fn setup_script_installs_the_probe_where_it_is_run() {
        let s = setup_script("0.1.10");
        let path = PROBE.path();
        assert!(s.starts_with("set -eu\nv=0.1.10\n"));
        assert!(s.contains(&format!("\nprobe={path}\n")));
        assert!(s.contains("sudo install -m 0755 \"$new/t3-probe\" \"$probe\"\n"));
        // The busy check asks the probe this build installs, from where it is staged.
        assert!(s.contains("cat > \"$new/t3-probe\" <<'"));
        assert!(s.contains("answer=$(python3 \"$new/t3-probe\" 2>/dev/null"));
        assert!(s.contains(&format!(":{PORT}/")) && s.contains(&format!("--port {PORT} ")));
    }

    #[cfg(unix)]
    #[test]
    fn setup_script_is_valid_sh() {
        let dir = tempfile::tempdir().unwrap();
        for (name, script) in [
            ("setup.sh", setup_script("0.1.10")),
            ("refresh.sh", refresh_script()),
            ("version.sh", VERSION_SCRIPT.to_string()),
        ] {
            let p = dir.path().join(name);
            std::fs::write(&p, script).unwrap();
            let st = std::process::Command::new("/bin/sh")
                .arg("-n")
                .arg(&p)
                .status()
                .unwrap();
            assert!(st.success(), "{name}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn version_script_prints_the_stamp_only_beside_the_preload() {
        let home = tempfile::tempdir().unwrap();
        let bin = tempfile::tempdir().unwrap();
        let run = || {
            let out = std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(VERSION_SCRIPT)
                .env("HOME", home.path())
                .env("PATH", format!("{}:/usr/bin:/bin", bin.path().display()))
                .output()
                .unwrap();
            assert!(out.status.success());
            String::from_utf8(out.stdout).unwrap()
        };
        // No T3 and no setup: nothing, and still exit 0.
        assert_eq!(run(), "");
        let (_t3dir, cli) = fake_t3("echo 't3 v0.1.12'");
        std::fs::copy(&cli, bin.path().join("t3")).unwrap();
        let dir = home.path().join(".local/share/t3-instabox");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("setup"), format!("{}\n", setup_id())).unwrap();
        assert_eq!(
            run(),
            "t3 v0.1.12\n",
            "a stamp without the preload is not current"
        );
        std::fs::write(dir.join("forwarded-host.cjs"), FORWARDED_HOST).unwrap();
        let out = run();
        assert_eq!(out, format!("t3 v0.1.12\n{}\n", setup_line()));
        assert!(setup_current(&out));
    }

    /// Sends one raw HTTP/1.1 request with `headers` and returns the response body.
    #[cfg(unix)]
    fn http_body(port: u16, headers: &[(&str, &str)]) -> String {
        use std::io::{Read, Write};
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut req = String::from("GET /x HTTP/1.1\r\n");
        for (k, v) in headers {
            req.push_str(&format!("{k}: {v}\r\n"));
        }
        if !headers.iter().any(|(k, _)| *k == "Connection") {
            req.push_str("Connection: close\r\n");
        }
        req.push_str("\r\n");
        s.write_all(req.as_bytes()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out.split_once("\r\n\r\n").unwrap().1.to_string()
    }

    /// Runs the preload in a Node HTTP server, as `t3 serve` loads it. Skipped without Node.
    #[cfg(unix)]
    #[test]
    fn preload_puts_the_public_host_in_host_for_proxied_requests_only() {
        use std::io::BufRead;
        let Ok(node) = which::which("node") else {
            eprintln!("node not found: skipped");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let preload = dir.path().join("forwarded-host.cjs");
        std::fs::write(&preload, FORWARDED_HOST).unwrap();
        // The sandbox's identity stamp, which the test machine has not.
        let identity = dir.path().join("identity.cjs");
        std::fs::write(
            &identity,
            r#"const fs = require("node:fs");
const read = fs.readFileSync;
fs.readFileSync = (p, ...a) => (p === "/etc/instabox/identity" ? "m_abc123\n" : read(p, ...a));
"#,
        )
        .unwrap();
        let server = dir.path().join("server.cjs");
        std::fs::write(
            &server,
            r#"const http = require("node:http");
const s = http.createServer((req, res) => {
  const i = req.rawHeaders.findIndex((h) => h.toLowerCase() === "host");
  res.end(JSON.stringify({ host: req.headers.host, raw: req.rawHeaders[i + 1], env: process.env.NODE_OPTIONS ?? null }));
});
s.on("upgrade", (req, socket) => {
  const b = req.headers.host;
  socket.end(`HTTP/1.1 400 No\r\nContent-Length: ${Buffer.byteLength(b)}\r\nConnection: close\r\n\r\n${b}`);
});
s.listen(0, "127.0.0.1", () => console.log(s.address().port));
"#,
        )
        .unwrap();
        let keep = format!("--require={}", identity.display());
        let mut child = std::process::Command::new(node)
            .arg(&server)
            .env(
                "NODE_OPTIONS",
                format!(
                    "{keep} --require={} --max-old-space-size=64",
                    preload.display()
                ),
            )
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        struct Kill(std::process::Child);
        impl Drop for Kill {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut line = String::new();
        std::io::BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let _child = Kill(child);
        let port: u16 = line.trim().parse().unwrap();
        let seen = |headers: &[(&str, &str)]| -> (String, String) {
            let v: serde_json::Value = serde_json::from_str(&http_body(port, headers)).unwrap();
            (
                v["host"].as_str().unwrap().to_string(),
                v["raw"].as_str().unwrap().to_string(),
            )
        };
        let proxied = |fwd: &'static str| [("Host", "127.0.0.1:3774"), ("X-Forwarded-Host", fwd)];
        let public = "3774-abc123.instabox.tech".to_string();
        let fixed = (public.clone(), public.clone());
        assert_eq!(seen(&proxied("3774-abc123.instabox.tech")), fixed);
        assert_eq!(seen(&proxied("3774-ABC123.Instabox.Tech")), fixed);
        let kept = ("127.0.0.1:3774".to_string(), "127.0.0.1:3774".to_string());
        for fwd in [
            "3774-other.instabox.tech",
            "3000-abc123.instabox.tech",
            "3774-abc123",
            "3774-abc123.instabox.tech, evil.example",
            "evil.example",
        ] {
            assert_eq!(seen(&proxied(fwd)), kept, "{fwd}");
        }
        assert_eq!(seen(&[("Host", "127.0.0.1:3774")]), kept);
        for host in ["localhost:3774", "127.0.0.1:3000"] {
            let (h, _) = seen(&[
                ("Host", host),
                ("X-Forwarded-Host", "3774-abc123.instabox.tech"),
            ]);
            assert_eq!(h, host);
        }
        // A WebSocket upgrade gets the same fix.
        let upgrade = http_body(
            port,
            &[
                ("Host", "127.0.0.1:3774"),
                ("X-Forwarded-Host", "3774-abc123.instabox.tech"),
                ("Upgrade", "websocket"),
                ("Connection", "Upgrade"),
            ],
        );
        assert_eq!(upgrade, public);
        // T3's children do not load the preload; other options stay.
        let v: serde_json::Value =
            serde_json::from_str(&http_body(port, &[("Host", "x")])).unwrap();
        assert_eq!(
            v["env"].as_str(),
            Some(format!("{keep} --max-old-space-size=64").as_str())
        );
    }

    #[test]
    fn heredoc_markers_are_not_in_the_text_they_end() {
        assert!(!PROBE.script.contains(PROBE_EOF));
        assert!(!FORWARDED_HOST.contains(FORWARDED_HOST_EOF));
        assert!(!FORWARDED_HOST.contains("\nS\n") && !FORWARDED_HOST.contains("\nU\n"));
    }

    #[test]
    fn setup_script_rejects_unsafe_version() {
        let s = setup_script("1.0;rm -rf /");
        assert!(!s.contains("rm -rf"));
        assert!(s.contains("exit 1"));
    }

    #[test]
    fn probe_asset_is_python() {
        assert!(PROBE.script.starts_with("#!/usr/bin/env python3\n"));
        assert!(PROBE.script.contains("statev2.sqlite"));
        // A CRLF checkout (Windows) would break the script on the sandbox.
        assert!(!PROBE.script.contains('\r'));
    }

    // ---- the probe, run with python3 on a T3 database ----

    /// A T3 base directory with a live server (this test's pid) and a database holding `runs`
    /// (thread, status) and `events` orchestration events, in the tables the probe reads.
    #[cfg(unix)]
    fn t3_base(
        python: &std::path::Path,
        runs: &[(&str, &str)],
        events: usize,
    ) -> tempfile::TempDir {
        let base = tempfile::tempdir().unwrap();
        let userdata = base.path().join("userdata");
        std::fs::create_dir_all(&userdata).unwrap();
        std::fs::write(
            userdata.join("server-runtime.json"),
            serde_json::json!({ "pid": std::process::id() }).to_string(),
        )
        .unwrap();
        let fill = "import json, sqlite3, sys\n\
            con = sqlite3.connect(sys.argv[1])\n\
            con.executescript('CREATE TABLE orchestration_v2_projection_threads (thread_id TEXT PRIMARY KEY, deleted_at TEXT);\
            CREATE TABLE orchestration_v2_projection_runs (run_id INTEGER PRIMARY KEY, thread_id TEXT, status TEXT);\
            CREATE TABLE orchestration_events (sequence INTEGER PRIMARY KEY AUTOINCREMENT, body TEXT);')\n\
            runs = json.loads(sys.argv[2])\n\
            con.executemany('INSERT OR IGNORE INTO orchestration_v2_projection_threads VALUES (?, NULL)', [(t,) for t, _ in runs])\n\
            con.executemany('INSERT INTO orchestration_v2_projection_runs (thread_id, status) VALUES (?, ?)', runs)\n\
            con.executemany('INSERT INTO orchestration_events (body) VALUES (?)', [('-',)] * int(sys.argv[3]))\n\
            con.commit()\n";
        let out = std::process::Command::new(python)
            .args(["-c", fill])
            .arg(userdata.join("statev2.sqlite"))
            .arg(serde_json::json!(runs).to_string())
            .arg(events.to_string())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        base
    }

    /// The probe's answer on `base`, run as the guest does (an empty environment but HOME and
    /// a short PATH), which must be one JSON line with `busy`, `seq` and `reason`.
    #[cfg(unix)]
    fn t3_probe(python: &std::path::Path, base: &std::path::Path) -> (bool, i64, String) {
        let probe = base.join("t3-probe");
        std::fs::write(&probe, PROBE.script).unwrap();
        let out = std::process::Command::new(python)
            .arg(&probe)
            .env_clear()
            .env("HOME", base)
            .env("PATH", "/usr/bin:/bin")
            .env("INSTABOX_T3_BASE_DIR", base)
            .output()
            .unwrap();
        let stdout = String::from_utf8(out.stdout).unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(stdout.lines().count(), 1, "{stdout}");
        let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
        assert_eq!(
            v["busy"].as_bool(),
            Some(crate::ade::parse_probe(&stdout).busy)
        );
        (
            v["busy"].as_bool().unwrap(),
            v["seq"].as_i64().unwrap(),
            v["reason"].as_str().unwrap().to_string(),
        )
    }

    #[cfg(unix)]
    #[test]
    fn the_probe_counts_the_active_runs_in_its_reason() {
        let Ok(python) = which::which("python3") else {
            eprintln!("python3 not found: skipped");
            return;
        };
        for (runs, want) in [
            (vec![], (false, "no active runs")),
            (
                vec![("t1", "completed"), ("t2", "waiting")],
                (false, "no active runs"),
            ),
            (
                vec![("t1", "completed"), ("t1", "running")],
                (true, "1 run active (running)"),
            ),
            (
                vec![("t1", "running"), ("t2", "running")],
                (true, "2 runs active (running)"),
            ),
            (
                vec![("t1", "running"), ("t2", "queued")],
                (true, "2 runs active (queued,running)"),
            ),
        ] {
            let base = t3_base(&python, &runs, 3);
            let (busy, seq, reason) = t3_probe(&python, base.path());
            assert_eq!((busy, reason.as_str()), want, "{runs:?}");
            assert_eq!(seq, 3);
        }
    }

    #[test]
    fn pair_code_and_link() {
        assert_eq!(
            parse_pair_code("Pairing URL: x\nToken: ABCD1234\n").as_deref(),
            Some("ABCD1234")
        );
        assert_eq!(parse_pair_code("nothing"), None);
        assert_eq!(
            pairing_link("https://p-1.instabox.sh", "ABCD1234"),
            "t3code://pair?pairingUrl=https%3A%2F%2Fp-1.instabox.sh%2Fpair%23token%3DABCD1234"
        );
    }
}
