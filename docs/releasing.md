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

## Why Linux doesn't ship from a laptop

An AppImage bundles its libraries but not glibc, and the `.deb`/`.rpm` link
against the system glibc too. Either one only runs on systems with a glibc at
least as new as the build machine's. A build from a rolling distro like Arch
needs a newer glibc than Ubuntu LTS or Debian stable ship, so it won't start
there. CI builds on `ubuntu-22.04`, the oldest runner available.

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
