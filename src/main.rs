//! tinybuild: a tiny build system in a few hundred lines of Rust.
//!
//! It exists to make one idea concrete: a build system is a DAG of cacheable
//! actions. Here an action is a shell script that declares, in header comments,
//! what it needs, what it reads, what it writes, and which environment values
//! affect it. From those declarations tinybuild builds a graph, runs
//! independent work in parallel, and skips anything whose inputs have not
//! changed.

mod cache;
mod graph;
mod runner;
mod task;

use std::path::PathBuf;
use std::process::exit;

use graph::Graph;

fn main() {
    if let Err(err) = real_main() {
        eprintln!("error: {err}");
        exit(1);
    }
}

fn real_main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let workspace = std::env::current_dir().map_err(|e| e.to_string())?;
    let tasks_dir = workspace.join("tasks");

    let command = args.first().map(String::as_str).unwrap_or("run");

    match command {
        "run" => {
            let target = args.get(1).map(String::as_str);
            let tasks = task::load(&tasks_dir)?;
            let graph = Graph::new(tasks)?;
            let outcomes = runner::run(&graph, target, &workspace)?;
            let hits = outcomes.iter().filter(|o| o.hit).count();
            println!(
                "\n{} task(s): {} run, {} cached",
                outcomes.len(),
                outcomes.len() - hits,
                hits
            );
            Ok(())
        }
        "graph" => {
            let tasks = task::load(&tasks_dir)?;
            let graph = Graph::new(tasks)?;
            print_graph(&graph)
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

fn print_graph(graph: &Graph) -> Result<(), String> {
    let subset = graph.tasks.keys().cloned().collect();
    let waves = graph.waves(&subset)?;
    for (i, wave) in waves.iter().enumerate() {
        println!("wave {i} (runs in parallel):");
        for name in wave {
            let deps = graph.dependencies(name);
            if deps.is_empty() {
                println!("  {name}");
            } else {
                println!("  {name}  <- {}", deps.join(", "));
            }
        }
    }
    Ok(())
}

fn print_help() {
    let exe = PathBuf::from(std::env::args().next().unwrap_or_default());
    let name = exe
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("tinybuild");
    println!(
        "{name} - a tiny build system\n\n\
         USAGE:\n\
         \x20 {name} run [TASK]   run TASK and its dependencies (default: everything)\n\
         \x20 {name} graph        print the execution waves\n\
         \x20 {name} help         show this message\n\n\
         Tasks are *.sh files in ./tasks that declare their contract in headers:\n\
         \x20 # tinybuild needs other-task\n\
         \x20 # tinybuild input src/**/*.txt\n\
         \x20 # tinybuild output build/out.txt\n\
         \x20 # tinybuild env NAME"
    );
}
