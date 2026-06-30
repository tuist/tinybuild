# tinybuild

A tiny build system in a few hundred lines of Rust. It exists to make one idea concrete: **a build system is a DAG of cacheable, hermetic actions.**

An action is a shell script that declares its contract in header comments. From those declarations tinybuild builds a graph, runs each task in a sandbox containing only what it declared, and content-addresses the result so unchanged work is skipped, locally or pulled from a shared store. It is small enough to read in one sitting, but it borrows the ideas that make Bazel and Nix correct: hermetic execution, content-addressed outputs, early cutoff, and a shared store with a remote cache and garbage collection.

This is the companion code for the blog post [_Three build systems, one graph_](https://tuist.dev/blog/2026/06/30/three-build-systems-one-graph). It is a teaching tool, not something to build real software with.

## A task is a script with a contract

```sh
#!/usr/bin/env bash
# tinybuild needs ./compile.sh
# tinybuild needs ./resource.sh
# tinybuild input Info.plist
# tinybuild output MyApp.app
set -euo pipefail

app="MyApp.app"
rm -rf "$app"; mkdir -p "$app"
cp MyApp "$app/MyApp"          # produced by ./compile.sh
cp message.txt "$app/message.txt"   # produced by ./resource.sh
cp Info.plist "$app/Info.plist"     # a declared input
```

The directives are the whole language:

- **`needs`**: another task, by relative path to its script. A task's outputs are materialized into this task's sandbox.
- **`input`**: a glob, relative to the project root, whose matched files are part of the cache key and materialized into the sandbox.
- **`output`**: a path the task produces. Stored by content and restored on a cache hit.
- **`env`**: an environment variable the result depends on. The environment is otherwise scrubbed.
- **`tool`**: a command that fingerprints a tool, e.g. `swiftc --version`. The toolchain is an input, so upgrading it invalidates correctly.

The OS and architecture are folded into every key automatically.

## Try it

The example in `example/` builds a minimal iOS `.app` bundle from a single Swift file and a resource: it compiles the Swift file against the iOS Simulator SDK, processes the resource, and assembles `MyApp.app`. It needs a full Xcode install (for the iOS SDK and `swiftc`).

```sh
cargo build
cd example

../target/debug/tinybuild graph   # show the execution waves
../target/debug/tinybuild plan     # print the build plan as JSON
../target/debug/tinybuild run      # first run: everything executes
../target/debug/tinybuild run      # second run: everything is cached
```

The result is `example/out/MyApp.app`, which you could install into a booted simulator with `xcrun simctl install booted out/MyApp.app`.

Edit `example/Sources/main.swift` and run again: `compile` reruns because its input changed, `bundle` reruns because it depends on `compile`, and `resource` stays cached. Run with `CONFIGURATION=release ../target/debug/tinybuild run` and `compile` reruns too, because the declared environment value is part of its key.

## The store

Outputs live in a content-addressed store shared across projects, by default `~/.cache/tinybuild/store` (override with `--store` or `TINYBUILD_STORE`).

```sh
# pull from and push to a remote store (a "substituter")
tinybuild run --substituter /path/to/shared/store

# drop everything the last build cannot reach
tinybuild gc
```

Because outputs are addressed by content and the machine that produced them does not matter, a build on one machine can be restored on another straight from the substituter.

## How it works

- **`src/task.rs`** parses the `# tinybuild` headers and resolves `needs` paths into task ids.
- **`src/graph.rs`** turns tasks into a DAG, refuses cycles, finds roots, and groups tasks into waves of independent work.
- **`src/store.rs`** is the content-addressed store: blobs, an action cache, a substituter, and garbage collection.
- **`src/build.rs`** computes each task's key, runs it in a scrubbed sandbox containing only its declared inputs and its dependencies' outputs, and stores the result. A task's key includes its dependencies' *output* hashes, which is what gives early cutoff: a dependency that reruns but produces identical bytes does not invalidate its dependents.

## What it shows, and what it skips

It implements the load-bearing ideas: a graph, hermetic execution (an undeclared read fails because the file is not in the sandbox), content-addressed outputs with early cutoff, tools and platform as inputs, a shared store with a remote cache and GC, and a build plan you could ship elsewhere.

It deliberately skips the hard engineering: true OS-level sandboxing (it isolates declared project files but still lets tools reach the system through `PATH`), dynamic dependencies discovered mid-build, remote *execution*, and scaling to millions of nodes. Those are where real build systems spend their complexity.

## License

[MIT](LICENSE).
