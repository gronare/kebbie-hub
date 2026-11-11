# Contributing

Contributions are licensed under the project's [MIT licence](LICENSE).

## Sign off every commit

Every commit in a pull request carries a `Signed-off-by:` line with its author's name and email, certifying the [Developer Certificate of Origin](https://developercertificate.org/): that you wrote the change or have the right to submit it under the project's licence.

```sh
git commit -s
```

To sign off commits already on your branch:

```sh
git rebase --signoff main
```

A check on the pull request fails until every commit is signed off by its author.

## Before you open a pull request

Run the tests (`node --test` needs Node 25 or newer):

```sh
cargo test
npm install
node --test
```

`cargo test` replays the shared vectors in `vectors/*.json`; after a change to the protocol, regenerate them from their fixed keys with `cargo run -p kebbie-hub-protocol --features test-vectors --bin vectors`.

CI also checks the licences of the Rust crates (`deny.toml`) and the npm packages; a new dependency needs a permissive licence.
