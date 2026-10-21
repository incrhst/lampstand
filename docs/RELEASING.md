# Releasing Lampstand

A release is a version tag. Pushing `v0.1.0` makes CI build the installers for
Mac, Linux and Windows and attach them to a **draft** pre-release on GitHub.
Nothing is public until someone reads the notes and presses **Publish**.

| Platform | Installers | Signed? |
|---|---|---|
| macOS (Apple silicon and Intel in one) | `.dmg` | Signed with a Developer ID and notarised by Apple, so it opens without a warning |
| Linux | `.AppImage`, `.deb` | Not signed |
| Windows | NSIS installer (`.exe`) | **Not signed yet**: Windows shows a SmartScreen warning (T95 adds signing) |

The auto-updater (the rest of T59) is not in yet. Until it is, people install
each release by hand.

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
5. Publish. Then update the landing page's download link.

To withdraw a release, delete the release and its tag; nothing else depends on
them.

## Notes

- The Mac build is a universal binary, so it takes the longest.
- Entitlements are in `app/src-tauri/entitlements.plist`. It is empty: Lampstand
  is not sandboxed. Add an entitlement only if a notarised build fails without
  it, and say why.
- The bundle settings are under `bundle` in `app/src-tauri/tauri.conf.json`.
