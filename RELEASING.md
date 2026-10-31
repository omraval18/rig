# Releasing rig

Releases are built, signed and published by `.github/workflows/release.yml` when a `v*` tag is pushed. Nothing is uploaded until the checksum list has been checked against the public key in this repo.

## One-time setup (maintainers)

1. **Create the release key.** Keep the secret key out of the repo.

   ```bash
   minisign -G -W -p minisign.pub -s ~/rig-release.key -t "rig release key"
   ```

   `-W` makes the key unencrypted. CI cannot type a passphrase, so protecting the key falls to the `release` environment below (required reviewers, and only the release job can read the secret). Anyone who gets the secret key can publish updates that clients will accept, so back it up offline.

2. **Commit `minisign.pub`** (the public key file). It is public and is what CI and the rig binary check signatures against.

3. **Create the `release` environment** in GitHub (Settings → Environments) with required reviewers, then store the secret:

   ```bash
   gh secret set RIG_MINISIGN_KEY --env release < ~/rig-release.key
   ```

   Delete the local copy once it is backed up.

4. **Embed the public key** in the binary: set `RELEASE_PUBLIC_KEY` in `crates/rig-cli/src/update/mod.rs` to the base64 line from `minisign.pub`. Until this is set, released binaries never download updates (the safe default).

## Cutting a release

1. Bump `version` in the workspace `Cargo.toml` and move the `Unreleased` entries in `CHANGELOG.md` under the new version and today's date. Merge through a pull request.
2. Tag the merge commit and push the tag:

   ```bash
   git tag -a v0.2.0 -m "rig 0.2.0"
   git push origin v0.2.0
   ```

3. The workflow checks that the tag matches the crate version, builds, writes `SHA256SUMS`, signs it, checks the signature against `minisign.pub`, and only then uploads the archives and signatures to the GitHub release.
4. Check the result: `rig update --check` from an older install should report the new version, and a background update should stage it.

## Rotating the key

Generate a new key, replace `minisign.pub`, update the `RIG_MINISIGN_KEY` secret and `RELEASE_PUBLIC_KEY`, and publish a release signed by the new key. Installs still on the old key cannot verify it, so they must reinstall once (`install.sh`). Plan a rotation, don't do it on a hunch.

## Known gaps

- macOS builds are not yet signed with a Developer ID certificate or notarized, so Gatekeeper may warn on first run after a download.
- Only `aarch64-apple-darwin` is published. Other targets listed in `update/release.rs` are used once their archives are added to the workflow matrix.
