<div align="center">
  <a href="https://instabox.sh">
    <img src="src-tauri/icons/128x128@2x.png" alt="InstaADE" width="96">
  </a>

  <h1>InstaADE</h1>

  <p>Run your ADE (agentic development environment) on an instabox sandbox, from your menu bar or system tray.<br>Works with T3 Code, cmux, Orca, Paradev, Paseo and Herdr.</p>

  <p>
    <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-yellow.svg" alt="License: MIT"></a>
    <a href="https://github.com/InsForge/instaADE/releases/latest"><img src="https://img.shields.io/github/v/release/InsForge/instaADE" alt="Latest release"></a>
    <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey.svg" alt="Platform: macOS | Windows">
  </p>
</div>

<p align="center">
  <img src="docs/images/list.png" alt="Sandbox list in cmux mode" width="200">
  <img src="docs/images/switcher.png" alt="ADE switcher with T3 Code, cmux, Orca, Paradev, Paseo and Herdr" width="200">
  <img src="docs/images/menu.png" alt="Sandbox actions in Paradev mode" width="200">
  <img src="docs/images/details.png" alt="Sandbox details with T3 Code and cmux connected, and Orca, Paradev, Paseo and Herdr to add" width="200">
</p>

## Getting Started

You need macOS or Windows (x64), an [instabox](https://instabox.sh) account, and the desktop app of your ADE (see [Supported ADEs](#supported-ades)).

1. Download the latest release from [Releases](https://github.com/InsForge/instaADE/releases/latest):
   - macOS: the `.dmg`, `aarch64` for Apple silicon, `x64` for Intel.
   - Windows: the `x64-setup.exe` installer.
2. Install it:
   - macOS: drag InstaADE to Applications and open it.
   - Windows: run the installer. It is not signed yet, so SmartScreen asks once: click **More info**, then **Run anyway**.
   - Had Instabox, its old name? InstaADE replaces it and keeps your sign-in and data.
3. Click the InstaADE icon in the menu bar or the system tray (on Windows, maybe under the **^** arrow). Can't find it? See [Troubleshooting](#troubleshooting).
4. Sign in with GitHub or Google.
5. Pick your ADE in the menu at the top. **InstaBox** shows sandboxes only.
6. Click **New sandbox**, pick a size, then click **Create sandbox**.
7. Confirm the sandbox in your ADE. Orca, Paradev and Paseo need one more step the first time, below.

## Features

- Create a sandbox or connect one you already have
- **Install** or **Open in** your ADE on each sandbox, and use several ADEs on one sandbox
- Sandboxes sleep when idle, also with cmux, Orca or Paseo open, and stay awake while an agent works
- Pause, resume, resize and set auto sleep
- Copy the SSH command or open it in a terminal
- Sign in to Claude Code or Codex on the sandbox
- Starts at login, and updates itself from **Update** in Settings

## Supported ADEs

Every **Open in** wakes the sandbox first.

- **[T3 Code](docs/ades.md#t3-code)** · macOS, Windows · [download](https://github.com/pingdotgg/t3code/releases/latest)
  - **Install T3 Code** installs T3 on the sandbox and pairs it with the app.
  - **Open in T3 Code** pairs again, so T3 Code finds the sandbox even if it lost it.
- **[cmux](docs/ades.md#cmux-macos)** · macOS · [download](https://github.com/manaflow-ai/cmux/releases/latest)
  - Opens the sandbox in cmux over SSH. Nothing is installed on the sandbox.
- **[Orca](docs/ades.md#orca)** · macOS, Windows · [download](https://github.com/stablyai/orca/releases/latest)
  - Adds the sandbox as an SSH host and starts Orca.
  - First time: in Orca, open **Settings** → **SSH Hosts**, connect the host InstaADE names, and open `/data/home`. Not listed? Click **Import**.
- **[Paradev](docs/ades.md#paradev-macos)** · macOS, not the Canary build · [download](https://github.com/paradevai/paradev/releases/latest)
  - **Install Paradev** copies the sandbox's SSH address and opens Paradev's host settings.
  - First time: click **Set up a host over SSH**, paste the address, then click **Install Paradev** in InstaADE again.
- **[Paseo](docs/ades.md#paseo)** · macOS, Windows · [download](https://github.com/getpaseo/paseo/releases/latest)
  - Installs the Paseo daemon on the sandbox, adds it as an SSH host, copies its address and starts Paseo. On Windows, InstaADE shows the address.
  - First time: finish or skip Paseo's setup, then open **Settings** → **Switch host** → **Add host** → **Remote SSH** and paste the address.
- **[Herdr](docs/ades.md#herdr)** · no app, runs in your terminal
  - Installs Herdr on the sandbox and opens it in your terminal over SSH.
  - Closing the terminal leaves Herdr and its agents running. The next **Open in Herdr** attaches again.

**Sleep**

- A sandbox pauses after its auto sleep time with no activity.
- While an agent works in T3 Code, Orca, Paradev or Paseo, a small probe on the sandbox keeps it awake.
- cmux, Orca and Paseo connect with a background SSH token, so leaving them open does not keep the sandbox awake.

**Your SSH config** (Orca and Paseo only)

- InstaADE adds `Include ~/.ssh/instabox/config` at the top of `~/.ssh/config`, after a backup to `~/.ssh/config.instabox.bak`.
- `~/.ssh/instabox/config` holds the sandboxes' hosts and their SSH tokens, which InstaADE renews. On macOS only you can read it.

Is your favorite ADE missing? Add it, or ask for it in an issue. See [CONTRIBUTING.md](CONTRIBUTING.md#add-an-ade). For how InstaADE works with each ADE, see [docs/ades.md](docs/ades.md).

## Troubleshooting

- **No InstaADE icon in the macOS menu bar.** When the menu bar is crowded, the icon can hide behind the notch. Hold Command and drag the icon to the right to bring it back. You can also open InstaADE again from Applications, which shows the popover.
- **The popover is in the way.** Drag it by its top bar.

## Roadmap

- More ADEs. Pull requests that add one are welcome.

## Development

For the code layout, adding an ADE, the checks to run before a pull request, preview builds and releases, see [DEVELOPMENT.md](DEVELOPMENT.md). Coding agents load the same guide through the `instabox-development` skill.

### Prerequisites

- macOS or Windows
- [Bun](https://bun.sh)
- Rust via [rustup](https://rustup.rs)
- macOS: Xcode Command Line Tools (`xcode-select --install`)
- Windows: Microsoft C++ Build Tools

### Setup

```bash
bun install
```

### Run with hot reload

1. Quit any installed copy of InstaADE (or Instabox, its old name). Only one instance runs at a time.
2. Run `bun tauri dev`.
3. Click the InstaADE icon in the menu bar or system tray.
4. Edit the code. UI changes reload at once, and Rust changes rebuild the app.

**Tips:**

- For browser-only UI development, run `bun run dev` and open `http://localhost:1420`. It shows labeled sample data; refresh to reset. Use the menu bar app for real sandbox and desktop actions.
- The default layout removes the hero and uses its colors as the app background, fading to black over the top 200px. Open `http://localhost:1420/?layout=hero` to compare the original hero layout.
- The browser preview never calls the app or the instabox API. Actions that need them, such as Open or Install, show an error there.
- Set `INSTABOX_API_URL` to point the app at another API.
- Set `INSTABOX_API_KEY` to sign in with an API key instead of the browser.
- You do not need an installer to see a change.
- A dev build never turns on Start at login by itself.

### Tests

```bash
bun run test
cd src-tauri && cargo test
```

### Build an installer

Only needed to test installing.

```bash
# macOS: src-tauri/target/release/bundle/dmg/
bun tauri build --bundles dmg

# Windows: src-tauri/target/release/bundle/nsis/
bun tauri build --bundles nsis
```

A local build is not notarized, so macOS blocks it on other Macs.

### Share a preview build

To let others try a pull request, build InstaADE Preview in CI. It installs next to InstaADE and keeps its own sign-in. The macOS `.dmg` is signed and notarized like a release. The Windows installer is not signed yet, so SmartScreen asks once.

1. Add the `preview-build` label to the pull request. The pull request's author must be listed in `.github/PREVIEW_AUTHORS`.
2. **Preview build** builds the app from the pull request without any secrets. **Preview publish** then runs main's code only: it checks the author and the app, signs and notarizes it, and takes the label off.
3. The files go to the `instabox-preview` prerelease, and the pull request gets a comment with direct links. They download without a GitHub account.
4. To build a newer commit, add the label again. Closing the pull request deletes its files.

## Contributing

Contributions are welcome, from new ADEs to new features and fixes. See [CONTRIBUTING.md](CONTRIBUTING.md).

- **Add your favorite ADE**: start with [Add an ADE](CONTRIBUTING.md#add-an-ade).
- **Add a feature or fix a bug**: open a pull request. For a large change, open an issue first.
- **Issues**: open an [issue](https://github.com/InsForge/instaADE/issues) for a bug, an idea or an ADE you want.

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.
