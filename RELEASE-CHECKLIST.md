# Publishing v0.1.0

Everything below is ready; these are the steps left, which are yours.

## 1. Push

```
git remote add origin https://github.com/<you>/Rebuffer.git
git push -u origin master
```

## 2. Tag

```
git tag -a v0.1.0 -m "Rebuffer v0.1.0"
git push origin v0.1.0
```

## 3. Create the release

Paste `RELEASE-NOTES-v0.1.0.md` as the body and attach:

```
src-tauri/target/release/bundle/nsis/Rebuffer_0.1.0_x64-setup.exe
```

The bare `rebuffer.exe` is not worth attaching: it needs the WebView2 runtime
and writes to `%APPDATA%` either way, so the installer is the only sensible
artifact for a user.

## Worth knowing before you press publish

**The build is unsigned.** SmartScreen will warn every first-time user, and the
warning looks alarming. It is stated in the README and in the release notes,
but people read the release page, not the README, so keep it visible there.

**CI has never run.** The workflow in `.github/workflows/build.yml` was written
during development and its gates (`cargo fmt --check`, `clippy -D warnings`,
tests, `tsc`, `svelte-check`) all pass locally as of this commit. The full
`tauri build` step on a GitHub runner has never executed. Expect the first run
to need a fix or two, and do not let a red badge on day one surprise you.

**`docs/KNOWN-ISSUES.md` is linked from the README on purpose.** It names two
real defects and everything that was verified by reading rather than by
running. It is better for that to be found by a reader than by a bug report.

## Current state

- 113 files tracked; the 18 MB of development scaffolding is gone
- `cargo fmt`, `clippy -D warnings`, 104 tests, `tsc`, `svelte-check`: all clean
- Artifacts built from this commit: installer 3.29 MB, exe 8.17 MB
