//! Dependency graph algorithms.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

/// Shortest path from `from` to `to` following the edges, both ends included.
pub fn find_path<T: Copy + Eq + Hash>(edges: &[(T, T)], from: T, to: T) -> Option<Vec<T>> {
    let mut next: HashMap<T, Vec<T>> = HashMap::new();
    for &(a, b) in edges {
        next.entry(a).or_default().push(b);
    }
    let mut prev = HashMap::from([(from, from)]);
    let mut queue = VecDeque::from([from]);
    while let Some(n) = queue.pop_front() {
        if n == to {
            let mut path = vec![to];
            let mut cur = to;
            while cur != from {
                cur = prev[&cur];
                path.push(cur);
            }
            path.reverse();
            return Some(path);
        }
        for &m in next.get(&n).into_iter().flatten() {
            if let Entry::Vacant(e) = prev.entry(m) {
                e.insert(n);
                queue.push_back(m);
            }
        }
    }
    None
}

/// Whether following the edges never comes back to where it started.
pub fn acyclic<T: Copy + Eq + Hash>(edges: &[(T, T)]) -> bool {
    // Takes away what nothing leads to until nothing is left, or only circles and what they lead to.
    let (mut leading, mut next): (HashMap<T, usize>, HashMap<T, Vec<T>>) = (HashMap::new(), HashMap::new());
    for &(a, b) in edges {
        next.entry(a).or_default().push(b);
        leading.entry(a).or_default();
        *leading.entry(b).or_default() += 1;
    }
    let mut free: Vec<T> = leading.iter().filter(|(_, n)| **n == 0).map(|(t, _)| *t).collect();
    let mut left = leading.len();
    while let Some(t) = free.pop() {
        left -= 1;
        for m in next.get(&t).into_iter().flatten() {
            let n = leading.get_mut(m).expect("counted");
            *n -= 1;
            if *n == 0 {
                free.push(*m);
            }
        }
    }
    left == 0
}

#[cfg(test)]
mod tests {
    use super::{acyclic, find_path};

    #[test]
    fn tells_a_graph_with_a_circle() {
        assert!(acyclic(&[(1, 2), (2, 3), (1, 3), (4, 3)]));
        assert!(acyclic::<i32>(&[]));
        assert!(!acyclic(&[(1, 2), (2, 3), (3, 1), (4, 1)]));
        assert!(!acyclic(&[(1, 1)]));
    }

    #[test]
    fn finds_the_shortest_path() {
        let edges = [(1, 2), (2, 3), (3, 4), (1, 4)];
        assert_eq!(find_path(&edges, 1, 4), Some(vec![1, 4]));
        assert_eq!(find_path(&edges, 2, 4), Some(vec![2, 3, 4]));
        assert_eq!(find_path(&edges, 4, 1), None);
        assert_eq!(find_path(&edges, 3, 3), Some(vec![3]));
    }
}
