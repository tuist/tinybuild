# tinybuild

A tiny build system in a few hundred lines of Rust. It exists to make one idea concrete: **a build system is a DAG of cacheable actions.**

Here an action is just a shell script that declares its contract in header comments. From those declarations tinybuild builds a graph, runs independent work in parallel, and skips anything whose inputs have not changed.

This is the companion code for the blog post [_Three build systems, one graph_](https://tuist.dev/blog/2026/06/30/three-build-systems-one-graph). It is a teaching tool, not something to build real software with.

## A task is a script with a contract

```sh
#!/usr/bin/env bash
# tinybuild needs banner
# tinybuild input build/greeting.txt
# tinybuild output build/package.txt
# tinybuild env GREETING

cat build/banner.txt build/greeting.txt > build/package.txt
```

Four directives, and that is the whole language:

- **`needs`**: another task that must run first. This is an edge in the graph.
- **`input`**: a glob whose matched files decide whether the work changed.
- **`output`**: a path the task produces, restored on a cache hit.
- **`env`**: an environment variable that is folded into the cache key.

## Try it

```sh
cargo build
cd example

../target/debug/tinybuild graph   # show the execution waves
../target/debug/tinybuild run     # first run: everything executes
../target/debug/tinybuild run     # second run: everything is cached
```

Change `example/names.txt` and run again. The `greeting` task reruns because one of its inputs changed, and `package` reruns because it depends on `greeting`, but `banner` stays cached because nothing it declared moved.

## How it works

The interesting part is small:

- **`src/task.rs`** parses the `# tinybuild` headers into a `Task`.
- **`src/graph.rs`** turns tasks into a DAG, refuses cycles, and groups tasks into waves of independent work.
- **`src/cache.rs`** computes each task's identity as a hash of its script, its input contents, its declared environment values, and the identities of its dependencies, then stores and restores outputs by that key.
- **`src/runner.rs`** walks the waves, runs each wave on threads, and asks the cache before doing any work.

## The honest-declaration catch

The cache is only correct if a task reads nothing it did not declare. A task that reads an undeclared file gets the same key for two different states of the world, so the cache serves a stale result. tinybuild does not stop you from doing this. Closing that gap is exactly what sandboxed build systems do by making undeclared reads fail.

## License

[MIT](LICENSE).
