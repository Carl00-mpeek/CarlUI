# Security policy

> **Türkçe özet:** Bir güvenlik açığı bulduysan lütfen herkese açık bir issue
> açma. GitHub'daki **Security → Report a vulnerability** düğmesiyle gizli
> bildir ya da maildemhat@gmail.com adresine yaz. 3 gün içinde yanıt, en geç
> 30 gün içinde düzeltme ya da plan veririz. Aşağıda neyi koruduğumuz ve neyi
> korumadığımız (tehdit modeli) anlatılıyor.

## Supported versions

Only the latest release gets security fixes. CarlUI updates itself (see
README → Updates), so a fix reaches users through the normal update.

## Reporting a vulnerability

Please **don't open a public issue** for a security problem.

- Preferred: GitHub → **Security** tab → **Report a vulnerability** (private).
- Or email **maildemhat@gmail.com** with "CarlUI security" in the subject.

Include the CarlUI version, Windows build, steps to reproduce and what an
attacker gains. A proof of concept helps but isn't required.

What to expect: a reply within 3 days, and a fix or a plan within 30 days.
You'll be credited in the release notes unless you'd rather not be. Please
give us a chance to ship a fix before you publish details.

## Threat model

CarlUI is a per-user desktop shell. It runs without admin rights, has no
server, and keeps all data on the PC. What follows is what it tries to
protect and what it explicitly doesn't.

### In scope

**Carl Vault at rest.** Someone who copies `%APPDATA%\CarlUI\vault\` (stolen
laptop, backup, cloud sync of the profile) must not be able to read entries
without the master password.

- Key derived with Argon2id (64 MiB, 3 passes); entries sealed with
  XChaCha20-Poly1305. KDF parameters are stored in the header and bounded on
  read: lowered parameters (a downgrade) and absurdly high ones (a
  denial-of-service) are both rejected.
- A tampered file (entry ids, KDF parameters, version, truncation) is
  rejected rather than partially read.
- `.carlvault` backups use the same construction with their own password.
- Wrong-password attempts are throttled (5 s up to 5 min) and the counter is
  stored on disk, so closing the window doesn't reset it. This slows someone
  at the keyboard; offline guessing is limited by Argon2's cost only. **A
  weak master password is the main risk.**

**Carl Vault while unlocked.** Entries are decrypted only when needed,
secrets are zeroized after use, and the vault locks on idle, sleep and
Windows lock. Copying a field puts a single-use, 60-second token on the
clipboard instead of the value; the clipboard history never stores it.

**The local bus between CarlUI processes.** Named pipes are created for the
current user only, reject remote clients, and both ends check that the other
process runs from the same install folder in the same session. Lines are
size-limited.

**The browser extension channel** (`127.0.0.1:47321`). Vault requests need a
pairing token that the user approves once per extension. Passkeys are only
offered when the page's origin is HTTPS (or `localhost`) and the requested
relying-party id is the page's host or a registrable parent of it (WebAuthn
§5.1.3); public suffixes are refused.

**Untrusted input.** Import files (CSV/JSON from other password managers and
2FA apps), `otpauth://` and Google Authenticator migration URIs, QR codes in
screenshots and backup files are attacker-controllable. They must not crash
or hang CarlUI or the vault. These parsers are covered by mutation tests that
run in every `cargo test` and by libFuzzer targets; see
[fuzz/BENIOKU.md](fuzz/BENIOKU.md).

**Updates.** The updater downloads only from this repository's GitHub
releases and checks the installer's SHA-256 against the release before
running it.

### Out of scope

- **Malware already running as your Windows user (or as admin).** It can read
  CarlUI's memory while the vault is unlocked, log keystrokes, read the
  screen, or talk to the local extension port. No user-mode app can fully
  defend against this; CarlUI only reduces exposure (short-lived decryption,
  auto-lock, clipboard tokens).
- Someone with physical access to an **unlocked** session.
- Other Windows users with admin rights on the same PC.
- The security of the sites and apps whose passwords you store.
- Weak master passwords, and unencrypted CSV/JSON exports you chose to make
  (CarlUI warns before writing them).
- The optional third-party components: the PawnIO driver, HWiNFO shared
  memory, and AI models (downloaded from their official source and verified
  by SHA-256, but their contents are their authors').

### Known limits

- The installer isn't code-signed yet (SignPath application in progress), so
  you can't verify it with Authenticode. Compare its SHA-256 with the one on
  the Releases page.
- The browser extension is loaded unpacked, so the browser doesn't update it
  automatically; it updates with CarlUI.

## Crash reports

When a CarlUI part crashes, a report is written to
`%LOCALAPPDATA%\CarlUI\logs\crash-*.txt`. It stays on the PC and is never
sent anywhere. Your user folder is replaced with `%USERPROFILE%`, but the
log lines can still mention file or window names, so read it before you
paste it into a public issue.
