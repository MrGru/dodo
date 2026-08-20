# macOS code signing and notarisation

**Status: implemented, and never yet exercised against Apple.** The repo owner
holds an individual Apple Developer Program membership, the six secrets in §2
exist on `MrGru/dodo`, and the plumbing is in the tree:
`.github/workflows/release.yml` sets up the keychain, `scripts/package.sh` signs
and notarises the plain binary, and `scripts/macos-app-bundle.sh` signs the app,
notarises it and staples the ticket. **No release run has
gone through that path yet.** Everything about the *shape* of the code was
tested locally; everything that needs the certificate or the notary service was
not, and is marked accordingly below.

**The unsigned path is unchanged and is still the default.** With no secrets —
a fork, a clone, a local `scripts/package.sh` — every script ad-hoc signs
exactly as it always did and the release notes still tell users to clear
quarantine. That is not a fallback bolted on; it is the same code path, guarded.

**This is a separate file rather than a section of `docs/release.md` on
purpose.** `docs/release.md` is a runbook for someone cutting a release. This
is the design, the procurement checklist and the trap list behind one part of
it. Folding it in would add four hundred lines to an already long runbook.
`docs/release.md` links here from "Required GitHub Secrets" and "Future
readiness"; those remain the authority on how a release works, and this file is
the authority on signing.

## How to read the confidence labels

Signing is an area where plausible-sounding wrong answers are cheap and cost a
whole release to discover, so every non-obvious claim below carries one of:

- **VERIFIED** — run on this machine (macOS 26.6 / build 25G72, Xcode 26.6,
  `notarytool` 1.1.2 (41)) while writing this document, or read out of the local
  `man` page for the tool in question. The command is quoted.
- **INVESTIGATION** — recorded from an earlier local experiment and not re-run
  while this document was written.
- **READ** — Apple's or GitHub's own documentation, fetched 2026-08-08, not
  executed. The URL is given.
- **INFERRED** — reasoning from the above. Called out as such every time.

The implementation round added one more, and it is the important one:

- **UNEXERCISED** — written, reviewed against the documentation, and *not run
  against Apple*. Every `codesign --sign <real identity>`, every
  `notarytool submit`, every `stapler staple` in this repository is
  UNEXERCISED until a real release run says otherwise. The certificate and the
  notarisation key live on the repo owner's machine and in GitHub secrets; the
  session that wrote the code had neither, and deliberately did not fake one.
  What *was* run is stated where it applies.

---

## Contents

