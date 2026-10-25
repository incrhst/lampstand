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
   (what is new, known problems). Put what an operator should expect from the
   update under a line that starts `**What's new in 0.1.0**`, as a short list in
   plain words. Publishing copies that section into `latest.json` (the Release
   notes workflow), and installed copies show it under Setup, About before the
   operator installs. A release without that heading shows no notes.
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

### Releasing 0.8.0: roles and sound

- **Every device must update together.** 0.8.0 is sync protocol 5 and 0.7.0 is protocol 4. A device on the other version is refused at the handshake with a message saying which side should update, so a church with several paired computers updates all of them before the next service. The release notes say so first. Installed copies offer the update from Setup, About, and refuse to install while a service is on air or starts within the hour.
- **Roles move out of the old files.** A device reads `presenter.json` and `driver.json` once into `roles.json` and leaves the old files alone, so going back to 0.7.0 starts from them as they were.
- **The Roles page.** `site/docs/roles.html` says roles are new in 0.8.0. The site deploys when this merges to `main`, so merge the release on the day the release is published.
- **What the notes should say is not there yet:** sending Lampstand's sound to the stream through OBS; a music-only item starting by itself from Next (it plays from its Play button); and the warnings that nobody runs the sound or the stream. The same list is at the end of `docs/roles.md`.

### Sound in the order of the service, and going back

A service can hold music-only items from 0.8.0. A build older than the fix that shipped with this (any 0.8.0 build before it, on sync protocol 5) cannot list **any** service while one of them holds a music-only item, and the Setup switch "Use sound in services" does not protect it, because the item still syncs from the device that added it. Update such a device before anyone adds sound. Released 0.7.0 is on protocol 4 and never receives these documents.

From the fix on, a version that cannot understand a service leaves it out of the list and says "N services were saved by a newer Lampstand and can't be opened here. Update Lampstand to see them." Installing an update is never refused for that reason: About asks "Install anyway?" because the next service's start time is unknown. Before releasing a version that reads a new kind of item, field or document, check what the previous release does with it.

To roll back past this change, remove the music-only items from the services first; backing tracks on songs are ignored harmlessly.
