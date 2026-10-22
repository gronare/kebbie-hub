# kelpie-hub

A hub that lets you follow and steer [kelpie](https://github.com/gronare/kelpie) from your phone. Each machine running kelpie dials out to the hub; your devices read every machine's board, get notified when a task needs you, and answer questions, approve pushes or start tasks.

The hub is a mailbox, never a party. Everything it carries is end-to-end encrypted between a machine and the devices that machine has paired with, and every command is signed by a device the machine trusts. The hub sees who talks to whom and when, not what is said, and it cannot start anything on a machine.

This repository holds:

- `kelpie-hub-protocol`: envelopes, messages and pairing, plus the test vectors other implementations are checked against.
- `kelpie-hub`: the hub server. It relays sealed boards from machines to the devices they paired with, signed commands from devices to machines and sealed results back, and carries pairing between them.
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

## Running a hub locally

```sh
cargo build --workspace
target/debug/kelpie-hub --dev
```

The hub links OpenSSL (through `webauthn-rs`) built from source and statically, so the binary needs no system OpenSSL; building it needs a C compiler, `make` and `perl`.

`--dev` listens on `127.0.0.1:8470`, keeps its sqlite file in `kelpie-hub-dev` under the system temp directory, uses `localhost` as the passkey relying party with `http://localhost:8470` as the origin, and lets a pairing code create a device without an account so the fake phone can pair. `--data <dir>`, `--listen <address>`, `--rp-id <domain>` and `--origin <url>` override these; without `--dev` all four are required, and with `--dev` the hub refuses to listen on anything but a loopback address. Behind Cloudflare, `--trusted-proxy cloudflare` takes the client address from `CF-Connecting-IP`; without it the hub uses the TCP peer and ignores that header.

A public hub:

```sh
kelpie-hub --data /var/lib/kelpie-hub --listen 0.0.0.0:8470 \
  --rp-id hub.example.com --origin https://hub.example.com --trusted-proxy cloudflare
kelpie-hub --data /var/lib/kelpie-hub admin invite
```

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

`fake-machine apply` runs until stopped (`--once` polls once); it keeps the ids it applied, with their results, in its state file. `fake-machine role <device> read` makes a device read-only, so its commands are refused with `not-allowed`. `fake-machine poll` long-polls the machine's queue once, and `fake-phone attempts` lists every pairing attempt, failed ones included. Both CLIs keep their keys and tokens in the `--state` file, written with mode 0600. Against a hub without `--dev`, give the phone a device an account registered: `fake-phone init <hub> --device <id> --token <token>`.

## Accounts and who holds what

People sign in to the hub with its own accounts. An account holds passkeys (WebAuthn, user verification required) and the devices it registered; it never sees a board, because boards are sealed to device keys the account does not have.

- **Sign-up** takes a one-time invite from `kelpie-hub admin invite` and a handle (lowercase letters, digits and `-`, starting with a letter), then registers the first passkey. The invite is spent when the passkey is accepted, so two ceremonies started with one invite make one account. The account table has room for a GitHub id and a verified email, for sign-up through GitHub or an email link later. Safari refuses `Secure` and `__Host-` cookies on `http://localhost`, so a `--dev` hub signs in from Chrome or Firefox, or from Safari behind a local TLS name.
- **Logging in** takes the handle and a passkey the account holds. Handles are not secret: logging in tells whether one exists. The hub keeps each passkey's signature counter and refuses a login whose counter does not move past it (unless both are zero), as a sign the passkey was cloned.
- **The session** is an opaque `khs_` token in the cookie `__Host-kelpie_session` with `HttpOnly`, `Secure`, `SameSite=Strict` and `Path=/`, valid 14 days. The hub stores its SHA-256 hash. Logging out deletes it.
- **Cross-site requests.** Every state-changing `/v1/a/` call needs an `Origin` header equal to `--origin`, or gets `403 cross-origin`. The cookie is accepted on `/v1/a/` only.
- **Devices.** A logged-in account registers a device and gets its `khd_` token once. Every `/v1/d/` call, the event feed included (read with `fetch`, since `EventSource` cannot send a bearer token), takes that token as a bearer token and never the cookie, so no device call rides on an ambient credential and none needs a CSRF check. The web app keeps the token next to its non-extractable device keys in IndexedDB, and uses its session only to register a device or manage the account. Logging out leaves the device working; removing the device ends it.
- **Removal.** A machine removes a device it admitted, and the hub stops handing that device the machine's board. An account removes its own devices, which deletes their tokens and ends their event feeds. An admin removes a machine.

Ceremony state (the challenge a passkey must sign) is kept on the hub for 5 minutes and used once; it never reaches the browser.

## The hub's API

Every endpoint is under `/v1`. Machine endpoints (`/v1/m/`) take a `khm_` bearer token, except joining, where the join code is the credential. Device endpoints (`/v1/d/`) take a `khd_` bearer token. Account endpoints (`/v1/a/`) take the session cookie, except signing up and logging in.

| Call | Who | What |
|---|---|---|
| `POST /v1/m/join` | machine, join code | trades a code from `kelpie-hub admin join-code` and the machine's public keys for a machine id and token |
| `PUT /v1/m/board` | machine | `{rev, envelope}`: a board sealed by the machine; the hub checks the machine's signature, then orders boards by the envelope's signed `at` with `rev` breaking ties within one second. It refuses the envelope it already holds or one signed earlier (`409 replayed-envelope`) and, at the same `at`, a `rev` at or below the one it holds (`409 stale-revision`) |
| `GET /v1/m/inbox?after=<seq>&wait=<s>` | machine | waits up to 25 s for queued envelopes with `seq > after`, plus the `board_rev` the hub holds; `after` acknowledges, and the hub deletes what it covers. Handing a command over marks it delivered |
| `POST /v1/m/results` | machine | `{envelope}`: a result sealed to the device that sent the command, with the command's id as its own. `400` for a bad signature or recipients, `404 unknown-command` for a command this machine was not sent by that device (or one the hub no longer holds; the machine drops that result and moves on), `409 not-delivered` before the inbox handed it over. Posting the same envelope again returns the same `seq`; a different result for a command that has one is `409 result-differs` |
| `POST /v1/m/pairings` | machine | opens one pairing invocation and returns its code, valid 10 minutes |
| `GET /v1/m/pairings/<id>?seen=<state>&wait=<s>` | machine | waits until the pairing leaves `seen`; shows the device's commitment, then its revealed keys and nonce |
| `POST /v1/m/pairings/<id>/reply` | machine | `M` and `n_m`, once, after the commitment |
| `POST /v1/m/pairings/<id>/admit` | machine | after the user confirmed the digits; the hub records which machine admitted the device |
| `POST /v1/m/pairings/<id>/refuse` | machine | `mismatch`, `declined` or `timeout` |
| `DELETE /v1/m/devices/<id>` | machine | stops handing the machine's board to a device it admitted; `404` for a device it did not admit |
| `POST /v1/d/pairings` | device, pairing code | the commitment. The same token sending the same commitment again gets the same answer; any other second commitment fails the pairing. An unknown code and a code whose pairing has ended both get `401`. Only a `--dev` hub accepts a commitment without a token, and hands out a device id and token with it |
| `GET /v1/d/pairings/<id>?seen=<state>&wait=<s>` | device | waits like the machine's; shows `M` and `n_m`, then the outcome |
| `POST /v1/d/pairings/<id>/reveal` | device | `D` and `n_d`, once, after the reply |
| `POST /v1/d/commands` | device | `{envelope}`: a signed, sealed command; `202` with `{command, seq}`, its place in the machine's queue. Sending the same envelope again returns the same `seq` and queues nothing; another envelope under an id already used is `409 command-id-taken` |
| `GET /v1/d/results?after=<seq>&wait=<s>` | device | waits up to 25 s for results to this device with `seq > after`, each with only this device's key row. `seq` counts this device's results only. `after` is a cursor, not an acknowledgement: results stay until they expire |
| `GET /v1/d/boards` | device | the latest board of every machine that admitted the device, with only this device's key row |
| `GET /v1/d/events` | device | a server-sent event stream: `board` (`{machine, rev}`) when an admitting machine stores a board, `pairing` (`{pairing, state}`) when one of the device's pairings changes or expires, `machine-removed` (`{machine}`) when a machine removes the device, `result` (`{machine, command}`, with the result's `seq` as the event id) when a result to the device is stored. It starts with the current state: a new feed without `Last-Event-ID` announces every result the hub still holds for the device (at most 7 days back), and with it only those after that `seq` (a `Last-Event-ID` that is not a number up to `i64::MAX` gets `400 bad-last-event-id`). It carries no content, keeps alive every 15 s and ends when the device is removed |
| `POST /v1/a/register/begin` | anyone, invite | `{invite, handle}`; returns a ceremony id and the passkey creation options |
| `POST /v1/a/register/finish` | anyone | `{ceremony, credential, name?}`; creates the account, spends the invite and sets the session cookie |
| `POST /v1/a/login/begin` | anyone | `{handle}`; returns a ceremony id and the passkey request options |
| `POST /v1/a/login/finish` | anyone | `{ceremony, credential}`; sets the session cookie |
| `POST /v1/a/logout` | account | deletes the session and clears the cookie |
| `GET /v1/a/account` | account | the account id, handle and creation time |
| `GET /v1/a/passkeys` | account | the account's passkeys: id, name, created, last used |
| `POST /v1/a/passkeys/begin`, `/finish` | account | adds a passkey, excluding the ones the account holds |
| `DELETE /v1/a/passkeys/<id>` | account | removes a passkey; the last one stays (`409 last-passkey`) |
| `POST /v1/a/devices` | account | `{name?}`; registers a device and returns its id and token, once |
| `GET /v1/a/devices` | account | the account's devices with the machines that admitted each |
| `DELETE /v1/a/devices/<id>` | account | removes the device, its token and its admissions |

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

