# userserversd

A daemon for managing simple user services.

Think of this as a very small init system for managing services on a user account.

## Why?

I wanted something to replace the mess of shell scripts I used on my servers, and I don't like creating system-wide service units for stuff I keep in my home directory.

Keeping servers in my home directory makes it easier to for me to manage and transfer them between machines.

## Building

This is a Rust project, so you can build it with the following command:

```console
$ cargo build # or cargo build --release
```

This will generate two binaries: `userserversd` and `userserversctl`. Run `userserversd` to start the daemon, and use `userserversctl` to add/remove/edit services.

## Building a Linux Binary for Distribution

To build a Linux binary that can work in any distribution without installing dependencies (i.e. static linking), you can install [musl](https://musl.libc.org/) in your system, add the `x86_64-unknown-linux-musl` target to your Rust toolchain, and run [the provided build script](./build-dist.sh) like so:

```console
$ ./build-dist.sh
```

This will produce binaries at `./target/x86_64-unknown-linux-musl/release/`.
