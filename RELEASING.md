# Release Process

Only a pushed version tag starts `.github/workflows/release.yml`. Ordinary branch
pushes, pull requests and manual dispatch do not trigger this workflow. Valid tags
are stable `vMAJOR.MINOR.PATCH` versions; invalid tags fail before building.

## Prepare

1. Update the versions in `Cargo.toml`, root/frontend `package.json` and
   `src-tauri/tauri.conf.json` together. Update the displayed UI version when changing
   versions and add `releases/vMAJOR.MINOR.PATCH.md`.
2. Run Rust checks, `pnpm test:frontend`, `pnpm test:release`, and Windows native
   regression against the final Windows build. Commit the release source.
3. Check the remote tag and Release do not already exist. Existing publications are
   never overwritten by the workflow. Resolving a duplicate requires explicit approval.
4. Push the intended source to `main`, then create and push the annotated version tag:

```powershell
git tag -a v0.1.0 -m "ssh-tunnel v0.1.0"
git push origin v0.1.0
```

Never move a published tag to fix a failed build. Prefer a new version. If an upload
fails after draft creation, the draft stays unpublished; inspect it before explicitly
deleting it and rerunning. The workflow does not silently clobber draft assets.

## Pipeline

- Validate the tag against every package version and require version-specific notes.
- Run Rust/React tests and lint on Windows, macOS and Ubuntu 22.04.
- Windows x64: portable EXE and current-user NSIS installer; run both native WebView2
  configuration-isolation regression modes against the bundled executable.
- macOS: Universal DMG with x86_64 + arm64; verify both architectures and code signature.
- Linux x64: AppImage and DEB, built on Ubuntu 22.04.
- Collect exactly the expected files; record version, source commit, size and SHA-256
  in platform manifests. The publish job rejects missing platforms, inconsistent
  commits/versions, altered bytes, unexpected files and unsafe names.
- Only after all builds pass, assemble `SHA256SUMS`, upload all files to a draft, and
  publish the complete Release. Only this last job receives `contents: write`.

## Local Packaging

Install frontend dependencies with `pnpm --dir frontend install --frozen-lockfile`.
Use the command for the native host:

```text
Windows: pnpm build:windows
macOS:   rustup target add aarch64-apple-darwin x86_64-apple-darwin
         pnpm build:macos
Linux:   pnpm build:linux
```

Windows output: `target/release/bundle/nsis/` and `target/release/ssh-tunnel.exe`.
macOS output: `target/universal-apple-darwin/release/bundle/dmg/`.
Linux output: `target/release/bundle/appimage/` and `target/release/bundle/deb/`.
Linux needs the system packages listed in the workflow; WSL can build Linux packages,
but cannot replace macOS/Xcode for macOS packaging. GitHub's macOS runner provides that host.

## Signing and Verification Limits

Windows is currently unsigned. macOS uses an ad-hoc signature (`-`), with no Developer
ID certificate or Apple notarization. Release notes must disclose OS security warnings.
Never claim these packages are notarized or bypass global operating-system protections.
Adding trusted signing later requires securely configured certificates/secrets.

Windows WebView regression is not macOS/Linux desktop acceptance. Successful builds,
unit tests and package/signature checks must be reported separately from actual UI tests.
