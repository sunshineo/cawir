# `.gitignore` and the `target/` directory

## `.gitignore`

```
/target
```

A single line. Ignores the `target/` directory — the build output root.

Cargo creates this file when it initializes a new VCS (during `cargo new` / `cargo init` if no `.git` exists yet). If a repo already exists when you run `cargo init`, Cargo still writes the `.gitignore` with `/target` so the build output won't accidentally get committed.

## What's actually in `target/`

Not committed, but worth understanding:

```
target/
├── debug/              — debug build output
│   ├── cawir           — the compiled binary (what `cargo run` executes)
│   ├── build/          — output of build.rs scripts, if any
│   ├── deps/           — compiled dependencies + the crate itself (.rlib files)
│   ├── examples/       — compiled example binaries
│   ├── incremental/    — rustc's incremental compile cache
│   └── .fingerprint/   — cargo's change-detection cache (what changed vs. last build)
├── release/            — release build output (appears only after `cargo build --release`)
├── doc/                — rustdoc HTML output (appears only after `cargo doc`)
└── package/, CACHEDIR.TAG, ...
```

Can easily grow to hundreds of MB once dependencies are added — a real project with 30+ deps commonly has `target/` in the 500MB–2GB range. Fully regeneratable — `cargo clean` nukes it.

## `target/` vs `~/.cargo/registry/`

A key distinction that makes Rust different from Node:

- **`~/.cargo/registry/`** — global, per-user. Contains **source code** of every crate you've ever downloaded, across all your Rust projects on this machine. Shared.
- **`target/`** — per-project. Contains **compiled artifacts** from building this crate and its deps. Not shared.

The upshot: if you have 10 Rust projects that all use `reqwest`, the `reqwest` source is stored **once** in `~/.cargo/registry/`, but each project compiles it independently into its own `target/deps/`.

Contrast with Node: each project's `node_modules/` contains both the source AND the compiled/installed form of every dep — duplicated across projects. Famously disk-heavy.

## Running the scaffold

```
$ cargo run
   Compiling cawir v0.1.0 (/Users/gordon/code/cawir)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.72s
     Running `target/debug/cawir`
Hello, world!
```

Three notable lines:

- **`Compiling cawir v0.1.0`** — rustc invoked on `src/main.rs`.
- **`Finished 'dev' profile [unoptimized + debuginfo]`** — we used the default **dev profile**, which prioritizes fast builds and usable debugging over runtime speed. `--release` switches to the **release profile** (optimized, no debuginfo, takes longer to compile).
- **`Running 'target/debug/cawir'`** — the compiled binary lives at `target/debug/<crate-name>` for binary crates. For `--release` builds, it's `target/release/<crate-name>`.

## The two default profiles

| Profile | Dir | Opt level | Debug info | Compile time | Runtime speed | When |
|---|---|---|---|---|---|---|
| `dev` | `target/debug/` | 0 (none) | full | fast | slow | default for `cargo run`/`build`/`test` |
| `release` | `target/release/` | 3 (max) | none | slow | fast | with `--release` |

Both configurable via `[profile.dev]` / `[profile.release]` in `Cargo.toml` (see `03b-cargo-toml.md`).

## Cleaning up

```bash
cargo clean                     # wipes entire target/ — next build is from scratch
cargo clean --release           # wipes only target/release/
cargo clean -p some_dep         # wipes just one dependency's artifacts
```

`target/` is fully regeneratable. Safe to delete at any time — the worst case is that the next build is slower.