## What the hub stores

One sqlite file: machine ids, names and public keys; accounts (id, handle, a random WebAuthn user handle, creation time, and room for a GitHub id and a verified email); each passkey's public key, signature counter, name and use times; SHA-256 hashes of machine, device and session tokens, never the tokens; SHA-256 hashes of join codes, invites and pairing codes, so they cannot be read off the file, though a code's 40 bits do not resist offline guessing during its life; pending passkey ceremonies for 5 minutes; device ids, names, public keys and owning account, and which machine admitted each device; the latest board envelope per machine, as ciphertext with its key rows, its id and signed time; queued envelopes until acknowledged or expired; for each command, the sending device, the machine, the id, the kind, a SHA-256 of the envelope (to recognise a resend), its queue position and when it was queued and delivered, kept 7 days; results, as ciphertext with their key rows, until they expire (at most 7 days); pairing sessions, deleted an hour after they expire; and an event log of what happened when (joins, stored revisions and sizes, pairing steps and refusal reasons, sign-ups, logins, device registrations and removals, queued commands with their kind and size, stored results), kept 30 days.

Every minute the hub sweeps expired sessions, ceremonies, invites and codes, and devices that no account holds and no machine admits once none of their pairings is still open. A database written by an earlier schema is refused at start-up.

It never stores or sees a board's, command's or result's content, a private key, or a token. It cannot decrypt an envelope, and it does not decide which devices a machine trusts: it records the machine's decision.

