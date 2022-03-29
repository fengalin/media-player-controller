# mpris-controller

`mpris-controller` is an interface to control an mpris player from a
Universal Control Surface such as the Behringer X-Touch One.

## Build

You need a stable Rust toolchain for the target host. Get it for [this page](https://www.rust-lang.org/fr/tools/install).
On Unix-like systems, you should be able to install `rustup` from your packet
manager.

Clone the git tree and run the following command in an environment where
`cargo` is available:

```
cargo b --release
```

## Run

If compilation succeeds, you should be able to launch the executable with:

```
target/release/mpris-controller
```

## LICENSE

This crate is licensed under MIT license ([LICENSE-MIT](LICENSE-MIT) or
http://opensource.org/licenses/MIT)
