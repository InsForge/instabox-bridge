# How InstaADE works with each ADE

The README's [Supported ADEs](../README.md#supported-ades) says what you need and what to click. This page says what InstaADE does on the sandbox and on your computer for each ADE. To add an ADE, see [Adding an ADE](../DEVELOPMENT.md#adding-an-ade).

When an ADE's app is installed more than once, such as in both `/Applications` and `~/Applications`, InstaADE uses the real copy with the newest version. The version it shows is the copy it opens.

Some ADEs get an activity probe in `/usr/local/lib/instabox/activity.d`, which tells instabox when the ADE is busy, so the sandbox sleeps only when it is idle. The ADE's setup installs it. When an ADE's Open runs nothing else on the sandbox (Orca, Paseo), Open also installs or updates its probe in the background, once per sandbox and probe version while InstaADE runs. Open never waits for it, and a failure only goes to the log: the next Open tries again. It does not run while a setup of the same sandbox runs.

## SSH tokens

InstaADE keeps two SSH tokens for each sandbox, in two keychain entries. Each token lasts 30 days. InstaADE uses the stored one while it has more than a day left, else it makes a new one.

- The **background token** is for an app that stays connected by itself and checks the connection every few seconds: cmux, Orca and Paseo. When instabox applies background tokens to the sandbox, the connections of this token do not wake the sandbox and do not count as activity. So an idle app does not keep the sandbox awake. An agent at work still keeps it awake, through the ADE's activity probe and the sandbox's terminals and CPU. InstaADE asks for this token with `background: true`. An instabox that does not know background tokens makes an ordinary token. InstaADE then uses that token in its place, and writes this to its log once.
- The **ordinary token** is for a login that you start: the SSH command in the sandbox's details, **Open in Terminal**, Herdr, Paradev's SSH address and the sign-in of Claude Code or Codex. Its connections wake the sandbox and count as activity.

The SSH host that Orca and Paseo use holds the background token. An older InstaADE wrote that host with its one token, which is now the ordinary one. The host keeps its alias. Its `User` changes to the background token the next time InstaADE starts, or at the next **Open**. So you do not add the host again in Orca or Paseo.

## T3 Code

InstaADE installs T3 at the same version as your T3 Code desktop app, then pairs them. On the sandbox it adds:

- T3 under `/opt/t3`, linked as `/usr/local/bin/t3`
- A `t3-instabox` service that runs the T3 server on port 3774, which InstaADE publishes for pairing
- An activity probe in `/usr/local/lib/instabox/activity.d/t3`, so the sandbox sleeps only when T3 is idle
- A preload, `~/.local/share/t3-instabox/forwarded-host.cjs`, that the service loads into `t3 serve` through `NODE_OPTIONS`. The instabox port proxy sends `Host: 127.0.0.1:3774` and the published address in `X-Forwarded-Host`. T3 checks DPoP proofs (T3 Code 0.1.12 and later) against a URL built from `Host`, so for a request that came through the proxy the preload puts the published address back in `Host`. It acts only when `Host` is `127.0.0.1:3774` and `X-Forwarded-Host` is `3774-<id>.<domain>`, where `<id>` is this sandbox's id from `/etc/instabox/identity` without its `m_` prefix (any id on a sandbox without that file). T3's own child processes do not load it.

Every **Open in T3 Code** pairs again. It wakes the sandbox, updates T3 to the app's version when it is behind and idle, and pairs them with a new code. The sandbox keeps the same address, so T3 Code updates the environment it saved, or adds it again when it lost it. On a sandbox that has T3 but was never paired from this computer, Open first runs the same setup as **Install T3 Code**.

InstaADE never moves T3 on a sandbox back to an older version. When the sandbox runs a newer T3 than the app, InstaADE only pairs.

The setup writes `~/.local/share/t3-instabox/setup` with the id of the setup the running server was started with. A sync (Open, resume, a new T3 Code version) whose sandbox T3 is behind the app runs the whole setup at the app's version. One whose sandbox T3 is at or ahead of the app's version, but has an older id, keeps that version and brings only the probe, the preload and the unit up to date. Either way T3 restarts only while it is idle. When it is busy, nothing T3 uses changes and the next sync tries again.

