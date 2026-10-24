# kelpie-hub

A hub that lets you follow and steer [kelpie](https://github.com/gronare/kelpie) from your phone. Each machine running kelpie dials out to the hub; your devices read every machine's board, get notified when a task needs you, and answer questions, approve pushes or start tasks.

The hub is a mailbox, never a party. Everything it carries is end-to-end encrypted between a machine and the devices that machine has paired with, and every command is signed by a device the machine trusts. The hub sees who talks to whom and when, not what is said, and it cannot start anything on a machine.

This repository holds:

- `kelpie-hub-protocol`: envelopes, messages and pairing, error reports, error signals and their proof of work, plus the test vectors other implementations are checked against.
- `kelpie-hub`: the hub server. It relays sealed boards from machines to the devices they paired with, signed commands from devices to machines and sealed results back, pushes sealed notices to devices through Web Push, and carries pairing between them. It also takes error reports and error signals from kelpie installs for kelpie's maintainers.
- `kelpie-hub-api`: the request and response bodies of the hub's HTTP API.
- `kelpie-hub-fakes`: `fake-machine` and `fake-phone`, two CLIs that stand in for kelpie and the web app. The fake machine never runs anything: it prints what kelpie would do.

## The protocol

- **Envelope.** A plaintext header (`v`, `id`, `kind`, `from`, `machine`, `at`, `exp`, `schema`, `kc`, and for notices `urgency` and `collapse`), one key row per recipient, the ciphertext and a signature. Each kind has a maximum lifetime (commands 24 hours, notices 1 day, results 7 days, boards 30 days); a header with a longer one is refused.
- **Content.** AES-256-GCM with a fresh 32-byte key and 96-bit nonce per envelope; the header's canonical bytes are the associated data. `kc` is SHA-256 of the content key, so a recipient handed a different key detects it before decrypting.
- **Key rows.** The content key is sealed to each recipient device or machine with HPKE (RFC 9180) base mode, `DHKEM(X25519, HKDF-SHA256)`, `HKDF-SHA256`, `AES-128-GCM`, with the header bytes as associated data. A row copied under another recipient's id does not open, because it is sealed to the first recipient's key.
- **Signature.** Ed25519 over the header bytes followed by the ciphertext. Key rows are outside it, so the hub can hand each device only its own row.
- **Commands.** `authorize` drops, without any reply, anything that fails before the signature is verified: wrong version or shape, not a command, another machine, an unknown device, a bad signature. A validly signed command whose id the machine already applied for that device gets the stored result again, even once it has expired. Otherwise the machine applies it only if it is unexpired, from a device with the `control` role, of a kind the machine accepts and well formed, and answers a refusal with a reason the device can show.

The message types (board snapshot, notice, answer, push approval, start task, run routine, result, alert) are in `crates/kelpie-hub-protocol/src`. A board carries only allow-listed fields: no local paths, hostnames, pane ids or terminal titles.

## Pairing

Pairing a device with a machine binds both parties' keys without trusting the hub that relays them. The device commits before it learns anything from the machine, so the hub cannot try substitute keys until the two codes match.

1. The device draws a 32-byte nonce `n_d` (from `crypto.getRandomValues`; `pairingNonce()` in `js/protocol.js`) and sends `commitment = SHA-256("kelpie-pair-commit-v1" || D || n_d || code)`, where `D` is its signing key followed by its encryption key and `code` is the one-time code from the hub.
2. The machine draws a fresh 32-byte nonce `n_m` and replies with its keys `M` and `n_m`.
3. The device reveals `D` and `n_d`. The machine refuses the pairing if they do not hash, together with the code the machine holds, to the commitment, so a machine holding a different code fails here before any digits are shown.
4. Both sides show `SHA-256("kelpie-pair-v1" || M || D || n_m || n_d || code)` as six digits. The user confirms that the digits match before the machine adds the device to its trusted list.

The flow holds only if each side takes one chance per attempt:

- One pairing invocation (`kelpie hub pair` on the machine, "Pair" in the app) accepts exactly one commitment.
- A commitment mismatch, a second commitment or a timeout ends the invocation with a visible failure. The user starts again; nothing retries quietly.
- The device shows every attempt, including failed ones, so an attempt the user did not start is visible.

A hub that swaps keys has to fix its substitutes before it sees `n_d`, so it matches the codes only by a one-in-a-million guess. `DevicePairing` and `MachinePairing` in the crate, and `pairingCommitment` and `machineComparisonCode` in `js/protocol.js`, carry the two sides.

## Connecting a machine

`kelpie hub connect` connects a machine to a hub in one step, in the shape of OAuth's device authorization grant (RFC 8628). Nobody needs a join code or a shell on the hub.

1. The machine posts its public keys and a name to `POST /v1/m/connect` and gets a `device_code` (its secret for polling), a `user_code` like `K7QF-M2XD`, `verification_uri` and `verification_uri_complete` (`<origin>/connect?code=K7QF-M2XD`), `interval` and `expires_in`.
2. The machine draws a 32-byte secret of its own and opens `verification_uri_complete#k=<secret>&m=<fingerprint>` in the browser, where the fingerprint is `SHA-256("kelpie-connect-machine-v1" || M)` over its signing and encryption keys. A browser never sends the part after `#` to the hub. `ConnectSecret` in the protocol crate and `parseConnectFragment` and `machineFingerprint` in `js/protocol.js` carry it.
3. The page shows the machine's name and the user code. The person checks the code against the terminal, signs in with a passkey (or signs up) and presses Connect. Connect needs a session that a passkey signed in within the last 5 minutes (`401 reauth-required` otherwise, and the page asks for the passkey again) and the hub's `Origin`. The machine belongs to the account that approved it. Cancel denies it.
4. The machine polls `POST /v1/m/connect/token` every `interval` seconds and gets its machine id and a `khm_` token, the same token a join code gives.

**Also show its board in this browser.** With the box ticked, the browser registers itself as a device of the account and sends, with the approval, the commitment of the pairing protocol with the machine's secret as the code: `SHA-256("kelpie-pair-commit-v1" || D || n_d || secret)`. The hub opens a pairing that starts committed, and the token answer names it. The machine replies `M` and `n_m` as in any pairing; the browser checks that `M` hashes to the fingerprint in the fragment before it reveals, and the machine checks the reveal against its secret before it admits the browser. No six digits are compared, because nothing is left for them to catch: the user code on both screens tells the person they approve the machine in front of them, and the secret and the fingerprint bind the keys, both carried past the hub in the fragment. A hub that commits for a device of its own fails the machine's check, since it never saw the secret, and one that hands the browser other machine keys fails the browser's. The user code alone could not bind them: the hub mints it, so a hub in the middle would know it. A browser that reached the page by a typed code has no fragment; it connects the machine but offers no board, and pairs later like any other device.

The codes:

- A user code is 8 symbols from `0`-`9` and `A`-`Z` without `I`, `L`, `O` and `U`, so it reads aloud and survives a mistyped `O` or `I`. Both codes are valid 10 minutes and once, and the hub keeps only their SHA-256 hashes.
- The token endpoint answers `400` with RFC 8628's error names: `authorization_pending`, `slow_down` (polled sooner than `interval`; the interval grows by 5 seconds), `access_denied` (once; the code is gone after that), `expired_token`, and `invalid_grant` for a code it does not know or already redeemed.
- Starting a connect is limited per address (10 at once, 10 a minute), looking up and approving codes per account (10 at once, 10 a minute).
- An approval that is never redeemed leaves nothing: the machine it named is deleted with the expired code.

`kelpie-hub admin join-code` keeps working for scripts; a machine that joined with one belongs to no account.

## Adding a phone from the machine

`POST /v1/m/devices/add` opens a pairing as `POST /v1/m/pairings` does and returns `{pairing, code, url, expires_at}`, where `url` is `<origin>/add?code=<code>`. kelpie shows the link and a QR code of it. The phone opens it, signs in with a passkey or signs up (sign-up brings it back to the link), reads "Add this phone to <machine>?" and taps Add. From there it is the pairing above: the phone commits, the machine replies, the phone reveals, and both show six digits. The person answers `y` in the terminal and taps Same on the phone; the machine admits only on its own `y`, because the phone's tap goes through the hub.

The digits stay here because the phone is not where the code was shown: anyone who read the link over a shoulder or photographed the QR code could commit first. The digits come from the reveal, so the machine learns them from `GET /v1/m/pairings/<id>` like any pairing's, not from the call that opens it. A machine that belongs to an account takes devices only from that account (`403 not-your-machine`, at `GET /v1/a/add` and at the commitment).

## Plans

The hub knows whether each account's plan is `active` or `inactive`, with an optional `until`; subscriptions themselves live in a separate billing service. `--plans off` (the default, for a hub you run for yourself) makes every account entitled. With `--plans on`:

- approving a connect, and every call a machine makes to post or read (board, notice, inbox, results, pairings, adding a device), needs an entitled plan on the account the machine belongs to: `active` with no `until` or one still ahead. Otherwise the answer is `402` with `{"error": "plan-inactive", "url": <--billing-url>}`, for kelpie to show "Start a subscription at <url>".
- signing in, the account page and removing devices keep working. `GET /v1/a/account` carries the plan and, when it is not entitled, `billing_url`.
- the owner is always entitled, and machines that joined with a join code (no account) are not gated.
- an account created after the upgrade to schema 7 starts `inactive` until the billing service sets it; accounts carried over from an older schema start `active`.

The billing service uses an admin token with the `billing` scope (`kelpie-hub admin token --name billing --scope billing`): `GET /v1/admin/accounts?email=<address>` finds an account by its verified email, and `PUT /v1/admin/accounts/<id>/plan` with `{status, until?}` sets the plan. Both answer `{account, handle, email, plan: {status, until, entitled}}`. A token of the other scope gets `403 wrong-scope`.

## Running a hub locally

```sh
cargo build --workspace
target/debug/kelpie-hub --dev
```

The hub links OpenSSL (through `webauthn-rs`) built from source and statically, so the binary needs no system OpenSSL; building it needs a C compiler, `make` and `perl`.

`--dev` listens on `127.0.0.1:8470`, keeps its sqlite file in `kelpie-hub-dev` under the system temp directory, uses `localhost` as the passkey relying party with `http://localhost:8470` as the origin, signs push requests with this repository's URL as the VAPID subject, and lets a pairing code create a device without an account so the fake phone can pair. `--data <dir>`, `--listen <address>`, `--rp-id <domain>`, `--origin <url>` and `--vapid-subject <contact>` override these; without `--dev` all five are required, and with `--dev` the hub refuses to listen on anything but a loopback address.

The VAPID subject is the contact every push service sees in the hub's push requests: `mailto:you@your-domain` or the hub's `https:` URL. The hub refuses to start with anything else, including a `localhost`, `.local`, `.test`, `.internal` or IP address, because Apple answers such tokens with `403 BadJwtToken`. The hub's VAPID key pair is made on first start and kept in `vapid.key` in the data directory (mode 0600, the P-256 private scalar as base64url); `--vapid-key <file>` keeps it elsewhere. The hub refuses to start when the key file is readable by group or others. The push services are reached over TLS with the platform's root certificates, so a container image needs its CA bundle (`ca-certificates` on Debian or Alpine) for real pushes. Keep the file across restarts and moves: a new key invalidates every device's push subscription, and the web app has to subscribe again. `--push-allow-host <host>` (repeatable, `*.example.com` for subdomains) accepts push endpoints on a host beyond the known push services, for a self-hosted push service or tests; on a loopback host it may be plain `http:`. Behind Cloudflare, `--trusted-proxy cloudflare` takes the client address from `CF-Connecting-IP`; without it the hub uses the TCP peer and ignores that header.

A public hub:

```sh
kelpie-hub --data /var/lib/kelpie-hub --listen 0.0.0.0:8470 \
  --rp-id hub.example.com --origin https://hub.example.com --trusted-proxy cloudflare \
  --vapid-subject mailto:you@example.com
kelpie-hub --data /var/lib/kelpie-hub admin invite
```

On its first start with no accounts, the hub prints a one-time setup link (`<origin>/setup?token=kho_…`) to its log; whoever opens it picks a handle and a passkey and becomes the hub's owner. Without the link nobody can claim a fresh hub. Each start mints a new link while no account exists, and the previous one stops working.

`--signup invite` (the default) lets people in by invite only: from `admin invite`, or as a link the owner makes in the admin page. `--signup open` lets anyone sign up who verifies an email address by a link the hub mails; it needs `--smtp-host <host>` and `--mail-from <address>` (`--smtp-port`, 587 by default; `--smtp-security starttls`, `tls` or `none`, the last only for a loopback host; `--smtp-user <user>` with the password in `KELPIE_HUB_SMTP_PASSWORD`). Without `--smtp-host`, and always with `--dev`, the hub prints each mail to its log instead of sending it. `--plans on` and `--billing-url <url>` turn on [plans](#plans).

`GET /healthz` answers `204` without credentials, for a load balancer's or Kubernetes' probes. `SIGTERM` and `Ctrl-C` both stop the hub, giving open requests 5 seconds to finish.

Error reports and signals arrive at `/v1/reports` and `/v1/signals` without credentials, so in front of a public hub a WAF rate-limit rule on those paths (Cloudflare's rate limiting rules, for one) is recommended on top of the hub's own limits. `--report-difficulty <bits>` sets the proof of work for both (20 by default, about a second on a laptop).

Upgrading a live hub from schema 6 to 7 (the release that adds connecting, owners, sign-up by email and plans) goes the same way as below, with the backup named `hub-v6.sqlite3`; the new binary adds the connect table and the account, machine, invite and admin token columns at start-up and keeps everything there. Existing accounts are members with an `active` plan, existing admin tokens have the `reports` scope, and existing machines belong to no account. A hub that already has accounts prints no setup link; make one of them the owner with `sqlite3 hub.sqlite3 "UPDATE accounts SET role = 'owner' WHERE handle = '<handle>'"` while the hub is stopped.

Upgrading a live hub from schema 5 to 6 (the release that adds error signals) goes the same way as below, with the backup named `hub-v5.sqlite3`; the new binary adds the signal tables at start-up and keeps every report, key standing and token. A schema 4 database gains both releases' tables at once.

Upgrading a live hub from schema 4 to 5 (the release that adds error reports): stop the hub, back up its database with `sqlite3 hub.sqlite3 ".backup hub-v4.sqlite3"` in the data directory, then start the new binary, which adds the report tables at start-up. To roll back, stop it, restore `hub-v4.sqlite3` as `hub.sqlite3` and start the old binary. Never run the new binary's `admin` commands against a database an old hub still serves: they upgrade it under the running hub.

`admin invite` mints a one-time sign-up code, valid 7 days. `admin machines` lists the machines that joined, and `admin remove-machine <id>` removes one with its board, queue and admissions; its token stops working at once.

In a second terminal, mint a join code and join a machine:

```sh
target/debug/kelpie-hub admin join-code --dev
target/debug/fake-machine --state machine.json join http://127.0.0.1:8470 <join code> --name mac
```

Pair a phone. The machine prints a pairing code and waits; the phone takes the code, and both print six digits. Answer `y` on the machine only when they match:

```sh
target/debug/fake-machine --state machine.json pair
target/debug/fake-phone --state phone.json init http://127.0.0.1:8470
target/debug/fake-phone --state phone.json pair <pairing code>
```

Post a board and read it on the phone:

```sh
target/debug/fake-machine --state machine.json post-board examples/board.json
target/debug/fake-phone --state phone.json boards
```

Send a command from the phone and apply it on the machine. The phone waits for the result and prints it; `push` approves the head the phone sees on the board unless `--head` names another:

```sh
target/debug/fake-machine --state machine.json apply
target/debug/fake-phone --state phone.json answer refunds@greenhouse 1761069540-4411-0-question --choice 1
target/debug/fake-phone --state phone.json push refunds@greenhouse
target/debug/fake-phone --state phone.json start greenhouse invoice-rounding "Fix the rounding in invoices"
target/debug/fake-phone --state phone.json routine nightly-deps
```

Post a notice. The machine seals it to every device it paired with; the hub pushes each subscribed device its own cut of the envelope:

```sh
target/debug/fake-phone --state phone.json push-subscribe --endpoint <push endpoint> --keys
target/debug/fake-machine --state machine.json notice "Keep the old endpoint for a release?" \
  --event question --urgency high --collapse refunds@greenhouse
target/debug/fake-machine --state machine.json notice "answered" --collapse refunds@greenhouse --retract
```

`push-subscribe` makes fresh subscription keys, keeps the private half in the state file and registers the endpoint; `--keys` prints the subscription it sent. A command-line phone has no push service of its own, so the endpoint is one the hub accepts (a known push service, or a host given with `--push-allow-host`). `--collapse` takes the task's `task@repo`: the fake machine turns it into an opaque tag (a SHA-256 over its own key and `event:address`; kelpie will use an HMAC key shared at pairing), so the next notice for that task replaces this one. `--retract` sends a notice whose `retract` names that tag, for the app to close the earlier one.

`fake-machine apply` runs until stopped (`--once` polls once); it keeps the ids it applied, with their results, in its state file. `fake-machine role <device> read` makes a device read-only, so its commands are refused with `not-allowed`. `fake-machine poll` long-polls the machine's queue once, and `fake-phone attempts` lists every pairing attempt, failed ones included. Both CLIs keep their keys and tokens in the `--state` file, written with mode 0600. Against a hub without `--dev`, give the phone a device an account registered: `fake-phone init <hub> --device <id> --token <token>`.

## Accounts and who holds what

People sign in to the hub with its own accounts. An account holds passkeys (WebAuthn, user verification required) and the devices it registered; it never sees a board, because boards are sealed to device keys the account does not have.

- **Sign-up** takes a handle (lowercase letters, digits and `-`, starting with a letter) and one ticket, then registers the first passkey: the setup token of a fresh hub (`setup`, which makes the owner and works only while there is no account), an invite (`invite`, from `kelpie-hub admin invite` or an invite link), or on an open hub an email verification token (`verification`, from the mailed link; the account keeps the address as verified). `POST /v1/a/signup/email` mails the link and answers `202` whether or not the address already has an account, sending nothing then; it is limited per address and per email (3 at once, 1 a minute). The ticket is spent when the passkey is accepted, so two ceremonies started with one make one account. The account table has room for a GitHub id, for sign-up through GitHub later.
- **The owner** is the account made from the setup link. Only the owner opens `/admin` and the `/v1/a/admin/` endpoints (`403 not-owner` for anyone else): counts of accounts, machines, devices, reports and signals; accounts with their plan, machines, devices and when they were last seen; invite links; deactivating and activating an account; removing a machine; allowing, blocking and acknowledging error reports; error signal counts. Deactivating an account, removing a machine and blocking a report key need a session a passkey signed in within 5 minutes (`401 reauth-required`, and the page asks for the passkey again); reading, inviting, activating, allowing and acknowledging do not. A deactivated account's sessions are deleted, it cannot log in, and its devices' and machines' tokens are refused until it is activated again. The owner cannot be deactivated (`409 owner-stays`). Safari refuses `Secure` and `__Host-` cookies on `http://localhost`, so a `--dev` hub signs in from Chrome or Firefox, or from Safari behind a local TLS name.
- **Logging in** takes the handle and a passkey the account holds. Handles are not secret: logging in tells whether one exists. The hub keeps each passkey's signature counter and refuses a login whose counter does not move past it (unless both are zero), as a sign the passkey was cloned.
- **The session** is an opaque `khs_` token in the cookie `__Host-kelpie_session` with `HttpOnly`, `Secure`, `SameSite=Strict` and `Path=/`, valid 14 days. The hub stores its SHA-256 hash. Logging out deletes it.
- **Cross-site requests.** Every state-changing `/v1/a/` call needs an `Origin` header equal to `--origin`, or gets `403 cross-origin`. The cookie is accepted on `/v1/a/` only.
- **Devices.** A logged-in account registers a device and gets its `khd_` token once. Every `/v1/d/` call, the event feed included (read with `fetch`, since `EventSource` cannot send a bearer token), takes that token as a bearer token and never the cookie, so no device call rides on an ambient credential and none needs a CSRF check. The web app keeps the token next to its non-extractable device keys in IndexedDB, and uses its session only to register a device or manage the account. Logging out leaves the device working; removing the device ends it.
- **Removal.** A machine removes a device it admitted, and the hub stops handing that device the machine's board. An account removes its own devices, which deletes their tokens and ends their event feeds. An admin removes a machine.

Ceremony state (the challenge a passkey must sign) is kept on the hub for 5 minutes and used once; it never reaches the browser.

## The hub's API

Every endpoint is under `/v1`. Admin endpoints (`/v1/admin/`) take a `kha_` bearer token from `kelpie-hub admin token --name <name>`, with the `reports` scope (the default) for the report endpoints and the `billing` scope (`--scope billing`) for the account endpoints. Machine endpoints (`/v1/m/`) take a `khm_` bearer token, except joining and connecting, where the join code or device code is the credential. Account endpoints marked *owner* need the owner's session, and *fresh* a session a passkey signed in within 5 minutes. Device endpoints (`/v1/d/`) take a `khd_` bearer token. Account endpoints (`/v1/a/`) take the session cookie, except signing up and logging in.

| Call | Who | What |
|---|---|---|
| `GET /v1/hub` | anyone | `{signup: "invite"\|"open", plans, needs_setup}` |
| `POST /v1/m/join` | machine, join code | trades a code from `kelpie-hub admin join-code` and the machine's public keys for a machine id and token |
| `POST /v1/m/connect` | anyone | `{name, keys}`; `{device_code, user_code, verification_uri, verification_uri_complete, interval, expires_in}`. See [Connecting a machine](#connecting-a-machine) |
| `POST /v1/m/connect/token` | machine, device code | `{device_code}`; `{machine, token, pairing}` once approved, `pairing` naming the browser's pairing or `null`; otherwise `400` with `authorization_pending`, `slow_down`, `access_denied`, `expired_token` or `invalid_grant` |
| `POST /v1/m/devices/add` | machine | `{pairing, code, url, expires_at}`: a pairing, valid 10 minutes, with the link a phone opens. See [Adding a phone](#adding-a-phone-from-the-machine) |
| `PUT /v1/m/name` | machine | `{name}`: renames the calling machine, with the same check as joining (`400 bad-name`); returns `{name}` as stored. Paired phones show the new name on their next read |
| `PUT /v1/m/board` | machine | `{rev, envelope}`: a board sealed by the machine; the hub checks the machine's signature, then orders boards by the envelope's signed `at` with `rev` breaking ties within one second. It refuses the envelope it already holds or one signed earlier (`409 replayed-envelope`) and, at the same `at`, a `rev` at or below the one it holds (`409 stale-revision`) |
| `GET /v1/m/inbox?after=<seq>&wait=<s>` | machine | waits up to 25 s for queued envelopes with `seq > after`, plus the `board_rev` the hub holds; `after` acknowledges, and the hub deletes what it covers. Handing a command over marks it delivered |
| `POST /v1/m/results` | machine | `{envelope}`: a result sealed to the device that sent the command, with the command's id as its own. `400` for a bad signature or recipients, `404 unknown-command` for a command this machine was not sent by that device (or one the hub no longer holds; the machine drops that result and moves on), `409 not-delivered` before the inbox handed it over. Posting the same envelope again returns the same `seq`; a different result for a command that has one is `409 result-differs` |
| `POST /v1/m/pairings` | machine | opens one pairing invocation and returns its code, valid 10 minutes |
| `GET /v1/m/pairings/<id>?seen=<state>&wait=<s>` | machine | waits until the pairing leaves `seen`; shows the device's commitment, then its revealed keys and nonce |
| `POST /v1/m/pairings/<id>/reply` | machine | `M` and `n_m`, once, after the commitment |
| `POST /v1/m/pairings/<id>/admit` | machine | after the user confirmed the digits; the hub records which machine admitted the device |
| `POST /v1/m/pairings/<id>/refuse` | machine | `mismatch`, `declined` or `timeout` |
| `POST /v1/m/notices` | machine | `{envelope}`: a notice sealed to the machine's devices; `202` with `{notice, pushes}`, the number of pushes queued. See [Notices and Web Push](#notices-and-web-push) |
| `DELETE /v1/m/devices/<id>` | machine | stops handing the machine's board to a device it admitted; `404` for a device it did not admit |
| `POST /v1/d/pairings` | device, pairing code | the commitment. The same token sending the same commitment again gets the same answer; any other second commitment fails the pairing. An unknown code and a code whose pairing has ended both get `401`. Only a `--dev` hub accepts a commitment without a token, and hands out a device id and token with it |
| `GET /v1/d/pairings/<id>?seen=<state>&wait=<s>` | device | waits like the machine's; shows `M` and `n_m`, then the outcome |
| `POST /v1/d/pairings/<id>/reveal` | device | `D` and `n_d`, once, after the reply |
| `POST /v1/d/commands` | device | `{envelope}`: a signed, sealed command; `202` with `{command, seq}`, its place in the machine's queue. Sending the same envelope again returns the same `seq` and queues nothing; another envelope under an id already used is `409 command-id-taken` |
| `GET /v1/d/results?after=<seq>&wait=<s>` | device | waits up to 25 s for results to this device with `seq > after`, each with only this device's key row. `seq` counts this device's results only. `after` is a cursor, not an acknowledgement: results stay until they expire |
| `PUT /v1/d/push` | device | the browser's push subscription, as `PushSubscription.toJSON()` gives it: `{endpoint, keys: {p256dh, auth}}`. `204`; it replaces the device's earlier one. An endpoint another device of the same account held moves to this one (logged `push.moved`); one held by another account's device is `409 endpoint-taken`. `400 bad-endpoint`, `endpoint-not-https`, `endpoint-not-allowed` (a host that is not a known push service) or `bad-push-keys` |
| `DELETE /v1/d/push` | device | removes the device's push subscription; `204` whether or not it had one |
| `GET /v1/push/vapid` | anyone | `{key}`: the hub's VAPID public key, base64url of the uncompressed P-256 point, for `pushManager.subscribe({applicationServerKey})` |
| `GET /v1/d/boards` | device | the latest board of every machine that admitted the device, with only this device's key row |
| `GET /v1/d/events` | device | a server-sent event stream: `board` (`{machine, rev}`) when an admitting machine stores a board, `pairing` (`{pairing, state}`) when one of the device's pairings changes or expires, `machine-removed` (`{machine}`) when a machine removes the device, `result` (`{machine, command}`, with the result's `seq` as the event id) when a result to the device is stored. It starts with the current state: a new feed without `Last-Event-ID` announces every result the hub still holds for the device (at most 7 days back), and with it only those after that `seq` (a `Last-Event-ID` that is not a number up to `i64::MAX` gets `400 bad-last-event-id`). It carries no content, keeps alive every 15 s and ends when the device is removed |
| `POST /v1/a/register/begin` | anyone, ticket | `{handle}` with one of `invite`, `setup` or `verification`; returns a ceremony id and the passkey creation options |
| `POST /v1/a/signup/email` | anyone, open hub | `{email}`; `202`, and the hub mails a sign-up link. `403 signup-closed` on an invite hub, `400 bad-email`, `502 mail-failed` |
| `GET /v1/a/signup/verify?token=<t>` | anyone | `{email}` the link verifies, or `401` |
| `POST /v1/a/register/finish` | anyone | `{ceremony, credential, name?}`; creates the account, spends the invite and sets the session cookie |
| `POST /v1/a/login/begin` | anyone | `{handle}`; returns a ceremony id and the passkey request options |
| `POST /v1/a/login/finish` | anyone | `{ceremony, credential}`; sets the session cookie |
| `POST /v1/a/logout` | account | deletes the session and clears the cookie |
| `GET /v1/a/account` | account | `{account, handle, created_at, owner, email, plan: {status, until, entitled}, billing_url}`, `billing_url` only while the plan is not entitled |
| `GET /v1/a/connect?code=<user code>` | account | `{code, name, state, expires_at}` of a pending connect; `404 unknown-code` for one that expired, was decided or does not exist |
| `POST /v1/a/connect/approve` | account, fresh | `{code, device?, commitment?}`; `{machine, name, pairing}`. `device` (one of the account's) and `commitment` together open the browser's pairing. `401 reauth-required` for a session older than 5 minutes, `402 plan-inactive`, `404 unknown-code` |
| `POST /v1/a/connect/deny` | account | `{code}`; `204`, and the machine gets `access_denied` |
| `GET /v1/a/add?code=<code>` | account | `{machine, name, expires_at}` of an open pairing; `403 not-your-machine` when the machine belongs to another account |
| `GET /v1/a/admin/overview` | owner | `{accounts, machines, devices, reports, quarantined, signals_this_week}` |
| `GET /v1/a/admin/accounts` | owner | each account's `{account, handle, email, owner, created_at, deactivated_at, plan, machines, devices, last_seen}` |
| `POST /v1/a/admin/accounts/<id>/deactivate`, `/activate` | owner (deactivate: fresh) | `204`; `409 owner-stays` for the owner |
| `POST /v1/a/admin/invites` | owner | `{code, url, expires_at}`: an invite and its `<origin>/signup?invite=<code>` link, valid 7 days |
| `GET /v1/a/admin/machines` | owner | each machine's `{machine, name, owner, joined_at, last_seen, devices}` |
| `DELETE /v1/a/admin/machines/<id>` | owner, fresh | removes the machine as `admin remove-machine` does |
| `GET /v1/a/admin/reports?quarantine=<bool>` | owner | up to 100 allowed unacknowledged reports, or the ones in quarantine |
| `POST /v1/a/admin/reports/<id>/ack`, `/v1/a/admin/reports/keys/<key>/allow`, `/block` | owner (block: fresh) | as `admin reports ack`, `allow` and `block` |
| `GET /v1/a/admin/signals?days=<n>` | owner | the error signal counts of the last `n` days (7 by default) |
| `GET /v1/a/passkeys` | account | the account's passkeys: id, name, created, last used |
| `POST /v1/a/passkeys/begin`, `/finish` | account | adds a passkey, excluding the ones the account holds |
| `DELETE /v1/a/passkeys/<id>` | account | removes a passkey; the last one stays (`409 last-passkey`) |
| `POST /v1/a/devices` | account | `{name?}`; registers a device and returns its id and token, once |
| `GET /v1/a/devices` | account | the account's devices with the machines that admitted each |
| `DELETE /v1/a/devices/<id>` | account | removes the device, its token and its admissions |
| `GET /v1/reports/challenge` | anyone | `{nonce, difficulty, expires, tag}`: a proof-of-work challenge for one report. See [Error reports](#error-reports) |
| `POST /v1/reports` | anyone | `{report, key, signature, challenge, counter}`; `202` with `{report}`, its id |
| `GET /v1/admin/reports?after=<id>` | admin token | `{reports, quarantined}`: up to 100 allowed, unacknowledged reports with `id > after`, each with its key and arrival time, and how many wait in quarantine |
| `POST /v1/signals` | anyone | `{signals, key, signature, challenge, counter}`; `202` with `{rows}`, the number of counts in it. See [Error signals](#error-signals) |
| `POST /v1/admin/reports/<id>/ack` | admin token | marks an allowed report handled; `204`, or `404` for one that is not allowed or does not exist |
| `GET /v1/admin/accounts?email=<address>` | billing token | `{account, handle, email, plan}` of the account with that verified email, or `404` |
| `PUT /v1/admin/accounts/<id>/plan` | billing token | `{status: "active"\|"inactive", until?}`; the account's plan, as above. See [Plans](#plans) |

## Error reports

kelpie keeps the problems it meets as local issues. When kelpie or its lead files one of kind `bug`, the board shows it with the exact text that would go out, and asks the user whether to send it to kelpie's maintainers; only the user's yes sends it. The report is plain text, not sealed: it is meant for the people who run the hub.

- **What a report holds.** `title`, `text`, `version` and `os` (`BugReport` in the protocol crate). kelpie redacts paths under the home directory to `~`, repository names, remote URLs and anything shaped like a token before the user sees it. The hub refuses a title that is empty, longer than 200 characters or holds a control character (`400 bad-title`), a `version` or `os` that is empty or longer than 64 characters (`400 bad-field`), and a text over 12 KiB or a body over 16 KiB (`413 too-large`).
- **The install key.** Each install makes an Ed25519 key on its first report. The key is the sender's id. It signs `"kelpie-report-sign-v1" || nonce || SHA-256(report bytes)`, where the report bytes are `"kelpie-report-v1"` followed by each of the four fields as a 4-byte big-endian length and its UTF-8 bytes.
- **Proof of work.** `GET /v1/reports/challenge` hands out `{nonce, difficulty, expires, tag}`: a fresh 32-byte nonce, the difficulty in bits, an expiry 10 minutes out and an HMAC-SHA256 tag over the three under a key the hub draws at start, so a client cannot lower the difficulty or extend the expiry. The sender looks for a `counter` such that `SHA-256("kelpie-report-pow-v1" || nonce || SHA-256(report bytes) || counter as 8 bytes big-endian)` starts with `difficulty` zero bits. `vectors/reports.json` has a worked case.
- **What the hub checks.** In order: the per-address rate (20 at once, 4 a minute; `429 rate-limited`), the tag (`400 bad-challenge`), the expiry (`400 expired-challenge`), the report's size and shape, the work (`400 insufficient-work`), the signature (`400 bad-signature`), the per-key rate (5 at once, 1 a minute) and that the challenge was not used before (`409 challenge-used`). A challenge is spent by the first report that passes, so a hub restart only ends the challenges handed out before it.
- **Quarantine.** A report from a key the hub has not seen waits in quarantine; at most 20 per key and 10 000 in all do, and further ones are dropped, answered like any other. The operator lets a key through with `admin reports allow <key>`, which releases the ones it has waiting, or drops it with `admin reports block <key>`. A blocked key's reports get the same `202` and an id from the same sequence as any other, and are not kept, so a sender cannot tell it was blocked.
- **Reading them.** `admin reports` lists the allowed reports not yet acknowledged, `--quarantine` the waiting ones, `--json` either as JSON; `admin reports ack <id>` marks one handled. `admin token --name <name>` mints a `kha_` token for a maintainer's machine, shown once and kept as a hash under that name; `admin token list` lists the names with when each was made and last used, and `admin token revoke <name>` ends one at once. The token is for `GET /v1/admin/reports` and `POST /v1/admin/reports/<id>/ack`. kelpie on that machine files each allowed report as a local issue of its own and acknowledges it.

Acknowledged reports and those still in quarantine are deleted 30 days after they arrived.

## Error signals

kelpie also counts its own failures, a panic, an internal error or a state file of its own it cannot read, and sends the counts at most once a day, on unless the user turned them off. A signal carries no text.

- **What a signal holds.** `ErrorSignals` in the protocol crate: `version`, an optional `commit`, `os`, `arch`, and `counts`, a list of `{component, kind, count}`. Every text field is a `Name`: 1 to 48 of lowercase ASCII letters, digits and `.` `_` `+` `-`, starting and ending with a letter or digit, so no sentence, path, quote or capital fits. `kind` is one of `panic`, `internal-error` and `corrupt-state`. The hub refuses an unknown field at any level, a value that is not a `Name` and an unknown kind with `422`, and no counts (`400 no-signals`), more than 64 (`400 too-many-signals`), a count of 0 or over 10 000 (`400 bad-count`), a component and kind twice (`400 repeated-signal`) and a commit that is not up to 40 hex digits (`400 bad-commit`).
- **Signing and work.** The same install key and challenge as a report. The signal bytes are `"kelpie-signal-v1"`, then `version`, `commit` (empty when absent), `os` and `arch` each as a 4-byte big-endian length and its bytes, the number of counts as 4 bytes, and per count its `component` and `kind` the same way and the count as 4 bytes. The key signs `"kelpie-signal-sign-v1" || nonce || SHA-256(signal bytes)`, and the work is the report's over that digest. A challenge is spent by whichever report or signal passes first. The difficulty is the report's: a sender spends about a second once a day, in the background.
- **What the hub checks.** The report's checks, in the same order, with the shape above in place of a report's.
- **What it keeps.** Counts per day, version, commit, OS, architecture, component and kind, and whether the sender's key is allowed; never the key with a count. A key the operator allowed for reports (`admin reports allow`) counts as vetted, any other as unvetted, and a blocked key's signals are dropped, answered with the same `202`. To bound one sender, the hub keeps each key's number of batches for the current day, and forgets it when the day turns (UTC): a key sends at most 4 a day. An address (an IPv4 address, or an IPv6 /64) sends at most 200 batches a day, whatever keys it uses, enough for an office or a carrier NAT full of installs; that count lives in memory only and is never written down, and when the table is full it keeps only the addresses at their cap, so new ones still get in. The address is the connecting one, or `CF-Connecting-IP` with `--trusted-proxy cloudflare`: behind any other proxy every install shares the proxy's address, so the cap holds them all to 200 a day together. Unvetted counts take at most 1 000 000 rows a day, a valve for the disk rather than a limit any real day reaches. Past any of these, a batch is answered like any other and dropped.
- **Reading them.** `admin signals` lists the last 7 days, newest first, `--days <n>` another span, `--json` as JSON; each line is the day, the count, the component and kind, the build, the OS and architecture, and `unvetted` for counts from keys not allowed.

Counts are deleted 180 days after their day.

## Commands

A device answers a question, approves a push, starts a task or runs a routine by sending a command: an envelope of kind `answer`, `push`, `start` or `routine`, sealed to the machine (and to the device itself), signed with the device's key. The hub carries it and cannot read it; the machine decides.

The hub checks what it can without the content, and queues nothing that fails:

- the device token, and that the header is well formed with an id, sender and lifetime that fit a command (at most 24 hours);
- that `from` is the device sending it (`400 wrong-sender`) and `kind` is a command kind (`400 not-a-command`);
- that the machine named in the header admitted the device (`404 unknown-machine`, the same for a machine that does not exist);
- that the key rows are for that machine and at most the device itself (`400 bad-recipients`);
- the signature, against the keys the device was admitted with (`400 bad-signature`), and that it has not expired (`400 expired`) or been signed more than 5 minutes ahead (`400 not-yet-valid`);
- sizes: a request of at most 64 KiB, a ciphertext of at most 36 KiB for `answer` and `start` and 4 KiB for `push` and `routine` (`413 too-large`), at most 1000 envelopes waiting in a machine's queue and at most 100 undelivered commands from one device to one machine (`429 queue-full`);
- a rate per device and kind: 30 at once and 30 a minute for `answer` and `routine`, 5 at once and 5 a minute for `start` and `push` (`429 rate-limited`), on top of the per-token limit every call has.

The machine applies a command with `authorize` from the protocol crate. It drops, without a reply, anything that fails before the signature checks out against its own list of trusted devices. It answers a command it applied before with the stored result. Otherwise it refuses, with a result the device can show, a command that expired while it waited in the queue (`expired`), one from a device with the `read` role or of a kind the machine does not take (`not-allowed`), and one that does not decrypt or is not well formed. Only then does the machine act: a push goes through only if the task's HEAD is still the sha the device signed (`head-moved` otherwise), and in kelpie an answer to a question that no longer waits is `already-settled`.

The hub drops a queued envelope once its `exp` has passed, so a machine that slept through the window comes back to an empty queue. Removing a device, by its account or by a machine, drops its commands that machine has not been handed yet.

The result is an envelope of kind `result` under the command's id, sealed to the device that sent the command. The hub stores it only for a command it delivered to that machine from that device, and announces it on the device's feed by id.

A pairing moves `open → committed → replied → revealed → admitted`, or ends `refused`, `failed` (a second commitment to the same code) or `expired`. Each step is accepted once; anything out of order is `409`. A device's public keys are fixed by its first admission, not by a reveal, so a relay that swaps keys in a refused pairing cannot lock the device out.

Requests are rate limited per token or account (commands also per device and kind, above), and per client address (IPv6 per /64) for unauthenticated calls and bad credentials; an address that has used up its bucket is refused before any lookup. The limiter keeps at most 100 000 buckets. Numbers above `i64::MAX` (`rev`, `after`) get `400`. Bodies are capped at 16 KiB, boards at 1 MiB. A connection that has not sent its request headers within 10 seconds is closed, and a request body that sends nothing for 30 seconds is cut. The hub serves at most 4096 connections at once; further ones wait to be accepted. A device holds at most 8 event feeds at once, and a ninth gets `429 too-many-feeds`. Long-polls and feeds end when the hub shuts down. The hub sends no CORS headers, so the web app is served from the hub's own origin.

Long-polls and event feeds wake only for their own machine, pairing or device.

## Notices and Web Push

A machine tells its devices that something needs them with a notice: an envelope of kind `notice`, sealed to the devices it paired with and signed by the machine. Its header carries `urgency` (`high` for a question, needs-you or a failed CI, otherwise `normal`) and `collapse`, an opaque tag per task, both covered by the signature. The content (`title`, `body`, the question and its options, `retract`) is end to end encrypted like a board.

`POST /v1/m/notices` checks, before anything is pushed:

- that the envelope is a notice (`400 not-a-notice`) for this machine (`400 wrong-machine`), signed by it (`400 bad-signature`), unexpired (`400 expired`) and well formed;
- that its key rows are devices, and at least one (`400 bad-recipients`);
- that each device's cut of the envelope fits one push (`413 too-large`): at most 3993 bytes of envelope JSON, so the encrypted push body stays within the 4096 bytes every push service takes;
- a rate of 30 at once and 30 a minute per machine (`429 rate-limited`), and that the notice id is new (`409 replayed-envelope`).

The hub then pushes to every recipient device that the machine still admits and that has a subscription, at most 30 at once and 30 a minute per device across all machines (a push over that is dropped and logged `push.capped`). The response does not wait for the push services: deliveries run in the background, at most 4096 waiting or retrying and at most 256 requests open at once. A request slot is held only while a request is out, never through a retry's wait, so a failing push service does not hold up the others. A delivery that finds the 4096 taken is dropped and logged. Each delivery checks its endpoint against the known push services and `--push-allow-host` again before sending, so a host dropped from the allowlist gets nothing (`push.refused`). Redirects are not followed.

Each push is the envelope cut to that device's key row, as JSON, encrypted to the subscription's keys with `aes128gcm` (RFC 8291) and sent with:

- `Authorization: vapid t=<JWT>, k=<public key>` (RFC 8292): an ES256 token with `aud` the endpoint's origin, `sub` the `--vapid-subject`, `exp` 12 hours out;
- `TTL`: the seconds left until the envelope's `exp`;
- `Urgency`: the header's urgency;
- `Topic`: the collapse tag when it is 32 characters or fewer, otherwise the first 32 characters of the base64url SHA-256 of it, so a newer notice for the same task replaces an undelivered one at the push service.

How the hub reads the push service's answer: `201` or any other 2xx is delivered; `404` and `410` mean the subscription is gone, and the hub deletes it (iOS hands out a new endpoint whenever the service worker registers again); `413` is logged; `429`, a 5xx or no answer is retried 3 times, waiting 1, 2 and 4 seconds or the `Retry-After` it names (at most a minute), and then dropped; any other status, a redirect included, is logged as `push.refused`, and a `401` or `403` is also printed, since it usually means the VAPID subject or key is wrong. The hub keeps no notice content, so a notice that found no subscription, or whose push was dropped, is gone; the device sees what it was about on the board.

**What a push service sees.** Apple, Google, Mozilla or Microsoft see the hub's address, its VAPID public key and subject, the endpoint, when each push is sent and how large it is, its TTL, urgency and topic. The body is encrypted to the browser's subscription key; inside it is the sealed envelope, which only the device can open. The topic is a hash of an opaque tag, so it groups one task's notices without naming the task. The hub itself sees the same as for a board: which machine notified which devices, when, how urgently and how large, never the text.

iOS does not replace a shown notification that has the same `tag`, so a notice is never quietly withdrawn: when a question is settled elsewhere, the machine sends a new notice whose sealed `retract` names the collapse tag, and the service worker closes the old notification.

## What the hub stores

One sqlite file: machine ids, names, public keys and the account each belongs to; accounts (id, handle, a random WebAuthn user handle, creation time, role, plan status and its end, when it was deactivated, a verified email and when it was verified, and room for a GitHub id); pending connects for 10 minutes (SHA-256 hashes of the device code and the user code, the machine's proposed name and public keys, the approving account and the machine and pairing the approval made, the poll interval and the last poll); each passkey's public key, signature counter, name and use times; SHA-256 hashes of machine, device and session tokens, never the tokens; SHA-256 hashes of join codes, invites, the setup token, email verification tokens (with the address they verify) and pairing codes, so they cannot be read off the file, though a code's 40 bits do not resist offline guessing during its life; pending passkey ceremonies for 5 minutes; device ids, names, public keys and owning account, and which machine admitted each device; the latest board envelope per machine, as ciphertext with its key rows, its id and signed time; queued envelopes until acknowledged or expired; for each command, the sending device, the machine, the id, the kind, a SHA-256 of the envelope (to recognise a resend), its queue position and when it was queued and delivered, kept 7 days; results, as ciphertext with their key rows, until they expire (at most 7 days); pairing sessions, deleted an hour after they expire; each device's push subscription (the endpoint and the browser's public `p256dh` key and `auth` secret, which let the hub encrypt to the browser but not decrypt); the ids of posted notices until they expire, to refuse a replay; error reports (title, text, version and OS as sent, the sender's public key, arrival and acknowledgement times) and each key's standing (quarantine, allowed or blocked) with when it was first and last seen; error signal counts per day, version, commit, OS, architecture, component, kind and whether the sender was vetted, for 180 days, and each sender key's number of signal batches today, until the day turns; the nonces of spent report challenges until they expire; SHA-256 hashes of admin tokens with their names, scopes and when each was made and last used; and an event log of what happened when (joins, stored revisions and sizes, pairing steps and refusal reasons, connects started, approved and denied, sign-ups and how, sign-up mails sent (never the address), plan changes, deactivations, logins, device registrations and removals, queued commands with their kind and size, stored results, push subscriptions with the push service's host, posted notices with their size and number of pushes, each push's outcome with the push service's status, each report's id, outcome and size, never its text, and each signal batch's outcome and number of counts, never its key), kept 30 days.

Every minute the hub sweeps expired sessions, ceremonies, invites, connects (with the machine an unredeemed approval made) and codes, and devices that no account holds and no machine admits once none of their pairings is still open. A database written by schema 6 gains the connect table and the new columns at start-up, one written by schema 5 the signal tables too, and one written by schema 4 the report tables as well; one written by an earlier schema is refused.

Apart from error reports, which are plain text by design, and error signals, which are counts, it never stores or sees a board's, command's, result's or notice's content, a machine's or device's private key, or a token. Its own VAPID private key is in its key file, outside the database. It cannot decrypt an envelope, and it does not decide which devices a machine trusts: it records the machine's decision.

## Not yet

- Answering, pushing and starting tasks from the web app: the buttons are there, disabled.
- The bridge in kelpie that applies commands and posts notices for real, and the collapse key a machine shares with its devices at pairing.
- APNs for an iOS app.
- Signing up through GitHub, and OIDC sign-in for self-hosted hubs.
- Adding or changing a verified email on an existing account.
- Billing itself: the open hub only records each account's plan.
- Logging in without typing a handle (discoverable passkeys).
- Removing an account.
- Logging out everywhere, and ending an account's sessions when one of its passkeys is removed.
- An event to an open feed when `admin remove-machine` runs: the CLI is another process, so a connected device learns of it on its next request.

## Tests

```sh
cargo test
node --test
```

`cargo test` checks the crates and replays `vectors/*.json`. For connecting: the device flow from start to token, `authorization_pending`, `slow_down` and its growing interval, `access_denied` once, `expired_token`, a code approved or redeemed twice refused, an unredeemed approval leaving no machine, the fresh-session and origin checks on approval, the start rate limit, the codes and tokens absent from the files, and the browser that connects reading the board without digits, refused when the link names another machine's keys or carries the wrong secret. For adding a phone: both sides showing the same digits and the phone reading the board, and another account refused at the link and at the commitment. For accounts: the setup link required to claim a fresh hub and spent by the owner, a new link per start, invite links, open sign-up through the logged mail and through a fake SMTP server, the sign-up mail rate, plans off and on for connects and every machine post with the billing link, the owner and join-code machines never gated, the plan API's token and scope, lookup by verified email, the owner-only admin endpoints, the fresh session their destructive actions need, deactivation ending sessions, logins and tokens, and the upgrade of a schema 6 database from `tests/fixtures/schema-6.sql`, its accounts active and later ones inactive. The hub's integration tests start it in-process on a random port and drive it with the fake machine and phone, and with `webauthn-authenticator-rs`'s software passkey for accounts: pairing with matching digits, a relay that swaps the machine's key (the digits differ and the machine refuses) or the device's key (the commitment fails), boards reaching the paired phone and no other, stale revisions, bad tokens, rate limits, a scan of the sqlite files for the board's text and the tokens, sign-up and login with the cookie's flags, invites and ceremonies used once, session expiry, the origin check, device registration behind a session, removal by machine, account and admin CLI ending boards and the event feed, the sweeps, feed events without content, and the header-read timeout. For commands: an answer applied and its result read back; a read-only device refused with `not-allowed`; a resend that queues nothing and a redelivery that replays the stored result; commands to a machine that did not admit the device, forged, misattributed or expired ones refused and never queued; a command that expires in the queue; a push whose head moved; the start rate limit; a result that reaches only the device that sent the command; a scan of the sqlite files for the commands' text; and `result` events without content. For notices, a local fake push service in the test decrypts every push with the subscription's private key (RFC 8291) and checks that it is the machine's envelope cut to that device's row and opens only on that device, verifies the VAPID token's ES256 signature against `GET /v1/push/vapid`, its `aud`, `sub` and `exp`, and the `Urgency`, `TTL`, `Topic` and `Content-Encoding` headers; further tests cover a failing push service not holding up deliveries to another, an endpoint moving within an account and refused across accounts, a stored endpoint off the allowlist refused at delivery, a `302` not followed, a key file others can read refused at start, misaddressed and misfiled notices, the sweep of notice ids, a device without a subscription, one that unsubscribed and one the machine removed getting nothing, `404` and `410` pruning the subscription, `429` and `503` retried until accepted, a service that keeps failing dropped after its retries, a notice too large for one push refused at the post, forged, replayed and misfiled notices, the machine rate and the device cap, endpoints off the known push services refused, a VAPID subject push services reject refused at start, the key file kept across restarts with mode 0600, and a scan of the data directory and the event log for the notice's text, ciphertext and collapse tag. For error reports: a valid report waiting in quarantine until its key is allowed, then handed to the admin API until it is acknowledged; a bad or moved signature, a lowered difficulty, a lengthened expiry, a made-up nonce, an expired or spent challenge and too little work refused; an oversized text and an oversized body refused; the per-key and per-address rates, with the address taken from `CF-Connecting-IP` behind Cloudflare; a blocked key's report answered like any other and dropped, its id spent; a full quarantine and one key's share of it; the admin API refusing anything but an admin token, which is kept as a hash, and a revoked one; the upgrade of a schema 4 database; and the admin CLI. For error signals: a random sweep of strings showing no space, path, quote, capital or control character ever becomes a `Name`, on its own or on the wire; unknown fields at every level, text in a component or the OS, and an unknown kind refused; empty, oversized, zero and repeated counts and a bad commit refused; a moved count, a report's signature or too little work refused, and a challenge spent on signals refused for a report; an unknown key's counts kept apart from an allowed key's, and a blocked key's dropped with nothing of it kept; the per-key batches and the unvetted cap; a flood from one address leaving another's counts alone; the sweep of senders when the day turns and counts after 180 days; the upgrade of a schema 5 database keeping its reports; and `admin signals`. The encryption is also checked against the example in RFC 8291 section 5, byte for byte, on both the hub's and the fake phone's side. `node --test` (Node 25 or newer) checks the same vectors with WebCrypto as browsers expose it, plus [`@hpke/core`](https://github.com/dajiaji/hpke-js) for HPKE: it opens what Rust sealed with non-extractable keys, reseals it byte for byte, refuses every tampered, expired, misaddressed or malformed case with the same error as Rust, and runs the pairing flows. Run `npm install` once first.

The vectors are generated from fixed keys and randomness:

```sh
cargo run -p kelpie-hub-protocol --features test-vectors --bin vectors
```

## License

MIT
