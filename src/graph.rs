//! The DAG.
//!
//! Tasks are nodes, keyed by their script path. A `needs` entry is an edge
//! "must run before". From that we refuse cycles, restrict the build to a
//! target and its dependencies, and group independent work into waves.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::task::Task;

pub struct Graph {
    pub tasks: HashMap<String, Task>,
    deps: HashMap<String, Vec<String>>,
}

impl Graph {
    pub fn new(tasks: Vec<Task>) -> Result<Graph, String> {
        let mut map = HashMap::new();
        for task in tasks {
            map.insert(task.id.clone(), task);
        }

        let mut deps = HashMap::new();
        for (id, task) in &map {
            for need in &task.needs {
                if !map.contains_key(need) {
                    return Err(format!("task `{id}` needs `{need}`, which was not loaded"));
                }
            }
            deps.insert(id.clone(), task.needs.clone());
        }

        Ok(Graph { tasks: map, deps })
    }

    pub fn dependencies(&self, id: &str) -> &[String] {
        self.deps.get(id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Tasks that nothing else depends on. Used as the default build targets.
    pub fn roots(&self) -> Vec<String> {
        let mut depended_on = HashSet::new();
        for needs in self.deps.values() {
            for need in needs {
                depended_on.insert(need.clone());
            }
        }
        let mut roots: Vec<String> = self
            .tasks
            .keys()
            .filter(|id| !depended_on.contains(*id))
            .cloned()
            .collect();
        roots.sort();
        roots
    }

    pub fn closure(&self, target: &str) -> Result<HashSet<String>, String> {
        if !self.tasks.contains_key(target) {
            return Err(format!("unknown task `{target}`"));
        }
        let mut seen = HashSet::new();
        let mut stack = vec![target.to_string()];
        while let Some(id) = stack.pop() {
            if seen.insert(id.clone()) {
                stack.extend(self.dependencies(&id).iter().cloned());
            }
        }
        Ok(seen)
    }

    /// Group `subset` into waves of independent tasks (Kahn's algorithm).
    pub fn waves(&self, subset: &HashSet<String>) -> Result<Vec<Vec<String>>, String> {
        let mut indegree: HashMap<&str, usize> = HashMap::new();
        for id in subset {
            indegree.entry(id).or_insert(0);
            for dep in self.dependencies(id) {
                if subset.contains(dep) {
                    *indegree.entry(id).or_insert(0) += 1;
                }
            }
        }

        let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
        for id in subset {
            for dep in self.dependencies(id) {
                if subset.contains(dep) {
                    dependents.entry(dep.as_str()).or_default().push(id);
                }
            }
        }

        let mut ready: VecDeque<&str> = indegree
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(n, _)| *n)
            .collect();

        let mut waves = Vec::new();
        let mut done = 0;
        while !ready.is_empty() {
            let mut wave: Vec<String> = ready.drain(..).map(|s| s.to_string()).collect();
            wave.sort();
            done += wave.len();
            let mut next = VecDeque::new();
            for id in &wave {
                if let Some(children) = dependents.get(id.as_str()) {
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
    use std::path::PathBuf;

    fn task(id: &str, needs: &[&str]) -> Task {
        Task {
            id: id.into(),
            path: PathBuf::from(id),
            source: Vec::new(),
            needs: needs.iter().map(|s| s.to_string()).collect(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            env: Vec::new(),
            tools: Vec::new(),
        }
    }

    fn graph(tasks: Vec<Task>) -> Graph {
        Graph::new(tasks).unwrap()
    }

    #[test]
    fn waves_group_independent_tasks() {
        let g = graph(vec![
            task("a.sh", &[]),
            task("b.sh", &[]),
            task("c.sh", &["a.sh", "b.sh"]),
        ]);
        let subset = g.tasks.keys().cloned().collect();
        let waves = g.waves(&subset).unwrap();

        assert_eq!(waves.len(), 2);
        assert_eq!(waves[0], ["a.sh", "b.sh"]);
        assert_eq!(waves[1], ["c.sh"]);
    }

    #[test]
    fn detects_cycles() {
        let g = graph(vec![task("a.sh", &["b.sh"]), task("b.sh", &["a.sh"])]);
        let subset = g.tasks.keys().cloned().collect();
        assert!(g.waves(&subset).is_err());
    }

    #[test]
    fn rejects_missing_dependency() {
        assert!(Graph::new(vec![task("a.sh", &["ghost.sh"])]).is_err());
    }

    #[test]
    fn roots_are_tasks_with_no_dependents() {
        let g = graph(vec![
            task("a.sh", &[]),
            task("b.sh", &["a.sh"]),
            task("c.sh", &["b.sh"]),
        ]);
        assert_eq!(g.roots(), ["c.sh"]);
    }

    #[test]
    fn closure_is_transitive_and_scoped() {
        let g = graph(vec![
            task("a.sh", &[]),
            task("b.sh", &["a.sh"]),
            task("c.sh", &["b.sh"]),
            task("unrelated.sh", &[]),
        ]);
        let closure = g.closure("c.sh").unwrap();
        assert!(closure.contains("a.sh"));
        assert!(closure.contains("b.sh"));
        assert!(closure.contains("c.sh"));
        assert!(!closure.contains("unrelated.sh"));
    }
}
