# kelpie-hub

A hub that lets you follow and steer [kelpie](https://github.com/gronare/kelpie) from your phone. Each machine running kelpie dials out to the hub; your devices read every machine's board, get notified when a task needs you, and answer questions, approve pushes or start tasks.

The hub is a mailbox, never a party. Everything it carries is end-to-end encrypted between a machine and the devices that machine has paired with, and every command is signed by a device the machine trusts. The hub sees who talks to whom and when, not what is said, and it cannot start anything on a machine.

This repository holds:

- `kelpie-hub-protocol`: envelopes, messages and pairing, plus the test vectors other implementations are checked against.
- `kelpie-hub`: the hub server. It relays sealed boards from machines to the devices they paired with, and carries pairing between them.
- `kelpie-hub-api`: the request and response bodies of the hub's HTTP API.
- `kelpie-hub-fakes`: `fake-machine` and `fake-phone`, two CLIs that stand in for kelpie and the web app.

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

`--dev` listens on `127.0.0.1:8470` and keeps its sqlite file in `kelpie-hub-dev` under the system temp directory. `--data <dir>` and `--listen <address>` override either; without `--dev` both are required. Behind Cloudflare, `--trusted-proxy cloudflare` takes the client address from `CF-Connecting-IP`; without it the hub uses the TCP peer and ignores that header.

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

`fake-machine poll` long-polls the machine's queue once, and `fake-phone attempts` lists every pairing attempt, failed ones included. Both CLIs keep their keys and tokens in the `--state` file, written with mode 0600.

## The hub's API

Every endpoint is under `/v1` and takes a bearer token, except joining and committing to a pairing, where the one-time code is the credential. Machine tokens start with `khm_`, device tokens with `khd_`.

| Call | Who | What |
|---|---|---|
| `POST /v1/m/join` | machine, join code | trades a code from `kelpie-hub admin join-code` and the machine's public keys for a machine id and token |
| `PUT /v1/m/board` | machine | `{rev, envelope}`: a board sealed by the machine; the hub checks the machine's signature, then orders boards by the envelope's signed `at` with `rev` breaking ties within one second. It refuses the envelope it already holds or one signed earlier (`409 replayed-envelope`) and, at the same `at`, a `rev` at or below the one it holds (`409 stale-revision`) |
| `GET /v1/m/inbox?after=<seq>&wait=<s>` | machine | waits up to 25 s for queued envelopes with `seq > after`, plus the `board_rev` the hub holds; `after` acknowledges, and the hub deletes what it covers |
| `POST /v1/m/pairings` | machine | opens one pairing invocation and returns its code, valid 10 minutes |
| `GET /v1/m/pairings/<id>?seen=<state>&wait=<s>` | machine | waits until the pairing leaves `seen`; shows the device's commitment, then its revealed keys and nonce |
| `POST /v1/m/pairings/<id>/reply` | machine | `M` and `n_m`, once, after the commitment |
| `POST /v1/m/pairings/<id>/admit` | machine | after the user confirmed the digits; the hub records which machine admitted the device |
| `POST /v1/m/pairings/<id>/refuse` | machine | `mismatch`, `declined` or `timeout` |
| `POST /v1/d/pairings` | device, pairing code | the commitment; a device without a token gets an id and token here. The same token sending the same commitment again gets the same answer; any other second commitment fails the pairing. An unknown code and a code whose pairing has ended both get `401` |
| `GET /v1/d/pairings/<id>?seen=<state>&wait=<s>` | device | waits like the machine's; shows `M` and `n_m`, then the outcome |
| `POST /v1/d/pairings/<id>/reveal` | device | `D` and `n_d`, once, after the reply |
| `GET /v1/d/boards` | device | the latest board of every machine that admitted the device, with only this device's key row |

A pairing moves `open → committed → replied → revealed → admitted`, or ends `refused`, `failed` (a second commitment to the same code) or `expired`. Each step is accepted once; anything out of order is `409`. A device's public keys are fixed by its first admission, not by a reveal, so a relay that swaps keys in a refused pairing cannot lock the device out.

Requests are rate limited per token, and per client address (IPv6 per /64) for unauthenticated calls and bad tokens; an address that has used up its bucket is refused before any token lookup. The limiter keeps at most 100 000 buckets. Numbers above `i64::MAX` (`rev`, `after`) get `400`. Bodies are capped at 16 KiB, boards at 1 MiB. The hub sends no CORS headers.

## What the hub stores

One sqlite file: machine ids, names and public keys; SHA-256 hashes of machine and device tokens, never the tokens; SHA-256 hashes of join and pairing codes, so they cannot be read off the file, though a code's 40 bits do not resist offline guessing during its 10-minute life; device ids and public keys, and which machine admitted each device; the latest board envelope per machine, as ciphertext with its key rows, its id and signed time; queued envelopes until acknowledged; pairing sessions, deleted an hour after they expire; and an event log of what happened when (joins, stored revisions and sizes, pairing steps and refusal reasons), kept 30 days.

It never stores or sees a board's content, a private key, or a token. It cannot decrypt an envelope, and it does not decide which devices a machine trusts: it records the machine's decision.

## Not yet

- Removing or revoking a device or a machine.
- Sweeping devices that no machine admitted.
- Waking only the waiters of one machine or pairing: every change wakes every long-poll, which then rechecks.
- A header-read timeout for running the hub exposed without a proxy in front.

## Tests

```sh
cargo test
node --test
```

`cargo test` checks the crates and replays `vectors/*.json`. The hub's integration tests start it in-process on a random port and drive it with the fake machine and phone: pairing with matching digits, a relay that swaps the machine's key (the digits differ and the machine refuses) or the device's key (the commitment fails), boards reaching the paired phone and no other, stale revisions, bad tokens, rate limits, and a scan of the sqlite files for the board's text. `node --test` (Node 25 or newer) checks the same vectors with WebCrypto as browsers expose it, plus [`@hpke/core`](https://github.com/dajiaji/hpke-js) for HPKE: it opens what Rust sealed with non-extractable keys, reseals it byte for byte, refuses every tampered, expired, misaddressed or malformed case with the same error as Rust, and runs the pairing flows. Run `npm install` once first.

The vectors are generated from fixed keys and randomness:

```sh
cargo run -p kelpie-hub-protocol --features test-vectors --bin vectors
```

## License

MIT
