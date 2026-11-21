# Developing InstaBox

Read this before you change the code, open a pull request, build a preview or cut a release. The README covers setup and running the app.

## Layout

- `src-tauri/` is the Rust core on Tauri 2.
  - `app.rs`: the app state and every action. `commands.rs`: the commands the UI calls.
  - `api/`: the instabox API client. `auth.rs`: device sign-in and token storage.
  - `sandbox/`: create, connect, sync and lifecycle of sandboxes.
  - `ade/`: the ADE registry (`mod.rs`) and one module per ADE (`t3.rs`, `cmux.rs`).
  - `agents.rs`: signing Claude Code or Codex in on a sandbox. `tray.rs`: the menu bar popover. `login.rs`: start at login.
- `src/` is the React UI. `api.ts` wraps the Rust commands and `types.ts` mirrors the Rust state. `mode.ts` keeps the ADE the user picked.

The UI never calls the instabox API itself. Every action on a sandbox, the account or this computer is a Rust command, and is tested in Rust. The UI owns what it shows, such as the picked ADE, and is tested with Vitest.

## Run it

1. Quit any installed InstaBox. Only one instance runs at a time.
2. `bun install`, then `bun tauri dev`.
3. UI changes reload at once. Rust changes rebuild and restart the app.

- On a Mac with a notch, a full menu bar hides the icon behind the notch. Make room in the menu bar, or use the popover that opens on launch.
- `INSTABOX_API_URL` points the app at another API. `INSTABOX_API_KEY` signs in with an API key instead of the browser.

## Before a pull request

Run these locally. They cover the `check` job in CI, with clippy also over the tests. CI builds the Windows installer as well.

```bash
bun install --frozen-lockfile
bun run typecheck
bun run test
bun run build
cd src-tauri
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

- Add a test with every fix, in Rust for an action and in Vitest for the UI.
- Keep `src/types.ts` in step with the Rust state. Rust serializes it in camelCase.
- `main` takes changes only through pull requests. One approval and the `check` job are required.

## ADEs and the sandbox mark

The README section **Adding an ADE** lists the files to touch. The mark that says which ADEs a sandbox has must stay compatible with older releases:

- `instabox-ades` lists every ADE id on the sandbox, comma separated, such as `t3,cmux`.
- `instabox-bridge` holds one id, the key v0.1.x reads and writes. It is `t3` whenever T3 is on the sandbox.
- A sandbox with no mark has no ADEs. An action that must pick an ADE and was given none uses T3.
- An id this build does not know is kept when writing and never treated as T3.

## Preview builds

A pull request can get a signed Instabox Preview, which installs next to InstaBox with its own sign-in (identifier `sh.instabox.bridge.preview`).

1. Add the `preview-build` label. The pull request's author must be in `.github/PREVIEW_AUTHORS`.
2. **Preview build** (`.github/workflows/preview.yml`) builds the app from the pull request without secrets.
3. **Preview publish** (`.github/workflows/preview-publish.yml`) runs main's code only. It checks the author and the build, signs and notarizes the macOS app, takes the label off, and puts the files on the `instabox-preview` prerelease. The pull request gets a comment with the links.
4. To build a newer commit, add the label again. Closing the pull request deletes its files.

Preview publish runs from `main`, so a change to it takes effect only after it merges. The Windows installer is not signed yet.

## Releases

Repository admins cut releases.

1. Bump the version in `package.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` and `src-tauri/tauri.conf.json`, and merge that through a pull request.
2. Push a `vX.Y.Z` tag on `main`. Only admins may push `v*` tags.
3. **Release** (`.github/workflows/release.yml`) builds the signed and notarized `.dmg` files for Apple silicon and Intel and the Windows installer, checks the signature, and uploads them to the GitHub release.

The signing secrets live in two environments: `release`, which only `v*` tags may use, and `preview`, which only `main` may use. Never add them at the repository level.
