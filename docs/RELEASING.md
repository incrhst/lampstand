# Releasing Lampstand

A release is a version tag. Pushing `v0.1.0` makes CI build the installers for
Mac, Linux and Windows and attach them to a **draft** release on GitHub, along
with the files installed copies use to update themselves. Nothing is public
until someone reads the notes and presses **Publish**.

| Platform | Installers | Signed? |
|---|---|---|
| macOS (Apple silicon and Intel in one) | `.dmg` | The app and the disk image are both signed with a Developer ID and notarised by Apple, with the tickets stapled, so it opens without a warning, even offline |
| Linux | `.AppImage`, `.deb` | Not signed |
| Windows | NSIS installer (`.exe`) | **Not signed yet**: Windows shows a SmartScreen warning (T95 adds signing) |

## Updating installed copies

Installed copies update themselves. A copy looks for the **latest published
release that is not marked a pre-release** at startup and every few hours, shows
"Update 0.2.0 available" in the status bar, and installs it from Setup, About
when the operator chooses. It refuses to install while anything is on air or a
service starts within the hour. The Mac and Windows installs and the Linux
AppImage update themselves; the Linux `.deb` does not (installing a package needs
administrator rights), so `.deb` users download the new one.

Updates are checked against a signing key, so only a release signed by us can be
installed:

- The **public** key is in `app/src-tauri/tauri.conf.json` (`plugins.updater`),
  and every copy ever installed trusts it.
- The **private** key is made once with `npx tauri signer generate -w
  ~/.tauri/lampstand-updater.key` (from `app/`). It stays on the machine of whoever
  releases, outside the repository, and is added to GitHub as the secret
  `TAURI_SIGNING_PRIVATE_KEY`: `gh secret set TAURI_SIGNING_PRIVATE_KEY <
  ~/.tauri/lampstand-updater.key`. If the key has a password, add
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` too (the first key has none).
- **Back the private key up somewhere safe.** If it is lost, copies already
  installed cannot be updated to a release signed with a new key: people would
  have to download and install once more by hand.
- A build with updates on **fails without that secret**, including a rehearsal.

The update address is the `latest.json` that the release workflow attaches to each
release. Marking a release a pre-release keeps it out of what installed copies
are offered, which is how to publish a build for testers only.

## One-time setup: the Mac signing secrets

Without these the macOS build still succeeds, but unsigned. To sign and
notarise it, the account holder of the Apple Developer Program does this once:

1. **Create a Developer ID Application certificate.** In Xcode: Settings,
   Accounts, pick the team, Manage Certificates, **+**, Developer ID Application.
   (Or on developer.apple.com under Certificates. Only the account holder can
   create this type. An "Apple Development" certificate will not do.)
2. **Export it.** In Keychain Access, find "Developer ID Application: Name
   (TEAMID)", export it as a `.p12`, and set a password.
3. **Make an app-specific password** at appleid.apple.com (Sign-In and
   Security, App-Specific Passwords). Notarisation uses it instead of your
   Apple ID password.
4. **Add these as repository secrets** (Settings, Secrets and variables,
   Actions), or with `gh secret set NAME` in a terminal. Never paste them into
   a chat, an issue or a commit.

| Secret | What it is |
|---|---|
| `APPLE_CERTIFICATE` | The `.p12`, base64-encoded: `base64 -i cert.p12 \| pbcopy` |
| `APPLE_CERTIFICATE_PASSWORD` | The password you set when exporting it |
| `APPLE_SIGNING_IDENTITY` | The certificate's full name, `Developer ID Application: Name (TEAMID)` |
| `APPLE_ID` | The Apple ID email |
| `APPLE_PASSWORD` | The app-specific password from step 3 |
| `APPLE_TEAM_ID` | The 10-character team ID (the part in brackets in the certificate name) |

The update-signing secret (`TAURI_SIGNING_PRIVATE_KEY`) is in the section above.

## Rehearse first

Do a rehearsal before the first real release, and after any change to the
bundle settings.

1. **Build without a release.** Actions, Release, Run workflow. It builds all
   three platforms and keeps the installers as downloadable artifacts. Nothing
   is published.
2. **Tag a rehearsal.** `git tag v0.1.0-preview.1 && git push origin v0.1.0-preview.1`
   makes a draft pre-release. Delete the draft and the tag afterwards.
3. **Check the Mac build as a user would.** Download the `.dmg` through a
   browser (so it carries the quarantine mark), then:

   ```bash
   spctl --assess --type open --context context:primary-signature -v Lampstand_*.dmg
   hdiutil attach Lampstand_*.dmg      # then, on the app inside:
   spctl --assess --type execute -v /Volumes/Lampstand/Lampstand.app
   xcrun stapler validate /Volumes/Lampstand/Lampstand.app
   ```

   Each should say accepted, and `stapler` that the ticket is valid. Then drag
   it to Applications and open it on a Mac that has never seen it.

## A real release

1. Merge everything for the release. CI on `main` is green.
2. **Raise the version in three places, together**: `version` in `Cargo.toml`
   (the workspace), `app/src-tauri/tauri.conf.json` and `app/package.json`. The
   workflow stops if the tag disagrees with any of them.
3. Tag and push: `git tag v0.1.0 && git push origin v0.1.0`.
4. Wait for the three builds. Open the draft release, check it has the
   `.dmg`, `.AppImage`, `.deb` and the Windows `.exe`, and write the notes
   (what is new, known problems).
5. Publish. Installed copies are offered it from then on. Update the landing
   page's download link.

To check updating end to end, publish a release, install it, then publish a higher
one (`v0.1.1`) and open Setup, About in the installed copy: it should offer the new
version and, with nothing on air, install it and restart.

To withdraw a release, delete the release and its tag; nothing else depends on
them.

## Notes

- The Mac build is a universal binary, so it takes the longest.
- Entitlements are in `app/src-tauri/entitlements.plist`. It is empty: Lampstand
  is not sandboxed. Add an entitlement only if a notarised build fails without
  it, and say why.
- The bundle settings are under `bundle` in `app/src-tauri/tauri.conf.json`.