## Not yet

- The web app.
- The bridge in kelpie that applies commands for real, and notices.
- Signing up through GitHub or an email link, and OIDC sign-in for self-hosted hubs.
- Logging in without typing a handle (discoverable passkeys).
- Removing an account.
- Logging out everywhere, and ending an account's sessions when one of its passkeys is removed.
- An event to an open feed when `admin remove-machine` runs: the CLI is another process, so a connected device learns of it on its next request.

## Tests

```sh
cargo test
node --test
```

`cargo test` checks the crates and replays `vectors/*.json`. The hub's integration tests start it in-process on a random port and drive it with the fake machine and phone, and with `webauthn-authenticator-rs`'s software passkey for accounts: pairing with matching digits, a relay that swaps the machine's key (the digits differ and the machine refuses) or the device's key (the commitment fails), boards reaching the paired phone and no other, stale revisions, bad tokens, rate limits, a scan of the sqlite files for the board's text and the tokens, sign-up and login with the cookie's flags, invites and ceremonies used once, session expiry, the origin check, device registration behind a session, removal by machine, account and admin CLI ending boards and the event feed, the sweeps, feed events without content, and the header-read timeout. For commands: an answer applied and its result read back; a read-only device refused with `not-allowed`; a resend that queues nothing and a redelivery that replays the stored result; commands to a machine that did not admit the device, forged, misattributed or expired ones refused and never queued; a command that expires in the queue; a push whose head moved; the start rate limit; a result that reaches only the device that sent the command; a scan of the sqlite files for the commands' text; and `result` events without content. `node --test` (Node 25 or newer) checks the same vectors with WebCrypto as browsers expose it, plus [`@hpke/core`](https://github.com/dajiaji/hpke-js) for HPKE: it opens what Rust sealed with non-extractable keys, reseals it byte for byte, refuses every tampered, expired, misaddressed or malformed case with the same error as Rust, and runs the pairing flows. Run `npm install` once first.

The vectors are generated from fixed keys and randomness:

```sh
cargo run -p kelpie-hub-protocol --features test-vectors --bin vectors
```

## License

MIT
