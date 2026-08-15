# macOS code signing and notarisation — readiness

**Status: not implemented, and deliberately so.** dodo ships unsigned today
(`docs/release.md`, "Future readiness"). Nothing here is enabled by any workflow
or script; this file exists so that the day the repo owner decides to turn it
on, the work is *procurement followed by an afternoon of plumbing* rather than a
week of discovery.

**This is a separate file rather than a section of `docs/release.md` on
purpose.** `docs/release.md` is a runbook for someone cutting a release —
everything in it is a thing you do today. This is a shopping list and a design,
for a decision nobody has taken. Folding it in would add four hundred lines to an
already long runbook and bury the one part that matters, which is
[the checklist](#1-what-the-repo-owner-must-personally-obtain). `docs/release.md`
links here from "Required GitHub Secrets" and "Future readiness"; those remain
the authority on how a release works, and this file is the authority on signing.

## How to read the confidence labels

Signing is an area where plausible-sounding wrong answers are cheap and cost a
whole release to discover, so every non-obvious claim below carries one of:

- **VERIFIED** — run on this machine (macOS 26.6 / build 25G72, Xcode 26.6,
  `notarytool` 1.1.2 (41)) while writing this document, or read out of the local
  `man` page for the tool in question. The command is quoted.
- **INVESTIGATION** — taken from the `dodo-ime-macos-scout` report (§8
  "Permissions, signing, notarization", §9 "Packaging"). Those experiments were
  **not** re-run here.
- **READ** — Apple's or GitHub's own documentation, fetched 2026-08-08, not
  executed. The URL is given.
- **INFERRED** — reasoning from the above. Called out as such every time.

---

## Contents

1. [What the repo owner must personally obtain](#1-what-the-repo-owner-must-personally-obtain)
2. [CI/CD secrets, by exact name](#2-cicd-secrets-by-exact-name)
3. [Entitlements — including the input method](#3-entitlements--including-the-input-method)
4. [What changes in the release workflow](#4-what-changes-in-the-release-workflow)
5. [What signing buys, and what it does not](#5-what-signing-buys-and-what-it-does-not)
6. [What breaks if we get it wrong, and how to verify success](#6-what-breaks-if-we-get-it-wrong-and-how-to-verify-success)
7. [Compatibility audit — what is already in the tree](#7-compatibility-audit--what-is-already-in-the-tree)

---

## 1. What the repo owner must personally obtain

Nobody but the account holder can do any of this: it needs a credit card, an
Apple Account with two-factor authentication, and (for two of the items) the
**Account Holder** role. An agent cannot obtain any of it.

| # | What | Where it comes from | Money | Renewal |
|---|---|---|---|---|
| 1 | Apple Developer Program membership | developer.apple.com/programs/enroll | **US$99/year** | annual |
| 2 | Team ID | falls out of (1) | — | — |
| 3 | **Developer ID Application** certificate + its private key, exported as one `.p12` | Certificates, Identifiers & Profiles | included in (1) | 5 years |
| 4 | A password for that `.p12` | you invent it | — | — |
| 5 | A notarisation credential — **either** an app-specific password **or** an App Store Connect API key | account.apple.com **or** App Store Connect | included in (1) | see below |
| 6 | (organisations only) a D-U-N-S number | dnb.com, free | free | — |

Items 3, 4 and 5 are the ones that become CI secrets. Items 1, 2 and 6 are
account facts.

### 1.1 — Apple Developer Program membership

- **Cost: US$99 (or local equivalent) per membership year.** **READ**
  ([compare-memberships](https://developer.apple.com/support/compare-memberships/)).
  Nonprofits, educational institutions and government entities may qualify for a
  [fee waiver](https://developer.apple.com/support/membership-fee-waiver/).
- **Individual/sole proprietor vs organisation is a real fork, and it is
  visible to users.** **READ**, same page.
  - *Individual*: enrol with your own Apple Account. No D-U-N-S number. The
    certificate's common name — and therefore the string Gatekeeper shows and
    the string `codesign -dvvv` prints — is **your legal personal name**:
    `Developer ID Application: Jane Doe (ABCDE12345)`.
  - *Organisation*: needs a free **D-U-N-S number** registered to the legal
    entity, and the name shown is the company's.
  - dodo is a personal project by one author, so *individual* is the default
    reading — but it publishes the author's legal name in every downloaded
    binary forever. That is a privacy decision, not a technical one, and it is
    the repo owner's to make. It is also **not reversible without a new Team
    ID**, which changes the app's TCC identity (see §5).
- **Membership lapsing does not brick what is already shipped.** **READ**
  ([create Developer ID certificates](https://developer.apple.com/help/account/certificates/create-developer-id-certificates/)):
  existing signed apps keep distributing; you just cannot create new
  certificates or sign new builds until it is renewed. Combined with the secure
  timestamp (§4), an expired membership does not retroactively break v0.1.x.

**What it looks like when you have it:** you can sign in at
`developer.apple.com/account` and see "Certificates, Identifiers & Profiles" in
the sidebar.

### 1.2 — Team ID

- A **10-character alphanumeric string**, e.g. `ABCDE12345`.
- Found at developer.apple.com/account → **Membership details** → Team ID. It
  also appears in parentheses at the end of every certificate name.
- **Used for:** `notarytool --team-id`, disambiguating the signing identity, and
  — the part that matters later — it is the identity that the nested input-method
  bundle must share with `Dodo.app` (§3, §7).
- Not a secret in the security sense (it is embedded in every signed binary and
  anyone can read it with `codesign -dvvv`), but it goes in CI as a secret
  anyway so the workflow reads uniformly.

### 1.3 — Developer ID Application certificate

This is the certificate that signs software distributed **outside** the Mac App
Store. It is the only certificate type dodo needs.

- **You do not need "Developer ID Installer".** That one signs `.pkg`
  installers. dodo ships `.tar.gz` and `.zip` (`scripts/package.sh`,
  `scripts/package.ps1`) and has no installer. If an `.msi`-equivalent `.pkg` is
  ever added (`docs/release.md`, "Future readiness" mentions MSI for Windows),
  that is when a second certificate becomes necessary.
- **You do not need "Apple Development" / "Apple Distribution".** Those are for
  Xcode-signed development builds and App Store submission. The Mac App Store is
  closed to us anyway once the input method exists — **INVESTIGATION** §8: an
  IME must be installed into `~/Library/Input Methods`, which App Store Review
  Guidelines 2.14/2.15 forbid.

**How to create it** — **READ**, from
[Apple's own steps](https://developer.apple.com/help/account/certificates/create-developer-id-certificates/):

1. **Role requirement: Account Holder.** A team member with the "Admin" role
   *cannot* create a Developer ID certificate. For a one-person team this is
   automatic.
2. On the Mac, open **Keychain Access → Certificate Assistant → Request a
   Certificate From a Certificate Authority**. Enter the Apple Account email and
   a common name; choose **Saved to disk**. This writes a
   `CertificateSigningRequest.certSigningRequest` file **and silently creates
   the matching private key in your login keychain**. That private key is the
   thing that actually matters — the `.cer` Apple hands back is worthless
   without it, and it exists on exactly one machine until you export it.
3. developer.apple.com/account → **Certificates** → **+** → under *Software*
   pick **Developer ID** → **Developer ID Application** → upload the CSR →
   **Download** the `.cer`.
4. Double-click the `.cer` to install it into the login keychain, where it pairs
   with the private key from step 2.
5. Confirm: `security find-identity -v -p codesigning` should list
   `"Developer ID Application: <name> (<TEAMID>)"`.

**Limits and lifetime** — **READ**, same page:

- **Maximum five Developer ID Application certificates per account.** They are
  not disposable; do not create one per experiment.
- Apps signed while the certificate was valid **keep running after it expires**.
  You need a valid one only to sign something new.

**How it becomes a CI secret** (this is item 3 + 4 in the table):

```bash
# In Keychain Access, select BOTH the certificate and its private key,
# right-click → "Export 2 items…", format "Personal Information Exchange (.p12)".
# You are asked to invent a password — that password is MACOS_CERTIFICATE_PWD.
#
# Then, to produce the secret value (macOS `base64` has no -w flag; GNU needs -w0):
base64 -i DeveloperID.p12 | pbcopy
```

**Keep the `.p12` and its password somewhere durable and offline.** If the
laptop dies, the private key dies with it; the certificate cannot be
re-downloaded in a usable form and you burn one of the five slots making a new
one.

### 1.4 — A notarisation credential: pick one of two

Notarisation is a separate authentication from signing. `notarytool` accepts two
forms — **VERIFIED**, from `xcrun notarytool submit --help` on this machine.

**Option A — Apple ID + app-specific password.** Three values:
`--apple-id`, `--team-id`, `--password`.

- The app-specific password is created at
  [account.apple.com](https://account.apple.com) → **Sign-In and Security** →
  **App-Specific Passwords** → **+**. Two-factor authentication on the Apple
  Account is a prerequisite. **READ**
  ([Apple Support HT204397 / 102654](https://support.apple.com/en-us/102654)).
- **What it looks like:** four lowercase groups of four,
  `abcd-efgh-ijkl-mnop`. It is shown **once**; there is no way to read it back.
- It is a credential for the whole Apple Account, not scoped to notarisation. It
  can be revoked individually from the same page.

**Option B — App Store Connect API key. RECOMMENDED.** Three values:
`--key` (a file path), `--key-id`, `--issuer`.

- Created at [App Store Connect](https://appstoreconnect.apple.com) → **Users
  and Access** → **Integrations** → **App Store Connect API** → **Keys** → **+**.
  Choose a **Team Key**; the "Developer" access role is sufficient for
  notarisation.
- **What it looks like:** a file `AuthKey_XXXXXXXXXX.p8` (a PEM-wrapped EC
  private key, ~250 bytes), **downloadable exactly once**; a **Key ID**
  (10 alphanumeric characters, also in the filename); and an **Issuer ID**
  (a UUID, shown once at the top of the Keys page and the same for every key on
  the team).
- **The `--issuer` rule is a real trap**, and it is documented in the tool
  itself — **VERIFIED**, `xcrun notarytool submit --help`:
  > `--issuer <issuer>` App Store Connect API Issuer ID, UUID format.
  > **Required for Team API Keys. Do not provide for Individual API Keys.**
  Passing it for an Individual key, or omitting it for a Team key, fails
  authentication with an error that does not say which of the two you did.

**Why B is recommended:** the key is scoped to App Store Connect and revocable
without touching the Apple Account's own sign-in; an app-specific password is a
credential for *everything* the Apple Account can do that does not require 2FA.
For a credential that will sit in a GitHub secret, the narrower one wins.

**Local convenience, not for CI:** on a developer machine you can collapse all
of this into one keychain profile with
`xcrun notarytool store-credentials <profile-name>` and then use
`--keychain-profile <profile-name>` — **VERIFIED**, `notarytool
store-credentials --help`. That is the right way to rehearse by hand before
wiring CI. It is *not* usable in a runner, which has no persistent keychain.

### 1.5 — What the repo owner does *not* need

Worth stating, because each of these is a thing people go and get by mistake:

- **No Apple Silicon-specific certificate.** One Developer ID Application
  certificate signs both `aarch64` and `x86_64` builds.
- **No provisioning profile.** Developer ID distribution outside the App Store
  does not use one.
- **No App ID / Bundle ID registration.** `io.github.mrgru.dodo` is asserted by
  `scripts/macos-app-bundle.sh` and does not need to be registered for
  Developer ID signing. (It *would* for App Store submission, which is closed to
  us — §1.3.)
- **No hardware token, no HSM.** That is EV Windows signing, not macOS.
- **Nothing for Windows or Linux.** `WINDOWS_CERTIFICATE` in
  `docs/release.md`'s table is a separate, unrelated purchase from a commercial
  CA; Linux packages are unsigned by convention here.

---

## 2. CI/CD secrets, by exact name

`docs/release.md` ("Required GitHub Secrets") already reserves five names. This
section says what goes in each and how to produce the value. **None of these is
referenced by any workflow today.**

| Secret | Route | Contents | How to produce the value |
|---|---|---|---|
| `MACOS_CERTIFICATE` | both | base64 of the Developer ID Application `.p12` | `base64 -i DeveloperID.p12 \| pbcopy` |
| `MACOS_CERTIFICATE_PWD` | both | the password you invented when exporting the `.p12` | you chose it (§1.3) |
| `MACOS_NOTARY_TEAM_ID` | both | the 10-character Team ID | developer.apple.com → Membership details |
| `MACOS_NOTARY_APPLE_ID` | A only | the Apple Account email | it is your email |
| `MACOS_NOTARY_PASSWORD` | A only | the app-specific password, `abcd-efgh-ijkl-mnop` | account.apple.com (§1.4) |

If **Option B** (App Store Connect API key) is chosen — which this document
recommends — three further names are needed. They are proposed here, not
reserved anywhere yet:

| Secret | Contents | How to produce the value |
|---|---|---|
| `MACOS_NOTARY_API_KEY` | base64 of `AuthKey_XXXXXXXXXX.p8` | `base64 -i AuthKey_XXXXXXXXXX.p8 \| pbcopy` |
| `MACOS_NOTARY_API_KEY_ID` | the 10-character Key ID | shown in App Store Connect and in the filename |
| `MACOS_NOTARY_API_ISSUER_ID` | the Issuer UUID | top of the App Store Connect → Keys page |

With Option B, `MACOS_NOTARY_APPLE_ID` and `MACOS_NOTARY_PASSWORD` go unused.
`MACOS_NOTARY_TEAM_ID` is still needed — for `codesign`, not for `notarytool`.

**Deliberately not a secret: the keychain password.** Recipes on the internet
often add a `MACOS_KEYCHAIN_PASSWORD`. It protects a keychain that exists for
one job on one ephemeral runner and is destroyed with it, so generating it in
the step (`uuidgen`) is strictly better than storing a long-lived secret that
buys nothing.

**Deliberately not a secret: the signing identity string.** `codesign --sign`
accepts the Team ID and resolves it, as long as exactly one matching identity is
in the keychain — which is exactly the situation on a freshly created temporary
keychain. So `--sign "$MACOS_NOTARY_TEAM_ID"` is enough and there is no
`MACOS_SIGN_IDENTITY` to keep in step with a certificate rename.

### 2.1 — Setting up the keychain on a runner

This is the part that is fiddly for reasons that are not obvious, so it is
written out. **INFERRED from the tool documentation; not executed here** — there
is no certificate to execute it with.

```bash
KEYCHAIN="$RUNNER_TEMP/dodo-signing.keychain-db"
KEYCHAIN_PWD="$(uuidgen)"

security create-keychain -p "$KEYCHAIN_PWD" "$KEYCHAIN"
# Without this the keychain re-locks after 5 minutes of inactivity and a later
# codesign fails halfway through a matrix job with an opaque error.
security set-keychain-settings -lut 21600 "$KEYCHAIN"
security unlock-keychain -p "$KEYCHAIN_PWD" "$KEYCHAIN"

printf '%s' "$MACOS_CERTIFICATE" | base64 --decode > "$RUNNER_TEMP/cert.p12"
security import "$RUNNER_TEMP/cert.p12" -k "$KEYCHAIN" \
    -P "$MACOS_CERTIFICATE_PWD" -T /usr/bin/codesign
rm -f "$RUNNER_TEMP/cert.p12"

# THE STEP EVERYONE FORGETS. Without it macOS wants to show a "codesign wants to
# use the key ... allow?" dialog. A runner has no one to click it, so codesign
# blocks and then fails with errSecInternalComponent, which says nothing about
# the cause.
security set-key-partition-list -S apple-tool:,apple:,codesign: \
    -s -k "$KEYCHAIN_PWD" "$KEYCHAIN" >/dev/null

# codesign searches the *search list*, not a path. Prepending is what makes the
# imported identity findable; keeping login.keychain on the list avoids
# surprising other tools in the job.
security list-keychain -d user -s "$KEYCHAIN" login.keychain-db
```

### 2.2 — The `if:` guard that does not work

`.github/workflows/release.yml`'s future-signing comment currently records the
guard as `if: runner.os == 'macOS' && secrets.MACOS_CERTIFICATE != ''`. **That
expression is invalid.** **READ**
([GitHub contexts reference](https://docs.github.com/en/actions/reference/workflows-and-actions/contexts),
fetched 2026-08-08): the `secrets` context is not available in
`jobs.<job_id>.if` or `jobs.<job_id>.steps.if`. It *is* available in
`jobs.<job_id>.env` and `jobs.<job_id>.steps.env`, and the `env` context *is*
available in `steps.if`. So the working shape is:

```yaml
    env:
      # job level, where `secrets` is legal
      MACOS_CERTIFICATE: ${{ secrets.MACOS_CERTIFICATE }}
    steps:
      - name: Sign, notarise and staple (macOS)
        if: runner.os == 'macOS' && env.MACOS_CERTIFICATE != ''
```

The comment in `release.yml` has been corrected to this form in the same commit
as this document. Nothing was enabled.

---

## 3. Entitlements — including the input method

### 3.1 — Hardened runtime is not optional

The notary service refuses any submission whose executables are not signed with
the hardened runtime. That is `codesign --options runtime`. **VERIFIED** from
the local `man codesign`:

> `runtime`  On macOS versions >= 10.14.0, opts signed processes into a hardened
> runtime environment which includes runtime code signing enforcement, library
> validation, hard, kill, and debugging restrictions. These restrictions can be
> selectively relaxed via entitlements.

The hardened runtime also requires a **secure timestamp** (`--timestamp`), which
contacts Apple's timestamp authority at signing time. Both are non-negotiable
for notarisation.

### 3.2 — dodo needs no entitlements at all

Hardened runtime restrictions are relaxed *by* entitlements; the question is
whether dodo does anything that the hardened runtime forbids. Going through the
list that actually applies to a Rust GPUI app, against
`Cargo.toml` as it stands (**VERIFIED** by reading this tree):

| Entitlement | Needed? | Why |
|---|---|---|
| `com.apple.security.cs.allow-jit` | **no** | The only scripting engine is `rquickjs` (QuickJS-NG), a bytecode interpreter. It generates no executable pages. |
| `com.apple.security.cs.allow-unsigned-executable-memory` | **no** | Same reason. Nothing writes-then-executes. |
| `com.apple.security.cs.disable-library-validation` | **no** | Everything is statically linked — `rusqlite` with `bundled`, `rquickjs-sys` compiling C in-tree, rustls rather than OpenSSL. The only dynamic libraries loaded are Apple's own frameworks, which library validation permits. |
| `com.apple.security.cs.allow-dyld-environment-variables` | **no** | dodo does not inject anything via `DYLD_*`. |
| `com.apple.security.cs.disable-executable-page-protection` | **no** | Nothing patches its own text segment. |
| `com.apple.security.get-task-allow` | **must be ABSENT** | This is the debugger-attach entitlement Xcode adds to debug builds. Its presence is an automatic notarisation rejection. Release builds via `cargo build --release` never set it, but a hand-signed local experiment can. |
| `com.apple.security.app-sandbox` | **no** | dodo is not sandboxed and cannot be — the Cleaner reads arbitrary paths, the Docker module talks to a unix socket, the Database module opens arbitrary SQLite files. Sandboxing is a Mac App Store requirement and the App Store is closed to us (§1.3). |
| network entitlements | **n/a** | Only the sandbox restricts network. Hardened runtime does not. |

So the entitlements file for `Dodo.app` is *empty*, which means **there is no
`--entitlements` argument at all**. Do not create a plist with
`<dict></dict>` "for completeness"; an empty entitlements dictionary and no
entitlements are the same thing to the notary service, and the file becomes one
more thing to keep correct.

**A related thing that is not an entitlement, and is worth checking when signing
lands.** dodo's Cleaner scans `~/Downloads`, `~/Desktop`, `~/Documents` and
`~/Movies` (`src/cleaner/macos/cleanup.rs`, `scanners/large_old_files.rs` —
**VERIFIED** by reading them). Those are TCC-protected directories. TCC consent
is orthogonal to signing and to the hardened runtime — the prompt happens either
way — but `Info.plist` usage-description strings
(`NSDesktopFolderUsageDescription`, `NSDocumentsFolderUsageDescription`,
`NSDownloadsFolderUsageDescription`, `NSRemovableVolumesUsageDescription`)
control the sentence the user reads in that prompt, and
`scripts/macos-app-bundle.sh` sets none of them today. **READ**, not verified
here, and **deliberately not changed by this round** — it is a user-visible
behaviour change unrelated to signing. Flagged so it is not discovered *during*
a signing rollout.

### 3.3 — The input-method bundle needs nothing beyond the app's

The short answer the brief asks for: **no.** Concretely:

- **It must be signed with the hardened runtime too** (`--options runtime`),
  because the notary service checks every executable in the submission, not just
  the outer one.
- **It must be signed with the same Team ID as `Dodo.app`.** This is forced
  anyway: `codesign` refuses to seal nested code from a different team.
- **It needs no entitlement to be an input method.** **INVESTIGATION** §8, and
  it is the whole architectural argument for InputMethodKit over a
  `CGEventTap`: the PoC "never linked or called any `AX*` API, never created an
  event tap, and no TCC prompt appeared across ~20 launches. It received every
  keystroke anyway." There is no Accessibility permission, no Input Monitoring
  permission, and therefore no entitlement gating any of it. The user's consent
  *is* adding the input source in System Settings.
- **It needs no entitlement to do its IPC.** The design is two single-writer
  JSON files under `data_dir()` plus an `NSDistributedNotificationCenter` ping
  (**INVESTIGATION** §7, verified there in both directions with "no app group,
  no XPC service, no shared team identifier, no entitlement"). None of those is
  restricted by the hardened runtime. See §7.1 for why signing does not change
  this design.
- **The one case where it would need entitlements is sandboxing**, and that case
  does not exist: an input method cannot ship through the Mac App Store
  (**INVESTIGATION** §8), which is the only thing that would require the
  sandbox. If it were ever sandboxed, the file-based IPC would need a
  `com.apple.security.temporary-exception.files.home-relative-path` entitlement
  — which is the point at which an App Group would become the tidier answer.

So: one `codesign` invocation for the inner bundle, same flags as the outer, no
`--entitlements` on either.

---

## 4. What changes in the release workflow

### 4.1 — The ordering constraint, stated once

There are five orderings, and getting any of them wrong produces a failure that
looks like something else:

1. **Inner bundle before outer.** `codesign` signs inside-out. Code nested in a
   bundle must already be signed, or signing the outer bundle fails —
   **VERIFIED** from the local `man codesign`: *"Code nested within bundle
   directories must already be signed or the signing operation will fail"*.
2. **Do not use `--deep` to achieve that.** **VERIFIED**, same man page:
   *"(DEPRECATED for signing as of macOS 13.0)"*, and *"All signing options will
   be applied, in turn, to all nested content. This is almost never what you
   want."* `--deep` remains correct and useful for **verification**.
3. **Sign before archiving, not after.** `scripts/package.sh` tars the bundle at
   the moment it is built (line 202) and checksums it immediately (line 204).
   The published SHA-256 and `update.json` entry are computed from that archive,
   so anything done to the bundle after the tar is invisible to the release.
4. **Notarise after signing, staple after notarising.** The notary service
   validates the signature; the ticket is issued against it.
5. **Never sign after stapling.** **VERIFIED** from `man stapler`:
   *"Code-signing a supported file format invalidates any stapled tickets, so
   `stapler staple` must be run again if this occurs."*

### 4.2 — Where the steps actually go

`.github/workflows/release.yml`'s current comment says the sign hook goes
"between packaging and upload". **That placement is wrong**, per constraint 3
above: by then the `.tar.gz` and its `.sha256` already exist and contain the
unsigned bundle. The comment has been corrected in the same commit as this
document.

The correct home is **inside the packaging scripts**, which is where
`scripts/macos-app-bundle.sh`'s own footer has always implied it:

```
scripts/package.sh
  ├─ stage the plain binary                       (line 100)
  │    → SIGN the binary copy in the stage dir     ← new
  ├─ tar  dodo-vX-macos-arm64.tar.gz              (line 182)
  ├─ checksum                                      (line 184)
  │
  └─ if --app-bundle:
       scripts/macos-app-bundle.sh                 (line 199)
         ├─ assemble dodo.app
         ├─ SIGN  Contents/Helpers/<IME>.app       ← new, when the IME exists
         ├─ SIGN  dodo.app                         ← new
         ├─ ditto -c -k --keepParent → a TEMPORARY zip
         ├─ xcrun notarytool submit --wait         ← new
         ├─ xcrun stapler staple dodo.app          ← new
         └─ rm the temporary zip
       tar  dodo-vX-macos-arm64-app.tar.gz        (line 202)
       checksum                                    (line 204)
```

The temporary zip exists **only** to satisfy the notary service, which does not
accept a `.tar.gz` — it takes a zip, a UDIF disk image, or a flat installer
package (**READ**; `notarytool submit --help` says only "Path to the archive").
`ditto -c -k --keepParent` is the correct way to build it. The *published*
artifact stays `dodo-vX-macos-arm64-app.tar.gz`, because
`tools/update-manifest/src/platform.rs` selects the macOS entry **by exact
filename** and `models::manifest` asserts the URL ends in `-app.tar.gz`
(`AGENTS.md`, `docs/release.md`). **Renaming the artifact would break the
in-app updater; do not "simplify" the zip into being the release asset.**

### 4.3 — The plain-binary archive can be signed but never stapled

`scripts/package.sh` publishes two macOS archives. The `.app` one can be
notarised and stapled. The plain-binary one can be signed and notarised, but
**not stapled**: **VERIFIED** from `man stapler` — *"stapler works only with
UDIF disk images, signed 'flat' installer packages, and certain code-signed
executable bundles such as '.app'."* A bare Mach-O is none of those.

The practical consequence is small: a notarised-but-unstapled binary is accepted
by Gatekeeper only when the machine can reach Apple to check online, and the
plain archive is the one a terminal user extracts and runs, where Gatekeeper's
quarantine path is already different. Sign it anyway — it costs one extra
`codesign` call and it makes `codesign -dvvv` on the shipped binary say something
truthful.

### 4.4 — Two knock-on effects worth knowing before you start

- **Signing needs the network, twice.** `--timestamp` contacts Apple's timestamp
  authority; `notarytool` obviously does. A release build becomes dependent on
  Apple's availability. `notarytool submit --wait` typically returns in minutes
  but has no upper bound; the macOS matrix job's timeout needs to accommodate it.
- **The `-app.tar.gz` stops being byte-reproducible.** `docs/build-optimization.md`
  ("Reproducibility") and `SOURCE_DATE_EPOCH` in `release.yml` exist so that
  re-running a release for the same tag produces the same bytes. A secure
  timestamp is *the current time from Apple's TSA* and a notarisation ticket is
  issued per submission, so two runs of the same tag will differ. This is not
  fixable and not a defect; it should be written into
  `docs/build-optimization.md` when signing lands, so the next person does not
  spend a day chasing it.

---

## 5. What signing buys, and what it does not

**Be honest about this: for the input method, signing is a user-experience
purchase, not an enablement one.**

### It does not buy the ability to run

**INVESTIGATION** §8 tested three states — as `cargo build` leaves it
(`adhoc,linker-signed`), properly ad-hoc signed, and ad-hoc signed *with*
`com.apple.quarantine` set (the real "downloaded from GitHub" case). All three
**launched and typed**:

> on macOS 26.6, an unsigned, unnotarized, quarantined input method dropped into
> `~/Library/Input Methods` does launch and does type. `spctl` says "rejected";
> that is not the gate that applies here.

That experiment was **not re-run here**. It is the single most important fact in
this document and it is somebody else's measurement — but it was measured three
ways on the same OS version this file was written on.

### The current workaround, which already ships

dodo's generated release notes tell users to run

```
xattr -dr com.apple.quarantine /Applications/dodo.app
```

(`docs/release.md`, and `src/updater/services/installers/macos.rs` does the
equivalent automatically for in-app updates). That line is the mitigation, it is
already there, and it is sufficient for the app to run today.

### What it does buy

| | Unsigned today | Signed + notarised + stapled |
|---|---|---|
| First launch of a downloaded `dodo.app` | *"…can't be opened because Apple cannot check it for malicious software"*; user must clear quarantine or right-click → Open | opens |
| App Translocation | a quarantined bundle runs from a randomised read-only path under `/private/var/folders/…/AppTranslocation/` (**INVESTIGATION** §8, verified) | does not happen |
| `spctl --assess --type execute` | `rejected` | `accepted` |
| Input method launches and types | **yes** | yes |
| TCC grants surviving an update | see below | see below |

**The TCC point is the one the investigation did not make, and it is the
strongest technical argument.** dodo has an in-app updater that replaces the
whole bundle (`src/updater/services/installers/`), and a Cleaner that needs
access to `~/Downloads`, `~/Desktop` and `~/Documents`. macOS keys a TCC grant
to the requesting application's *identity*: for a signed app that is the
designated requirement (bundle ID + Team ID), which is stable across versions;
for an unsigned or ad-hoc-signed app there is no such identity and the grant is
tied to the binary on disk. **INFERRED**, and marked as such: this means an
unsigned dodo plausibly re-prompts for Desktop/Documents/Downloads access after
every self-update, and a signed one does not. It is worth confirming empirically
before it is used as the deciding argument, and it costs one update cycle to
confirm.

### What it does not buy, beyond the above

- It does not make the Mac App Store possible (§1.3).
- It does not remove the need for the user to add the input source in System
  Settings. That step is the consent model, not a permission dialog.
- It does not sign Windows or Linux artifacts.
- It does not verify the update manifest. `update.json`'s `signature` field is a
  *different, unrelated* mechanism, still `null`, still unimplemented
  (`docs/release.md`, "Verification is not a signature check"). Developer ID
  signing and manifest signing solve different problems and neither substitutes
  for the other.

---

## 6. What breaks if we get it wrong, and how to verify success

### 6.1 — Failure modes, with the error you will actually see

| What went wrong | Symptom |
|---|---|
| `security set-key-partition-list` omitted | `codesign` hangs, then fails with `errSecInternalComponent`. Nothing mentions the keychain. |
| Keychain not on the search list | `Developer ID Application: …: no identity found` |
| Outer bundle signed before the inner one | signing fails on the outer bundle; if it somehow succeeds, `codesign --verify --deep` later reports `code object is not signed at all` for the nested path |
| `--timestamp` omitted | notarisation rejected: *the signature does not include a secure timestamp* |
| `--options runtime` omitted | notarisation rejected: *the executable does not have the hardened runtime enabled* |
| `get-task-allow` entitlement present | notarisation rejected outright |
| A `.tar.gz` handed to `notarytool` | submission rejected as an unsupported archive format |
| `stapler staple` run before notarisation finished | *The staple and validate action failed! Error 65* |
| Anything re-signed after stapling | ticket silently invalidated; `stapler validate` fails on the shipped archive (**VERIFIED** from `man stapler`) |
| Archive built before signing | `codesign --verify` passes on `dist/dodo.app` and fails on the extracted copy; the published SHA-256 covers the unsigned bundle |
| Nested bundle in a directory `codesign` does not treat as nested code | it is sealed as an opaque *resource* rather than as code; `--verify --deep --strict` does not recurse into it and the extracted copy may not validate. See §7.2 — this is a live design decision, not a hypothetical |
| Team ID mismatch between inner and outer | outer signing fails |
| `.p12` or `.p8` committed to the repo | revoke immediately; one of five certificate slots is burnt |

### 6.2 — The verification recipe

Run all of these; each catches something the others do not.

```bash
# 1. Structural + cryptographic validity, recursing into nested code.
#    --deep is correct here (it is only deprecated for *signing*).
codesign --verify --deep --strict --verbose=2 dist/dodo.app

# 2. The nested input method on its own, as the user will have it after
#    dodo copies it to ~/Library/Input Methods.
codesign --verify --deep --strict --verbose=2 \
    "dist/dodo.app/Contents/Helpers/Dodo Vietnamese.app"

# 3. What was actually claimed: identity, Team ID, hardened runtime
#    (look for `flags=0x10000(runtime)`), and that entitlements are empty.
codesign -dvvv --entitlements - dist/dodo.app

# 4. The Gatekeeper policy check — the one that says `rejected` today.
spctl --assess --type execute --verbose=4 dist/dodo.app

# 5. The notarisation ticket is attached and matches.
xcrun stapler validate dist/dodo.app

# 6. THE ONE THAT MATTERS: all of the above, on what actually ships.
#    Steps 1–5 pass on dist/dodo.app trivially; the question is whether the
#    published archive still carries them.
mkdir -p /tmp/dodo-verify && tar -xzf dist/dodo-v*-macos-*-app.tar.gz -C /tmp/dodo-verify
codesign --verify --deep --strict --verbose=2 /tmp/dodo-verify/dodo.app
xcrun stapler validate /tmp/dodo-verify/dodo.app
spctl --assess --type execute --verbose=4 /tmp/dodo-verify/dodo.app
```

Step 6 belongs in `scripts/verify-release.sh` when signing lands, guarded on the
signing being on, so a regression is caught by the release rather than by a user.
§7.3 records why it is expected to pass.

If notarisation is rejected, the log is the only thing that says why:

```bash
xcrun notarytool log <submission-id> --team-id "$TEAM_ID" \
    --key AuthKey_XXXXXXXXXX.p8 --key-id "$KEY_ID" --issuer "$ISSUER_ID"
```

It returns JSON with one entry per offending binary. It is genuinely good; do
not guess before reading it.

---

## 7. Compatibility audit — what is already in the tree

The second half of this round's question: **would anything dodo builds now have
to be undone to get to signing?** This is what was checked.

### 7.1 — The file-based IPC stays correct, and signing does not make XPC worth revisiting

The input-method design (**INVESTIGATION** §7) rejects XPC, App Groups and a
shared `NSUserDefaults` suite, "because both require the two bundles to be
signed by the same team, and dodo signs nothing today". Signing removes that
particular objection. It does **not** make XPC the better answer, for three
reasons that have nothing to do with signing:

1. **The IME must work when `Dodo.app` is not running.** macOS launches the
   input method, not dodo. An XPC connection to a GUI app that is not running,
   and that is not a registered `launchd` service, simply fails —
   **INVESTIGATION** §7's comparison table. The file survives; the connection
   does not. This is the decisive one.
2. **The file is the durable copy anyway.** Per-app language memory and the
   config have to be on disk regardless, so XPC would be a *second* mechanism
   layered on a file that still has to exist.
3. **It matches every other thing dodo persists.** Nine JSON files under
   `data_dir()`, single-writer, explicit `"version"`, refuse-if-higher
   (`AGENTS.md`). A tenth and eleventh are boring in the good way.

**Verdict: no change.** The one scenario that would flip it — sandboxing the IME
— is not reachable (§3.3). Worth recording so a future session with a
certificate in hand does not reopen it as free.

There is one *positive* compatibility property here: `data_dir()`
(`crates/dodo-paths`) builds an absolute path from `$HOME` and is therefore
unaffected by App Translocation, which is what an unsigned quarantined bundle
suffers. So the IPC design is correct **both** before and after signing, and the
transition changes nothing about it.

### 7.2 — The nesting location is a real finding: `Contents/Library/InputMethods/` is not signable as nested code

**INVESTIGATION** §9 recommends nesting the IME at
`Dodo.app/Contents/Library/InputMethods/`. **VERIFIED here** from `man codesign`
on this machine, `codesign` discovers nested code content in exactly these
directories:

```
Contents                       Contents/XPCServices
Contents/Frameworks            Contents/Helpers
Contents/SharedFrameworks      Contents/MacOS
Contents/PlugIns               Contents/Library/Automator
Contents/Plug-ins              Contents/Library/Spotlight
                               Contents/Library/LoginItems
```

> If any code (Mach-Os, bundles) are located outside the above listed locations
> they will not be signed by the `--deep` option

`Contents/Library/InputMethods` is **not on that list**. A bundle there is
sealed by the resource rules as data rather than as nested code, which is the
shape that produces "a sealed resource is missing or invalid" and a `--deep
--strict` verification that does not actually look inside.

**And nothing requires that path.** macOS looks for input methods in
`~/Library/Input Methods` and `/Library/Input Methods` only; it never looks
inside another application. The location *inside* `Dodo.app` is purely dodo's
own filing choice, read by dodo's own install action and nothing else.

**Recommendation: nest it at `Dodo.app/Contents/Helpers/Dodo Vietnamese.app`.**
`Helpers` is on the discovery list and is semantically exactly right — a helper
executable the app carries. This costs nothing to adopt because **the nested
bundle does not exist yet**: `crates/dodo-ime-core/` is the engine and links
into nothing but dodo so far, and there is no macOS bundle and no packaging for
one. Adopting the constraint now is free; adopting it after
`scripts/macos-input-method-bundle.sh` and the install action are written is a
packaging change plus a migration for anyone who installed from a release. This
document is the record of that constraint; §4.2's diagram already uses
`Contents/Helpers/`.

Two smaller notes for whoever builds the install action:

- **Copy the IME out with `ditto`, not `cp -R`.** `ditto` preserves extended
  attributes and ACLs; `cp -R` does not. The signature itself lives in the
  Mach-O and in `_CodeSignature/`, so `cp -R` probably survives, but `ditto` is
  the documented way to move a signed bundle and removes the question.
- **Never write into `Dodo.app`.** Anything the install action or the IME writes
  inside the outer bundle invalidates its signature and its stapled ticket. The
  design already puts everything mutable in `data_dir()` (§7.1), so this is a
  property to preserve rather than a bug to fix.

### 7.3 — The `.tar.gz` distribution shape survives signing and stapling

Worth checking rather than assuming, because Apple's own advice is to distribute
signed apps in a `.zip` made with `ditto` or in a disk image — and dodo cannot
change its macOS `.app` artifact name without breaking the in-app updater (§4.2).

**VERIFIED here, in two parts:**

1. **Where the ticket lives.** On this machine, `/Applications/Android
   Studio.app` passes `xcrun stapler validate`, and its stapled ticket is an
   ordinary file at `Contents/CodeResources` — 7911 bytes, starting with the
   magic `s8ch`. (Not to be confused with `Contents/_CodeSignature/CodeResources`,
   which is the sealed-resource list.) It is a regular file, not an extended
   attribute and not a resource fork.
2. **That a `tar` round-trip keeps it.** A synthetic bundle of the same shape —
   `Contents/MacOS/<exe>` at mode 755, `Contents/_CodeSignature/CodeResources`,
   `Contents/CodeResources` — was archived with the exact `tar -czf` invocation
   `scripts/package.sh` uses and extracted with the exact `tar -xf` invocation
   `src/updater/services/installers/extract.rs` uses. Both `CodeResources` files
   came back byte-for-byte and the executable bit survived. Extended attributes
   did **not** survive, which is the correct outcome: nothing in a Developer ID
   signature lives in an extended attribute for a bundle.

**Conclusion: `-app.tar.gz` stays viable.** Neither the artifact name, the
manifest's exact-filename selection, nor the updater's `tar -xf` needs to change.
One caveat inherited from `tar`: keep the bundle free of symlinked framework
`Versions/` trees. dodo has no frameworks and the IME bundle will not either, so
this is a constraint to preserve rather than a problem to solve.

### 7.4 — The updater's `strip_quarantine` stays correct

`src/updater/services/installers/macos.rs` runs
`xattr -dr com.apple.quarantine` over the extracted bundle before swapping it
in. Removing an extended attribute does not touch the code signature or the
stapled ticket (§7.3 part 1: neither lives in an xattr), so this stays correct
after signing — it just stops being necessary. Leave it: it is already
documented as best-effort and non-fatal, and a user who obtained the archive
some other way still benefits.

The module's doc comment says "dodo's binaries are **not** code-signed or
notarised", which is true today and is the sentence to update when this lands.

### 7.5 — Everything else checked, with nothing to change

- **`crates/dodo-ime-core/`** — the precondition for the IME bundle existing at
  all is already met. The engine is a real library crate depending on
  `unicode-normalization` and nothing else, guarded by its own
  `src/purity_lint.rs`, so a `platform/macos-input-method` bundle can link it
  without linking gpui. That is what makes the nested bundle a *separately
  signable* Mach-O rather than a second copy of dodo. Signing does not interact
  with any of it.
- **`CFBundleIdentifier` is already frozen** at `io.github.mrgru.dodo`, and
  `scripts/macos-app-bundle.sh` says why in a comment that already anticipates
  signing: it is the App ID the certificate and the notarisation ticket bind to.
  Correct as written.
- **`LSMinimumSystemVersion` is 11.0**, comfortably above the 10.14 the hardened
  runtime needs and the 10.9 the notary service requires.
- **`update.json`'s `signature` field** is unrelated and stays `null`. Do not
  let a signing rollout imply it was addressed.
- **`deny.toml` / `THIRD-PARTY-NOTICES.md`** — the open GPL-3.0-or-later
  distribution question (`AGENTS.md`) is untouched by signing. Signing is about
  who built the binary, not about what may be distributed.
- **Windows.** `scripts/package.ps1`'s equivalent hook signs the `.exe` *before*
  zipping it, which is the same constraint 3 as above. Nothing here changes it.

**No incompatibility was found that requires code changes now.** The three
corrections made alongside this document are all to recorded *plans* — the
deprecated `--deep` recipe in `scripts/macos-app-bundle.sh`'s footer, the wrong
step placement and the invalid `secrets`-in-`if` guard in
`.github/workflows/release.yml` — plus the nesting-location constraint in §7.2,
which costs nothing because the thing it constrains has not been built.
