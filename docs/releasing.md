# Releasing

Release installers are built by CI (`.github/workflows/release.yml`), never on a
dev machine.

1. Bump `version` in `src-tauri/tauri.conf.json` (and `package.json` to match).
2. Commit, then tag and push: `git tag v0.2.0 && git push origin v0.2.0`.
   The workflow fails fast if the tag doesn't match the version in `tauri.conf.json`.
3. When the Linux, macOS and Windows jobs finish, review the draft release on
   GitHub and publish it.

To try a build without releasing it, run the `release` workflow by hand from the
Actions tab on any branch. Each platform's installers are kept as an artifact of
the run instead.

The macOS and Windows builds aren't signed, so the first launch is stopped by
Gatekeeper (right-click the app and choose Open, or
`xattr -dr com.apple.quarantine /Applications/Orchestrate.app`) and SmartScreen
(More info → Run anyway).

## iPhone

`.github/workflows/ios.yml` builds the iPhone app on GitHub's macOS runner,
signs it, and uploads it to TestFlight, with every release tag. You can also run
it by hand from the Actions tab. No Mac is needed at any step. The app is only a
window: it shows the Hosts on your other machines, reached over Tailscale
(ADR 0016).

### One-time setup

Everything happens in a browser and in GitHub's settings. It needs a paid Apple
Developer Program membership, the same one that signs the macOS app.

1. **Register the app's ID.** At developer.apple.com → Certificates, Identifiers
   & Profiles → Identifiers, add an App ID with the bundle ID
   `io.github.devontheriault.orchestrate` (the `identifier` in
   `src-tauri/tauri.conf.json`).
2. **Create the app record.** At appstoreconnect.apple.com → Apps → +, choose
   New App, platform iOS, and pick that bundle ID. The name and SKU can be
   anything.
3. **Make an API key.** At App Store Connect → Users and Access → Integrations →
   App Store Connect API, generate a Team key with the **App Manager** role.
   Download the `.p8` file. Apple only offers the download once.
4. **Add the repository secrets** (Settings → Secrets and variables → Actions):

   | Secret | Value |
   |---|---|
   | `APPLE_API_KEY_P8` | the whole contents of the `.p8` file |
   | `APPLE_API_KEY_ID` | the key's ID, shown beside it in App Store Connect |
   | `APPLE_API_ISSUER` | the Issuer ID, shown above the list of keys |
   | `APPLE_TEAM_ID` | the 10-character Team ID (shared with macOS signing) |

The workflow stops at its first step and names any secret that's missing.

### Getting it on a phone

1. Run the `ios` workflow, or push a release tag.
2. Wait for the upload to finish processing in App Store Connect → TestFlight.
   That takes a few minutes to an hour. You'll get an email.
3. Under TestFlight → Internal Testing, add yourself as a tester once. After
   that, every new build shows up in the TestFlight app on your phone.
4. On the phone, install Tailscale and sign in as the same Tailscale user as
   your other machines. Then open Orchestrate, tap +, and add a machine by its
   Tailscale name. That machine's Host has to be running and listening on the
   tailnet.

Each run uploads a build number taken from the workflow's run number, so
uploads never clash. The `.ipa` is also kept with the run, as an artifact.

The Xcode project isn't committed. The workflow generates it (`tauri ios init`)
on every run. Anything the app needs in its Info.plist goes in
`src-tauri/Info.ios.plist`, which Tauri merges in. The icon comes from
`src-tauri/icons/ios.svg`.

## Why Linux doesn't ship from a laptop

An AppImage bundles its libraries but not glibc, and the `.deb`/`.rpm` link
against the system glibc too. Either one only runs on systems with a glibc at
least as new as the build machine's. A build from a rolling distro like Arch
needs a newer glibc than Ubuntu LTS or Debian stable ship, so it won't start
there. CI builds on `ubuntu-22.04`, the oldest runner available.

## What CI changes in the AppImage

CI unpacks the AppImage that Tauri builds, changes two things, and repacks it:

- It deletes the bundled `libwayland`. That copy comes from the Ubuntu runner and
  is too old for the user's Mesa, so the window stays blank on current distros.
  Every system that can run the app already has its own copy.
- It stops linuxdeploy's GTK hook from forcing `GDK_BACKEND=x11`. The hook does
  that to avoid crashes caused by the bundled `libwayland`, which is gone now.
  Under X11 the desktop's `GDK_SCALE` applies. Omarchy, for example, sets it to
  2, which makes the whole window twice as big. The hook now prefers Wayland,
  falls back to X11, and keeps any `GDK_BACKEND` the user set themselves.

Local builds (below) skip both changes. Their `libwayland` comes from your own
machine, so they don't open blank there. They still force X11, so they can open
oversized.

## Local builds

`npm run build:linux` builds all Linux bundles on your own machine for testing.
It sets two variables that stop the AppImage step from failing on newer distros:

- `NO_STRIP=true`: linuxdeploy comes with an old `strip` that can't read the
  `.relr.dyn` sections in newer system libraries, and fails with only
  `failed to run linuxdeploy`.
- `APPIMAGE_EXTRACT_AND_RUN=1`: runs linuxdeploy without FUSE, so the build
  doesn't need `libfuse.so.2` installed.

Only run these builds on the machine that made them. They aren't release
artifacts.