An older setup refuses a DPoP pairing. T3 Code pairs with DPoP only when both the desktop app and the T3 on the sandbox are 0.1.11-nightly.20261118.4903 or later. Only then does a pairing wait for the setup. When the setup is older, **Open**, **Pair again** and **Install** first bring it up to date. When T3 is busy or that fails, they do not pair, and show "T3 is busy on this sandbox. Try again when the agent is idle." or the error. When either side is older, the pairing uses a bearer token, which the older setup takes, and they pair as they always did. **Open** still syncs first, but a busy or failed refresh does not stop its pairing. **Pair again**, and **Install** on a sandbox with a newer T3, run no refresh. **Install** on a sandbox whose T3 is not newer runs the whole setup as always, and stops when it fails.

### Local proxy (prototype, off by default)

T3 Code keeps a WebSocket open to the sandbox's published port, and instabox counts an open connection to a published port as use, so a sandbox with T3 Code open never sleeps. The local proxy is a prototype that fixes this. It runs only when InstaADE starts with `INSTAADE_T3_PROXY=1` in its environment. Without the variable nothing changes.

- **Install**, **Open in T3 Code** and **Pair again** pair T3 Code with a proxy on this computer, `http://localhost:<port>`, instead of the published port. Each sandbox keeps its own port, from 37800 up. Before they pair, they bring an older setup up to date as they do without the proxy (the proxy pairs with T3 there with DPoP, which needs it), with the same errors. The proxy is used only with the T3 Code versions it was tried with (0.1.13). With `INSTAADE_T3_PROXY=any` it is used with every version.
- The proxy holds the connection to the sandbox while you use T3 Code. After 90 seconds with no action from you and no agent working, it closes the connection, and any stream it relays (such as a device view). The sandbox then pauses after its own auto sleep time.
- While the sandbox sleeps, the proxy answers T3 Code from what it recorded: the chat list, the chats it stored, settings and the like. Your next action, such as a message, wakes the sandbox, and T3 Code reconnects.
- The proxy wakes a sandbox the way instabox does for any use (SSH, a request to a public port): only a sandbox that paused itself for being idle. A sandbox you paused or stopped yourself, or that is pausing or stopping, stays offline. Your action in T3 Code then fails with "Sandbox offline" and what to do: resume or start it with `instabox resume` or `instabox start`, or use **Open in T3 Code**, which resumes or starts it as before.
- The proxy talks to T3 on the sandbox with a DPoP session of its own: a key for each sandbox, paired with `t3 auth pairing create` and kept in the keychain with its token. The token lasts an hour and is renewed while you use T3 Code. When T3 refuses it and a new pairing fails too, T3 Code shows an error until you use **Open in T3 Code** again. A T3 older than 0.1.11 has no DPoP: the proxy uses a bearer session with it only with `INSTAADE_T3_PROXY=any`. When InstaADE starts again without `any`, the proxy of a sandbox paired that way does not start; its session stays in the keychain for a later start with `any`.
- T3 Code saves a sandbox under the id of its T3 server, and refuses a pairing link for an environment it saved under another address. So for a sandbox it already saved with its published port, **Open in T3 Code** and **Pair again** show a guide instead of the link: in T3 Code's Settings, under Environments, add the proxy as a route of that environment (**Routes**, **Add route**, then paste the pairing link the guide copies into **Host**), then remove the old route. The environment keeps its chats and settings. T3 Code tries every saved route each time it connects, so the old route must go. The guide shows for a sandbox InstaADE paired before whose proxy T3 Code never paired with. **Done** or **Don't show again** ends it for that sandbox, and so does T3 Code pairing with the proxy. The copied link works once, for 15 minutes, and leaves the clipboard a minute later.

## cmux (macOS)

