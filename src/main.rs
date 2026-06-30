//! tinybuild: a tiny build system in a few hundred lines of Rust.
//!
//! It makes one idea concrete: a build system is a DAG of cacheable actions.
//! An action is a shell script that declares, in header comments, what it
//! depends on, what it reads, what it writes, the tools it uses, and the
//! environment it cares about. From those declarations tinybuild builds a
//! graph, runs each task hermetically, and content-addresses the result so
//! unchanged work is skipped, locally or pulled from a shared store.

mod build;
mod graph;
mod store;
mod task;

use std::path::PathBuf;
use std::process::exit;

use graph::Graph;
use store::Store;

fn main() {
    if let Err(err) = real_main() {
        eprintln!("error: {err}");
        exit(1);
    }
}

struct Options {
    command: String,
    target: Option<String>,
    store: PathBuf,
    substituter: Option<PathBuf>,
}

fn parse_args() -> Options {
    let mut command = "run".to_string();
    let mut target = None;
    let mut store = default_store();
    let mut substituter = None;

    let mut args = std::env::args().skip(1).peekable();
    let mut positional = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--store" => {
                if let Some(v) = args.next() {
                    store = PathBuf::from(v);
                }
            }
            "--substituter" => substituter = args.next().map(PathBuf::from),
            _ => positional.push(arg),
        }
    }
    if let Some(first) = positional.first() {
        command = first.clone();
    }
    if let Some(second) = positional.get(1) {
        target = Some(second.clone());
    }

    Options {
        command,
        target,
        store,
        substituter,
    }
}

fn default_store() -> PathBuf {
    if let Ok(dir) = std::env::var("TINYBUILD_STORE") {
        return PathBuf::from(dir);
    }
    let base = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".cache")
        });
    base.join("tinybuild").join("store")
}

fn real_main() -> Result<(), String> {
    let opts = parse_args();
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    let tasks_dir = root.join("tasks");

    match opts.command.as_str() {
        "run" => {
            let store = Store::open(opts.store, opts.substituter)?;
            let graph = load_graph(&root, &tasks_dir)?;
            let out_dir = root.join("out");
            let summary = build::run(&graph, opts.target.as_deref(), &root, &store, &out_dir)?;
            write_roots(&store, &summary.keys)?;
            println!(
                "\n{} task(s): {} run, {} cached -> {}",
                summary.ran + summary.cached,
                summary.ran,
                summary.cached,
                out_dir.display()
            );
            Ok(())
        }
        "plan" => {
            let graph = load_graph(&root, &tasks_dir)?;
            println!("{}", build::plan(&graph, opts.target.as_deref())?);
            Ok(())
        }
        "graph" => {
            let graph = load_graph(&root, &tasks_dir)?;
            print_graph(&graph)
        }
        "gc" => {
            let store = Store::open(opts.store, opts.substituter)?;
            let roots = read_roots(&store);
            let (actions, blobs) = store.gc(&roots)?;
            println!("removed {actions} action(s) and {blobs} blob(s)");
            Ok(())
        }
        "help" | "-h" | "--help" => {
            print_help();
            Ok(())
        }
        other => {
            print_help();
            Err(format!("unknown command `{other}`"))
        }
    }
}

fn load_graph(root: &std::path::Path, tasks_dir: &std::path::Path) -> Result<Graph, String> {
    Graph::new(task::load(root, tasks_dir)?)
}

fn roots_path(store: &Store) -> PathBuf {
    store.root.join("roots.json")
}

fn write_roots(store: &Store, keys: &[String]) -> Result<(), String> {
    let data = serde_json::to_vec(keys).map_err(|e| e.to_string())?;
    std::fs::write(roots_path(store), data).map_err(|e| e.to_string())
}

fn read_roots(store: &Store) -> Vec<String> {
    std::fs::read(roots_path(store))
        .ok()
        .and_then(|d| serde_json::from_slice(&d).ok())
        .unwrap_or_default()
}

fn print_graph(graph: &Graph) -> Result<(), String> {
    let subset = graph.tasks.keys().cloned().collect();
    let waves = graph.waves(&subset)?;
    for (i, wave) in waves.iter().enumerate() {
        println!("wave {i} (runs in parallel):");
        for id in wave {
            let deps = graph.dependencies(id);
            if deps.is_empty() {
                println!("  {id}");
            } else {
                println!("  {id}  <- {}", deps.join(", "));
            }
        }
    }
    Ok(())
}

fn print_help() {
    println!(
        "tinybuild - a tiny build system\n\n\
         USAGE:\n\
         \x20 tinybuild run [TASK]    build TASK and its dependencies (default: all roots)\n\
         \x20 tinybuild plan [TASK]   print the build plan as JSON\n\
         \x20 tinybuild graph         print the execution waves\n\
         \x20 tinybuild gc            drop store entries the last build cannot reach\n\
         \x20 tinybuild help          show this message\n\n\
         OPTIONS:\n\
         \x20 --store PATH            content-addressed store (default: ~/.cache/tinybuild/store)\n\
         \x20 --substituter PATH      a remote store to pull from and push to\n\n\
         Tasks are *.sh files in ./tasks that declare their contract in headers:\n\
         \x20 # tinybuild needs ./other-task.sh\n\
         \x20 # tinybuild input src/**/*.txt\n\
         \x20 # tinybuild output out.txt\n\
         \x20 # tinybuild env NAME\n\
         \x20 # tinybuild tool swiftc --version"
    );
}