1. [What the repo owner must personally obtain](#1-what-the-repo-owner-must-personally-obtain)
2. [CI/CD secrets, by exact name](#2-cicd-secrets-by-exact-name)
3. [Entitlements](#3-entitlements)
4. [What the release workflow does](#4-what-the-release-workflow-does)
5. [What signing buys, and what it does not](#5-what-signing-buys-and-what-it-does-not)
6. [What breaks if we get it wrong, and how to verify success](#6-what-breaks-if-we-get-it-wrong-and-how-to-verify-success)
7. [Compatibility audit — what is already in the tree](#7-compatibility-audit--what-is-already-in-the-tree)
8. [What is still owed](#8-what-is-still-owed)

---

## 1. What the repo owner must personally obtain

**All of this has been done.** The membership is an **individual** one, the
signing identity is `Developer ID Application: Nguyen Manh Duan (8C925DTA32)`
and the Team ID is `8C925DTA32`; notarisation uses **Option B**, the App Store
Connect API key. The section is kept because it is the record of *what* each
value is and how it is renewed or replaced — the day the certificate expires,
or the key is rotated, this is the checklist again.

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
  - dodo is a personal project by one author, and *individual* is what the repo
    owner enrolled as — so every downloaded binary carries the author's legal
    name, forever, and `codesign -dvvv` prints it. That was a privacy decision
    taken knowingly. It is **not reversible without a new Team ID**, which
    changes the app's TCC identity (see §5).
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
  what identifies `Dodo.app` to macOS across updates.
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
  Xcode-signed development builds and App Store submission; dodo distributes
  directly with Developer ID.

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

**Option B was chosen.** `MACOS_NOTARY_APPLE_ID` and `MACOS_NOTARY_PASSWORD`
are **not set** on the repository and nothing in the tree reads them; Option A
stays documented only so a future reader knows what the alternative was and why
it lost. The key is a **Team** key, so `--issuer` is passed unconditionally —
see the trap below.

**Option A — Apple ID + app-specific password. NOT USED.** Three values:
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

**Option B — App Store Connect API key. IN USE.** Three values:
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
  dodo's key is a **Team** key, so both packaging scripts pass `--issuer`
  unconditionally. Do not "make it conditional" to be safe: for this key,
  omitting it is the failure.

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

**Six secrets, all of them set on `MrGru/dodo`, and all six read by
`.github/workflows/release.yml`'s `build` job.** They are read at *job* level,
because `secrets` is not available in any `if:` — §2.2.

| Secret | Contents | How to produce the value |
|---|---|---|
| `MACOS_CERTIFICATE` | base64 of the Developer ID Application `.p12` | `base64 -i DeveloperID.p12 \| pbcopy` |
| `MACOS_CERTIFICATE_PWD` | the password you invented when exporting the `.p12` | you chose it (§1.3) |
| `MACOS_NOTARY_TEAM_ID` | the 10-character Team ID, `8C925DTA32` | developer.apple.com → Membership details |
| `MACOS_NOTARY_API_KEY` | base64 of `AuthKey_XXXXXXXXXX.p8` | `base64 -i AuthKey_XXXXXXXXXX.p8 \| pbcopy` |
| `MACOS_NOTARY_API_KEY_ID` | the 10-character Key ID | shown in App Store Connect and in the filename |
| `MACOS_NOTARY_API_ISSUER_ID` | the Issuer UUID | top of the App Store Connect → Keys page |

`MACOS_NOTARY_TEAM_ID` is not a `notarytool` argument under Option B — the API
key authenticates on its own. It is what `codesign --sign` resolves the identity
from, and it is the reason there is no separate identity secret.

**`MACOS_NOTARY_APPLE_ID` and `MACOS_NOTARY_PASSWORD` are the Option A names and
are deliberately absent.** Nothing reads them. Setting them would do nothing;
adding a code path for them would add a second, untested authentication route to
maintain.

**All six or none.** The keychain step fails with a named missing secret if
`MACOS_CERTIFICATE` is present and any of the other five is not. Half a
credential set otherwise produces failures that look like something else — an
identity `codesign` cannot resolve, or an authentication error `notarytool` will
not attribute.

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

This is the part that is fiddly for reasons that are not obvious. It is now the
"Set up signing keychain (macOS)" step in `.github/workflows/release.yml`, which
is the authority on the exact text; the sketch below is kept because it is the
*reasoning*, comment for comment. **UNEXERCISED** — the step has never run
against a real certificate. The reasoning is **INFERRED** from the tool
documentation, as it always was.

Two things the workflow adds beyond the sketch. It asserts that at least one
`Developer ID Application` identity actually landed in the keychain, and it does
so by **counting** rather than printing: the identity's common name is the
certificate holder's legal name, and while that is public the moment anything
ships, a CI log is no place to put it. And it deletes the keychain and the
decoded `.p8` in an `if: always()` step in the same job, so a failed build does
not leave credentials on the runner.

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

An earlier note in `.github/workflows/release.yml` recorded the guard as
`if: runner.os == 'macOS' && secrets.MACOS_CERTIFICATE != ''`. **That
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

That is the shape `release.yml` uses, in three places: the keychain step, the
credential-cleanup step, and the `meta` job, which resolves once whether this
run is a signed one so the publish job can write truthful release notes. A
matrix job cannot answer that question for the publish job — four rows, one
output slot — so `meta` reads the secret and emits a `macos_signed` output.

**VERIFIED by execution** that the release-notes step produces the right text in
both directions: with `MACOS_SIGNED=true` it describes a signed, notarised,
stapled `dodo.app` and the online-checked plain binary; with the variable unset
it prints the original `xattr -dr com.apple.quarantine` instructions unchanged.
The guard *expressions* themselves are **UNEXERCISED** — GitHub evaluates them,
not a shell.

---

## 3. Entitlements

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
| `com.apple.security.app-sandbox` | **no** | dodo distributes outside the Mac App Store and needs arbitrary filesystem, Docker socket and database access. |
| network entitlements | **n/a** | Only the sandbox restricts network. Hardened runtime does not. |

So the entitlements file for `Dodo.app` is *empty*, which means **there is no
`--entitlements` argument at all**. Do not create a plist with
`<dict></dict>` "for completeness"; an empty entitlements dictionary and no
entitlements are the same thing to the notary service, and the file becomes one
more thing to keep correct.

**A related thing that is not an entitlement, and is worth checking when signing
lands.** dodo's Cleaner scans `~/Downloads`, `~/Desktop`, `~/Documents` and
`~/Movies` (`crates/dodo-cleaner/src/macos/cleanup.rs`, `scanners/large_old_files.rs` —
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

---

## 4. What the release workflow does

### 4.1 — The ordering constraint, stated once

There are three orderings, and getting any of them wrong produces a failure that
looks like something else:

1. **Sign before archiving, not after.** `scripts/package.sh` tars the bundle at
   the moment it is built (line 202) and checksums it immediately (line 204).
   The published SHA-256 and `update.json` entry are computed from that archive,
   so anything done to the bundle after the tar is invisible to the release.
2. **Notarise after signing, staple after notarising.** The notary service
   validates the signature; the ticket is issued against it.
3. **Never sign after stapling.** **VERIFIED** from `man stapler`:
   *"Code-signing a supported file format invalidates any stapled tickets, so
   `stapler staple` must be run again if this occurs."*

### 4.2 — Where the steps actually go

An earlier note in `.github/workflows/release.yml` said the sign hook goes
"between packaging and upload". **That placement is wrong**, per constraint 1
above: by then the `.tar.gz` and its `.sha256` already exist and contain the
unsigned bundle.

The signing lives **inside the packaging scripts**, and the workflow owns only
the keychain, which is the one part that is a property of the runner rather than
of the package. This is what runs:

```
release.yml  build job (macOS rows only, and only when the secrets exist)
  ├─ Set up signing keychain            temporary keychain, import, partition
  │                                     list, search list, decode the .p8
  │
  └─ Package (Unix) → scripts/package.sh --sign <TEAMID> --notary-*
       ├─ stage the plain binary
       │    → codesign the binary copy in the stage dir
       │    → ditto -c -k → temp zip → notarytool submit --wait
       │      (NOT stapled — §4.3)
       ├─ tar  dodo-vX-macos-arm64.tar.gz
       ├─ checksum
       │
       └─ if --app-bundle:
            scripts/macos-app-bundle.sh --sign <TEAMID> --notary-*
              ├─ assemble and codesign dodo.app
              ├─ ditto -c -k --keepParent → a TEMPORARY zip
              ├─ xcrun notarytool submit --wait   → assert `status: Accepted`
              ├─ xcrun stapler staple dodo.app
              ├─ rm the temporary zip
              └─ codesign --verify --deep --strict / spctl / stapler validate
          tar  dodo-vX-macos-arm64-app.tar.gz
          checksum

  Verify archives → scripts/verify-release.sh   (§6.2 step 6, on the extract)
  Remove signing credentials                    (if: always())
```

**One `notarytool` invocation is not the same as one round trip.** A macOS
matrix row makes two submissions: the plain binary and the `.app`. Each is a
`--wait` with no upper bound, which is why the `build` job now states an
explicit `timeout-minutes` (§4.4).

The temporary zip exists **only** to satisfy the notary service, which does not
accept a `.tar.gz` — it takes a zip, a UDIF disk image, or a flat installer
package (**READ**; `notarytool submit --help` says only "Path to the archive").
`ditto -c -k --keepParent` is the correct way to build it — **VERIFIED by
execution** on an ad-hoc bundle: the zip it produces has `dodo.app/` as its
root, which is what the notary service expects to find a bundle in. The
plain-binary form, `ditto -c -k <file> <zip>`, was run too and produces a
one-entry archive.

The *published* artifact stays `dodo-vX-macos-arm64-app.tar.gz`, because
`tools/update-manifest/src/platform.rs` selects the macOS entry **by exact
filename** and `models::manifest` asserts the URL ends in `-app.tar.gz`
(`AGENTS.md`, `docs/release.md`). **Renaming the artifact would break the
in-app updater; do not "simplify" the zip into being the release asset.** Both
temporary zips are written under `mktemp -d` and removed by the script, with a
`trap` so a failed submission does not leave one behind.

### 4.3 — The plain-binary archive can be signed but never stapled

`scripts/package.sh` publishes two macOS archives. The `.app` one can be
notarised and stapled. The plain-binary one can be signed and notarised, but
**not stapled**: **VERIFIED** from `man stapler` — *"stapler works only with
UDIF disk images, signed 'flat' installer packages, and certain code-signed
executable bundles such as '.app'."* A bare Mach-O is none of those.

The practical consequence is small: a notarised-but-unstapled binary is accepted
by Gatekeeper only when the machine can reach Apple to check online, and the
plain archive is the one a terminal user extracts and runs, where Gatekeeper's
quarantine path is already different. It is signed and notarised anyway — it
costs one extra `codesign` call and one extra submission, and it makes
`codesign -dvvv` on the shipped binary say something truthful.

`scripts/package.sh` does this on the **staged copy** in `dist/.stage/`, never
on `target/<triple>/release/dodo`: the `.app` bundle takes its own copy from the
build output and signs that inside the bundle, and signing the build output in
place would make a second run of the script sign an already signed binary. The
release notes say plainly that this archive is checked online rather than
stapled, so nobody reads a missing ticket as a broken release.

### 4.4 — Two knock-on effects, now live

- **A macOS release build depends on Apple's availability.** `--timestamp`
  contacts Apple's timestamp authority on every `codesign`; `notarytool`
  obviously does. `notarytool submit --wait` typically returns in minutes but
  has no upper bound. The `build` job previously had no `timeout-minutes` at
  all and so inherited GitHub's 6-hour default; it now states
  `timeout-minutes: 360` explicitly, with a comment saying why, because a
  ceiling that exists by accident is one somebody tightens to "how long a build
  takes" without thinking about the notary. **360 is the maximum a hosted job
  may ask for**, so this is the ceiling raised as far as it goes.
- **The `-app.tar.gz` has stopped being byte-reproducible**, and that is
  expected. A secure timestamp is *the current time from Apple's TSA* and a
  notarisation ticket is issued per submission, so two runs of the same tag
  differ in bytes even though `SOURCE_DATE_EPOCH` keeps everything else fixed.
  Recorded in `docs/build-optimization.md` ("Reproducibility") so the next
  person does not spend a day chasing it.

---

## 5. What signing buys, and what it does not

### The workaround that is still shipped, conditionally

dodo's generated release notes used to tell every user to run

```
xattr -dr com.apple.quarantine /Applications/dodo.app
```

They still do — but only for a build with no signing secrets, which is what a
fork produces. A signed run's notes describe the signature instead and give the
three commands a suspicious user can check it with.
`crates/dodo-updater/src/services/installers/macos.rs` still does the `xattr`
equivalent automatically for in-app updates, and deliberately keeps doing it:
removing an extended attribute cannot invalidate a signature or a stapled ticket
(§7.1, §7.2), and a user who obtained the archive some other way still benefits.

### What it does buy

| | Unsigned today | Signed + notarised + stapled |
|---|---|---|
| First launch of a downloaded `dodo.app` | *"…can't be opened because Apple cannot check it for malicious software"*; user must clear quarantine or right-click → Open | opens |
| App Translocation | a quarantined bundle runs from a randomised read-only path under `/private/var/folders/…/AppTranslocation/` (**INVESTIGATION** §8, verified) | does not happen |
| `spctl --assess --type execute` | `rejected` | `accepted` |
| TCC grants surviving an update | see below | see below |

**The TCC point is the one the investigation did not make, and it is the
strongest technical argument.** dodo has an in-app updater that replaces the
whole bundle (`crates/dodo-updater/src/services/installers/`), and a Cleaner that needs
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
| `--timestamp` omitted | notarisation rejected: *the signature does not include a secure timestamp* |
| `--options runtime` omitted | notarisation rejected: *the executable does not have the hardened runtime enabled* |
| `get-task-allow` entitlement present | notarisation rejected outright |
| A `.tar.gz` handed to `notarytool` | submission rejected as an unsupported archive format |
| `notarytool` reports `status: Invalid` but exits 0 | nothing at all — which is why both scripts `grep` the output for `status: Accepted` rather than trusting the exit status |
| Only some of the six secrets set | the keychain step fails naming the missing one, before anything is signed |
| `stapler staple` run before notarisation finished | *The staple and validate action failed! Error 65* |
| Anything re-signed after stapling | ticket silently invalidated; `stapler validate` fails on the shipped archive (**VERIFIED** from `man stapler`) |
| Archive built before signing | `codesign --verify` passes on `dist/dodo.app` and fails on the extracted copy; the published SHA-256 covers the unsigned bundle |
| `.p12` or `.p8` committed to the repo | revoke immediately; one of five certificate slots is burnt |

### 6.2 — The verification recipe

Run all of these; each catches something the others do not.

```bash
# 1. Structural + cryptographic validity. --deep remains correct for verification.
codesign --verify --deep --strict --verbose=2 dist/dodo.app

# 2. What was actually claimed: identity, Team ID, hardened runtime
#    (look for `flags=0x10000(runtime)`), and that entitlements are empty.
codesign -dvvv --entitlements - dist/dodo.app

# 3. The Gatekeeper policy check — the one that says `rejected` today.
spctl --assess --type execute --verbose=4 dist/dodo.app

# 4. The notarisation ticket is attached and matches.
xcrun stapler validate dist/dodo.app

# 5. THE ONE THAT MATTERS: all of the above, on what actually ships.
#    Steps 1–4 pass on dist/dodo.app trivially; the question is whether the
#    published archive still carries them.
mkdir -p /tmp/dodo-verify && tar -xzf dist/dodo-v*-macos-*-app.tar.gz -C /tmp/dodo-verify
codesign --verify --deep --strict --verbose=2 /tmp/dodo-verify/dodo.app
xcrun stapler validate /tmp/dodo-verify/dodo.app
spctl --assess --type execute --verbose=4 /tmp/dodo-verify/dodo.app
```

**Step 5 is now in `scripts/verify-release.sh`**, which the release workflow runs
on every archive it built, so a ticket that did not survive packaging fails the
release rather than the user. The guard is the bundle's own signature rather
than a flag — `codesign -dvv` is asked whether the authority is
`Developer ID Application`, and only then are `stapler validate` and `spctl`
required. An ad-hoc bundle prints "no notarisation ticket to check" and moves
on, which is **VERIFIED by execution**: a full ad-hoc `scripts/package.sh
--app-bundle` run followed by `verify-release.sh` on the resulting
`-app.tar.gz` passes and takes the skip branch. The Developer ID branch is
**UNEXERCISED**.

If notarisation is rejected, the log is the only thing that says why:

```bash
xcrun notarytool log <submission-id> --team-id "$TEAM_ID" \
    --key AuthKey_XXXXXXXXXX.p8 --key-id "$KEY_ID" --issuer "$ISSUER_ID"
```

It returns JSON with one entry per offending binary. It is genuinely good; do
not guess before reading it.

---

## 7. Compatibility audit — what is already in the tree

The question the readiness round asked: **would anything dodo builds have to be
undone to get to signing?** The answer was no, and turning signing on did not
change it — nothing in this section had to be revisited when the plumbing
landed. It is kept as the record of what was checked and why each answer holds.

### 7.1 — The `.tar.gz` distribution shape survives signing and stapling

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
   `crates/dodo-updater/src/services/installers/extract.rs` uses. Both `CodeResources` files
   came back byte-for-byte and the executable bit survived. Extended attributes
   did **not** survive, which is the correct outcome: nothing in a Developer ID
   signature lives in an extended attribute for a bundle.

**Conclusion: `-app.tar.gz` stays viable.** Neither the artifact name, the
manifest's exact-filename selection, nor the updater's `tar -xf` needs to change.
One caveat inherited from `tar`: keep the bundle free of symlinked framework
`Versions/` trees. dodo has no frameworks, so this is a constraint to preserve
rather than a problem to solve.

### 7.2 — The updater's `strip_quarantine` stays correct

`crates/dodo-updater/src/services/installers/macos.rs` runs
`xattr -dr com.apple.quarantine` over the extracted bundle before swapping it
in. Removing an extended attribute does not touch the code signature or the
stapled ticket (§7.1 part 1: neither lives in an xattr), so this stays correct
after signing — it just stops being necessary. Leave it: it is already
documented as best-effort and non-fatal, and a user who obtained the archive
some other way still benefits.

The module's doc comment used to say "dodo's binaries are **not** code-signed or
notarised". It now says what is actually true — official macOS builds are
signed and notarised, builds without the secrets are not, and `strip_quarantine`
is correct and harmless either way.

### 7.3 — Everything else checked, with nothing to change

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
  zipping it, which is the same constraint 1 as above. Nothing here changes it.

**No incompatibility was found, and none appeared when signing was turned on.**
The release keeps signing inside packaging, before the archive and checksum, and
keeps the workflow's temporary-keychain setup separate.

---

## 8. What is still owed

The honest list, so nothing here reads as finished when it is not.

- **A real release run.** Everything that touches the certificate or Apple is
  UNEXERCISED. The first tagged release with the secrets in place is the test,
  and it is the kind that either passes or produces a `notarytool log` URL. §6.1
  is the table to read it against.
- **`README.md` still says "Builds are **not** code-signed or notarised", and it
  is right until the first signed release exists.** Every archive a user can
  download today was built before this landed. Change that sentence — and the
  `xattr -dr com.apple.quarantine` line in the macOS install steps, which
  becomes an unnecessary no-op — as part of the release that first ships signed,
  not before.
- **The TCC claim in §5 is still INFERRED.** Whether a signed dodo keeps its
  Desktop/Documents/Downloads grants across a self-update has not been measured.
  It costs one update cycle to confirm and should not be quoted as settled until
  it is.
- **The `Info.plist` usage-description strings are still absent** (§3.2). They
  control the sentence a user reads in a TCC prompt, they are unrelated to
  signing, and adding them is a user-visible change that deserves its own round.
- **Windows signing is untouched.** `scripts/package.ps1` still zips an unsigned
  `.exe`. The constraint is identical — sign before the archive — and
  `docs/release.md`'s "Future readiness" is where that work is described.
- **`update.json`'s `signature` field is still `null`.** A different mechanism
  solving a different problem; Developer ID signing did not address it and must
  not be read as having done so.