cmux connects over SSH by itself and uploads its own remote part, at the version of the cmux app, so InstaADE installs nothing and no versions must match. On the sandbox it runs one command, `mkdir -p ~/.local/state && chmod go-w ~/.local ~/.local/state`, because cmux refuses a group-writable state directory. **Open in cmux** wakes the sandbox first, since the SSH gateway refuses a paused one and the link does not wake it. Then it opens a `cmux://ssh` link with the sandbox's [background token](#ssh-tokens). When the sandbox does not wake, InstaADE shows the error and opens nothing.

The link holds the token, so InstaADE opens it only in the real cmux app in `/Applications` or `~/Applications`. Its bundle identifier must be `com.cmuxterm.app`, and `codesign` must find it signed with the Developer ID of Manaflow, Inc. (team `7WLXT3NR37`). InstaADE checks this when it looks for cmux, again before it wakes, creates or marks a sandbox for cmux, and again right before it makes or loads the token. It never opens the link in another handler of `cmux://` links or through cmux.com. Without that app, nothing opens and no token is made.

cmux has no idle probe yet, so the sandbox sleeps by its terminals and CPU.

## Orca

Orca connects over SSH by itself and installs its own server on the sandbox under `~/.orca-remote`, so no versions must match. InstaADE adds only an [activity probe](#orcas-activity-probe) there. Orca has no link that adds a host, but it imports the hosts in `~/.ssh/config`. So **Install Orca** and **Open in Orca** wake the sandbox, write it as a host to `~/.ssh/instabox/config` and start Orca. Then open **Settings**, then **SSH Hosts** in Orca, connect the host and open `/data/home`. Orca imports the hosts when it starts, and else only when you click **Import** there. So when Orca was already running, click **Import** first.

```
# instabox-sandbox <sandbox id>
Host instabox-dev-1
  HostName ssh.instabox.sh
  Port 2222
  User <background token>
  StrictHostKeyChecking accept-new
  ForwardAgent no
```

- The alias is `instabox-` and the sandbox name. When two sandboxes have the same name, the second one also gets the end of its id. A host keeps its alias, because Orca knows the host by it.
- `~/.ssh/config` gets one line at the top, `Include ~/.ssh/instabox/config`, so these options come before any `Host *`. InstaADE never changes another line there, and keeps a copy of the file in `~/.ssh/config.instabox.bak` before its first change. On Windows these files are in `%USERPROFILE%\.ssh`.
- `ForwardAgent no` is there because the InstaBox SSH gateway refuses agent forwarding, and Orca fails the whole connection when that request is refused.
- The hosts file holds the sandboxes' [background tokens](#ssh-tokens). On macOS only you can read it and its folder. When InstaADE gets a new background token for the sandbox, it rewrites `User`. A new ordinary token never changes the host.
- Tokens last 30 days. At startup, at sign-in and every few minutes, InstaADE renews the background token of each sandbox with a host once it has fewer than 7 days left, or when it has none yet. That is an API call only and never wakes the sandbox. When the host cannot take the new token, the stored token stays behind, so the next pass tries again.
- Deleting the sandbox in InstaADE removes its host. A full refresh removes the host of any sandbox no longer in your sandbox list, and signing out removes them all. InstaADE changes only the blocks it marked. Orca keeps a host it imported until you remove it in Orca.
- On macOS, InstaADE starts only the real Orca app in `/Applications` or `~/Applications`. Its bundle identifier must be `com.stablyai.orca`, and `codesign` must find it signed with the Developer ID of team `6CX3WHS9HZ`. InstaADE checks this when it looks for Orca, again before it wakes, creates or marks a sandbox for Orca, and again after the wake, right before it makes or loads the token.
- On Windows, InstaADE starts `Orca.exe` where the installer puts it (`%LOCALAPPDATA%\Programs\Orca` or `%ProgramFiles%\Orca`). It starts it only when the file's version resource names the product Orca and holds a version, read from the file each time. There is no signature check on Windows yet.

### Orca's activity probe

**Install Orca** and **Reconnect** add an activity probe in `/usr/local/lib/instabox/activity.d/orca`, so the sandbox does not sleep while an Orca agent works. Orca's server on the sandbox keeps running its agents after you quit Orca. An agent in a chat runs with no terminal, so without the probe the sandbox could pause under it. The probe asks Orca's server with Orca's own command, `orca agent list --json`, which the server puts in `~/.orca/cli/bin`. The answer takes about 0.2 seconds.

That server, `orcad`, is what Orca 1.4.225 and later run on an SSH host. Orca 1.4.224 and older run a relay there instead, `~/.orca-remote/relay-<version>/relay.js`, which keeps its terminals after you quit Orca. The relay has no agent list on the sandbox: its own command asks the Orca app on your computer. So with a relay and no `orcad`, the probe says idle and names the relay, and whether the app is connected. The agents of such an Orca run in the relay's terminals, which the sandbox's terminal check counts.

- An agent that is `working`, or in a state the probe does not know, keeps the sandbox awake.
- Agents that are `waiting` or `blocked` on you, or `done`, let it sleep. So does a sandbox where Orca's server does not run. Then the probe does not call the server.
- When the server does not answer within 4 seconds, or is too old for `agent list`, the sandbox sleeps by its terminals and CPU, as without the probe.
- Each call counts as use of Orca's server. So while the sandbox runs, the server does not stop after its 15 idle minutes. It uses no CPU while idle, and it sleeps with the sandbox.
- When the probe cannot be written, such as on a sandbox without `sudo`, Orca still installs, without the probe.
- **Open in Orca** installs or updates the probe in the background, so a sandbox that got Orca before the probe existed gets it at its next Open.

## Paradev (macOS)

Paradev (formerly Superset) runs a sandbox as one of its hosts, and sets the host up by itself over SSH: it installs its CLI and a `superset-host` user service on the sandbox, which then connects out to Paradev's relay. Paradev has no link that adds a host without you, so InstaADE hands the setup to Paradev, and adds only an idle probe once the host runs:

1. **Install Paradev** wakes the sandbox, copies its SSH address, `ssh://<SSH token>@<gateway>:<port>`, and opens Paradev's host settings. A minute later InstaADE clears the address from the clipboard, unless you copied something else since. When InstaADE quits before then, it clears the address on the way out by the same rule.
2. In Paradev, click **Set up a host over SSH** and paste the address.
3. Click **Install Paradev** again. InstaADE checks with one command on the sandbox that `superset-host` is active. Only then does the sandbox get the idle probe and `paradev` in its mark, and a new workspace on it opens in Paradev.

**Open in Paradev** wakes the sandbox, since Paradev does not wake a sleeping one, reads its machine id, and opens `superset://new-workspace?host=<host id>`, with the host id Paradev derives from that machine id. **Reconnect** checks the host again.

The address holds an SSH token, so InstaADE copies it, and opens Paradev's links, only for the real Paradev app in `/Applications` or `~/Applications`, named `Paradev.app` or, before the rename, `Superset.app`. Its bundle identifier must be `com.superset.desktop` (the Canary build is not supported), and `codesign` must find it signed with Paradev's Developer ID (team `2HVBK29S3C`). InstaADE checks this when it looks for Paradev, again before it wakes, creates or marks a sandbox for Paradev, and again right before it makes or loads the token or opens a link. It never opens a `superset://` link in another handler. A Mac can have both `Paradev.app` and `Superset.app`, with the same identifier. InstaADE then uses the newest, and opens the links in it by its path, never in the copy macOS would pick.

### The Paradev probe

Paradev's host reaches Paradev through its relay, which the sandbox's idle check does not count, and Paradev's chat agents run without a terminal. So, by its terminals and CPU alone, the sandbox could sleep under a working agent. When `superset-host` is active, the command that checks the host also installs an activity probe in `/usr/local/lib/instabox/activity.d/paradev`, or brings it up to date. **Install Paradev**, **Reconnect** and **Open in Paradev** all run that command, so a sandbox set up before the probe existed gets it at its next **Open in Paradev**. When the probe cannot be installed, the check still answers as before.

The probe reads the host's address and token from `~/.paradev/host/<org>/manifest.json`, which the host writes at every start (`~/.superset` before the rename), and asks the host for its agents. It sends the token only to an address on the sandbox, follows no redirect, and never prints it.

- An agent keeps the sandbox awake while its last event is `Start` (a turn in progress) or `Attached` (an agent that is starting), or an event the probe does not know.
- An agent that waits for you (`PermissionRequest`), is done (`Stop`) or failed (`Failed`) does not, unless one of its subagents still runs (a subagent can run in the background after its agent's turn ends). Paradev drops a subagent it has not heard from in 10 minutes.
- A terminal agent counts for at most 30 minutes after its last hook, or the last hook of one of its running subagents. Claude Code fires no hook when you interrupt it, so its last event stays `Start`. A working agent fires a hook after every tool call.
- A chat agent counts for at most 6 hours after its turn started, since Paradev records a chat agent's state only when it changes.
- An agent still `Attached` after 5 minutes waits for its first prompt, and does not count. An agent whose event has no time counts only by the hooks of its running subagents.
- Without a host, or when the host does not run, the probe says idle.
- When the probe fails, for example when the host turns its token down or answers in a form the probe does not know, it exits with an error. The sandbox server counts that as idle.

## Paseo

Paseo runs a daemon on every machine it works on, and its desktop app connects to that daemon. Over SSH, the app runs `ssh -W 127.0.0.1:6767 <host>` with your OpenSSH config, and it installs and starts nothing on the host. Paseo has no link that adds a host. So InstaADE runs the daemon on the sandbox and writes its host, and you add the host in Paseo once:

1. **Install Paseo** wakes the sandbox. When the sandbox has no `paseo`, InstaADE installs the Paseo CLI at your app's version, with `npm install -g @getpaseo/cli@<version>`. The standard sandbox has Node.js and npm, and its global npm folder needs no sudo.
2. It runs the daemon as the `paseo-instabox` service. The service runs `paseo daemon run` in a login shell, so the daemon finds Claude Code, Codex and the other agent CLIs on your PATH. The daemon listens on `127.0.0.1:6767` only, starts at boot and restarts when it fails. Once it answers, the sandbox gets `paseo` in its mark.
3. InstaADE writes the sandbox as a host in `~/.ssh/instabox/config`, the same host as for [Orca](#orca), with the same `Include` line, token renewal and cleanup. Then it copies `ssh://instabox-<name>` and starts Paseo. The address holds no token: ssh reads the login from the host.
4. In Paseo, open **Settings**, click **Switch host**, then **Add host** and **Remote SSH**. Paste the address as the **SSH host** and click **Connect**. Paseo keeps the host. The first time Paseo starts, it shows its setup (**Welcome to Paseo**) before its window: finish it or click **Skip setup**.

**Open in Paseo** wakes the sandbox, writes the host, copies the address and starts Paseo. On the sandbox it only brings the probe up to date, in the background. **Reconnect** runs the setup again, which starts a stopped daemon and brings the probe up to date.

InstaADE never updates or restarts a Paseo the sandbox already has: the daemon may be running agents, and Paseo's app and daemon work across versions. To update it, run `npm install -g @getpaseo/cli` on the sandbox, then `sudo systemctl restart paseo-instabox`.

On Windows, InstaADE does not copy the address yet. Type it from the message InstaADE shows.

Paseo reads the host, which holds an SSH token, so InstaADE starts only the real Paseo app:

- On macOS, Paseo in `/Applications` or `~/Applications`. Its bundle identifier must be `sh.paseo.desktop`, and `codesign` must find it signed with Paseo's Developer ID (team `99ZMJMKU9Y`). InstaADE checks this when it looks for Paseo, again before it wakes, creates or marks a sandbox for Paseo, and again after the wake, right before it makes or loads the token.
- On Windows, `Paseo.exe` where the installer puts it (`%LOCALAPPDATA%\Programs\Paseo` or `%ProgramFiles%\Paseo`). InstaADE starts it only when the file's version resource names the product Paseo and holds a version, read from the file each time. Paseo's Windows app is not signed, so there is no signature to check.

### The Paseo probe

Paseo's daemon runs its agents without a terminal. Once you quit Paseo, the sandbox's terminals and CPU can look idle under a working agent, so the sandbox could sleep under it. The setup, which **Install Paseo** and **Reconnect** run, also installs an activity probe in `/usr/local/lib/instabox/activity.d/paseo`, or brings it up to date. **Open in Paseo** installs or updates the probe and the CLI link (below) in the background, so a sandbox set up before the probe existed gets it at its next Open.

The probe asks the daemon on `127.0.0.1:6767` with the Paseo CLI: `paseo ls` for the agents, and `paseo permit ls` for the permissions and questions that wait for you. It runs with a short PATH, so the setup links the CLI as `~/.local/share/paseo-instabox/paseo`.

- An agent keeps the sandbox awake while it runs or starts, or has a status the probe does not know.
- An agent that waits for your permission or answer does not, so a question you forgot does not keep the sandbox awake.
- Without a daemon, or with no agent at work, the probe says idle.

## Herdr

Herdr is a terminal workspace manager for coding agents, with no desktop app. It runs on the sandbox, and InstaADE opens it in your terminal over SSH. Nothing of Herdr runs on your computer, and no app gets an SSH token.

**Install Herdr** wakes the sandbox and runs one command on it:

- When an SSH command cannot find `herdr`, the command runs Herdr's install script, `https://herdr.dev/install.sh`. It puts the latest Herdr in `~/.local/bin/herdr` and checks its SHA-256. An SSH command's PATH has no `~/.local/bin`, so InstaADE links Herdr as `/usr/local/bin/herdr`. A Herdr the sandbox already has stays as it is.
- Then it prints `herdr --version`. When the command fails or prints no version, the sandbox does not get `herdr` in its mark.

Then Herdr opens, as with **Open in Herdr**. **Reconnect** runs the same command again, which installs nothing while Herdr is there, and opens Herdr.

**Open in Herdr** wakes the sandbox, since the SSH gateway refuses a paused one. Then it opens your terminal with this command:

```
<OpenSSH client> -t -o StrictHostKeyChecking=accept-new -p 2222 <SSH token>@ssh.instabox.sh herdr
```

- The OpenSSH client is the one InstaADE found, by its full path, such as `/usr/bin/ssh` or `C:\Windows\System32\OpenSSH\ssh.exe`. InstaADE looks for it again right before it makes or loads the token, and the one it finds then is the one that runs.
- InstaADE builds the command from the host, port and token the API answers, not from the SSH command it shows. When the host or the token holds anything but letters, digits, `.`, `_` and `-`, or starts with `-`, nothing opens. **Open in Terminal** builds and checks its login the same way, and runs `ssh`.
- On macOS the terminal is Terminal, as for **Open in Terminal**: the command goes into a temporary `.command` file that only you can read, and InstaADE removes it 10 seconds later. On Windows it is Windows Terminal, else a console window.
- The token is the [ordinary token](#ssh-tokens), the one the SSH command in the sandbox's details uses: the stored one while it has more than a day left, else a new 30-day one. It goes on the command line, never through the SSH host of Orca and Paseo.
- The first time, ssh adds the gateway's host key to `~/.ssh/known_hosts` without asking.
- Herdr keeps its panes in a server on the sandbox. Closing the terminal, or detaching with `ctrl+b q`, leaves the agents running, and the next Open attaches to them again. `herdr server stop` on the sandbox ends them.

InstaADE never updates Herdr on the sandbox. Run `herdr update` there to update it.

On your computer, InstaADE needs only the OpenSSH client: `/usr/bin/ssh` on macOS, and on Windows `ssh.exe` on the PATH, else in `System32\OpenSSH`. Settings shows its version next to Herdr. Without it, nothing wakes or changes, and InstaADE asks you to install the OpenSSH client.

Herdr has no idle probe yet, so the sandbox sleeps by its terminals and CPU.

## Several ADEs on one sandbox

A sandbox can have several ADEs, and its details list each one with **Open**. The sandbox's metadata records which ADEs it has, in a form older releases (named Instabox) still read. [ADEs and the sandbox mark](../DEVELOPMENT.md#ades-and-the-sandbox-mark) has the rules.
