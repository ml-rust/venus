use std::collections::HashMap;
use venus_core::graph::CellId;

/// Check if `dep_id` is a transitive dependency of `target_id`.
pub fn is_transitive_dependency(
    dep_id: CellId,
    target_id: CellId,
    deps: &HashMap<CellId, Vec<CellId>>,
) -> bool {
    let mut visited = std::collections::HashSet::new();
    let mut stack = vec![target_id];

    while let Some(current) = stack.pop() {
        if !visited.insert(current) {
            continue;
        }

        if let Some(current_deps) = deps.get(&current) {
            if current_deps.contains(&dep_id) {
                return true;
            }
            stack.extend(current_deps.iter().copied());
        }
    }

    false
}
