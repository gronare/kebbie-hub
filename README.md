# kebbie-hub

Follow [kebbie](https://github.com/gronare/kebbie) from your phone: the hub shows every machine's board, its questions with their plans and its tasks' cards, end-to-end encrypted so the hub itself cannot read them.

## Connect a machine

On a machine running kebbie:

```sh
kebbie hub connect
```

It offers the public hub at hub.kebbie.dev or your own, shows a code, and opens your browser. Sign in with a passkey, check that the code matches, and the machine's board appears. Add the page to your home screen to use it as an app.

## What you get

- Every connected machine's board in one place: what needs you first, then the work in progress.
- A task's question with its options, and the plan behind it in full.
- A ready task's card, and whether it still matches the task's latest commit.
- On a hub run with `--commands on`: answers, choices on an agent's prompt and pushes from a device where you turn on "Allow changes from this device" under Account. That device gets its own key, which your account certifies, and signs what it sends. Each machine's `hub.commands` setting in kebbie decides what it takes, and you approve or deny each device in kebbie on that machine. Account lists your devices, where each one stands on each machine, and what it sent in the last 90 days.
- Layouts for phones, the iPhone Duo folded and unfolded, tablets and desktops.
- Boards sealed on the machine to your account's key; only your passkeys unlock that key, and the hub stores what it cannot open.

## Run your own hub

Build it (needs Rust, a C compiler, `make` and `perl`):

```sh
cargo build --release -p kebbie-hub
```

Run it behind TLS on your own domain:

```sh
target/release/kebbie-hub --data /var/lib/kebbie-hub --listen 0.0.0.0:8470 \
  --rp-id hub.example.com --origin https://hub.example.com
```

On its first start the hub prints a one-time setup link to its log; open it to create the owner's passkey. Then run `kebbie hub connect` on a machine and give it `https://hub.example.com`. `kebbie-hub --help` lists the rest of the options.

To try it on your own computer, `bin/dev` runs a hub at `http://localhost:8470`, and `KEBBIE_HUB_DEV=1 kebbie hub connect` connects a machine to it.

## More

- [kebbie](https://github.com/gronare/kebbie), the tool the hub serves
- [SECURITY.md](SECURITY.md) to report a vulnerability
- [CONTRIBUTING.md](CONTRIBUTING.md) to work on the hub

## License

MIT, see [LICENSE](LICENSE).
