//! The DAG.
//!
//! Tasks are nodes. A `needs` entry is an edge "must run before". From that we
//! can do the two things a build system exists to do: refuse to run if the
//! graph has a cycle, and find work that is independent so it can run at the
//! same time.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::task::Task;

pub struct Graph {
    pub tasks: HashMap<String, Task>,
    /// task -> tasks it depends on.
    deps: HashMap<String, Vec<String>>,
}

impl Graph {
    pub fn new(tasks: Vec<Task>) -> Result<Graph, String> {
        let mut map = HashMap::new();
        for task in tasks {
            if map.insert(task.name.clone(), task).is_some() {
                return Err("two tasks share the same name".into());
            }
        }

        let mut deps = HashMap::new();
        for (name, task) in &map {
            for need in &task.needs {
                if !map.contains_key(need) {
                    return Err(format!(
                        "task `{name}` needs `{need}`, which does not exist"
                    ));
                }
            }
            deps.insert(name.clone(), task.needs.clone());
        }

        Ok(Graph { tasks: map, deps })
    }

    pub fn dependencies(&self, name: &str) -> &[String] {
        self.deps.get(name).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Restrict the graph to `target` and everything it transitively needs.
    pub fn closure(&self, target: &str) -> Result<HashSet<String>, String> {
        if !self.tasks.contains_key(target) {
            return Err(format!("unknown task `{target}`"));
        }
        let mut seen = HashSet::new();
        let mut stack = vec![target.to_string()];
        while let Some(name) = stack.pop() {
            if seen.insert(name.clone()) {
                stack.extend(self.dependencies(&name).iter().cloned());
            }
        }
        Ok(seen)
    }

    /// Group `subset` into waves. Every task in a wave has all of its
    /// dependencies satisfied by earlier waves, so a wave can run in parallel.
    /// Kahn's algorithm, emitting one layer of in-degree-zero nodes at a time.
    pub fn waves(&self, subset: &HashSet<String>) -> Result<Vec<Vec<String>>, String> {
        let mut indegree: HashMap<&str, usize> = HashMap::new();
        for name in subset {
            indegree.entry(name).or_insert(0);
            for dep in self.dependencies(name) {
                if subset.contains(dep) {
                    *indegree.entry(name).or_insert(0) += 1;
                }
            }
        }

        let mut ready: VecDeque<&str> = indegree
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(n, _)| *n)
            .collect();

        // Dependents: who is waiting on me.
        let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
        for name in subset {
            for dep in self.dependencies(name) {
                if subset.contains(dep) {
                    dependents.entry(dep.as_str()).or_default().push(name);
                }
            }
        }

        let mut waves = Vec::new();
        let mut done = 0;
        while !ready.is_empty() {
            let mut wave: Vec<String> = ready.drain(..).map(|s| s.to_string()).collect();
            wave.sort();
            done += wave.len();
            let mut next = VecDeque::new();
            for name in &wave {
                if let Some(children) = dependents.get(name.as_str()) {
                    for child in children {
                        let d = indegree.get_mut(*child).unwrap();
                        *d -= 1;
                        if *d == 0 {
                            next.push_back(*child);
                        }
                    }
                }
            }
            ready = next;
            waves.push(wave);
        }

        if done != subset.len() {
            return Err("the graph has a cycle".into());
        }
        Ok(waves)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::Task;
    use std::path::PathBuf;

    fn task(name: &str, needs: &[&str]) -> Task {
        Task {
            name: name.into(),
            path: PathBuf::from(format!("tasks/{name}.sh")),
            source: Vec::new(),
            needs: needs.iter().map(|s| s.to_string()).collect(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            env: Vec::new(),
        }
    }

    fn graph(tasks: Vec<Task>) -> Graph {
        Graph::new(tasks).unwrap()
    }

    #[test]
    fn waves_group_independent_tasks() {
        let g = graph(vec![task("a", &[]), task("b", &[]), task("c", &["a", "b"])]);
        let subset = g.tasks.keys().cloned().collect();
        let waves = g.waves(&subset).unwrap();

        assert_eq!(waves.len(), 2);
        assert_eq!(waves[0], ["a", "b"]);
        assert_eq!(waves[1], ["c"]);
    }

    #[test]
    fn detects_cycles() {
        let g = graph(vec![task("a", &["b"]), task("b", &["a"])]);
        let subset = g.tasks.keys().cloned().collect();
        assert!(g.waves(&subset).is_err());
    }

    #[test]
    fn rejects_missing_dependency() {
        assert!(Graph::new(vec![task("a", &["ghost"])]).is_err());
    }

    #[test]
    fn closure_is_transitive_and_scoped() {
        let g = graph(vec![
            task("a", &[]),
            task("b", &["a"]),
            task("c", &["b"]),
            task("unrelated", &[]),
        ]);
        let closure = g.closure("c").unwrap();

        assert!(closure.contains("a"));
        assert!(closure.contains("b"));
        assert!(closure.contains("c"));
        assert!(!closure.contains("unrelated"));
    }
}
