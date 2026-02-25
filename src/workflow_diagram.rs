use std::collections::{HashMap, HashSet};

use crate::workflow::WorkflowDefinition;

const WILDCARD_STATE: &str = "*";

pub fn render(workflow: &WorkflowDefinition) -> Vec<String> {
    let (graph, wildcard_targets) = build_graph(workflow);
    let mut lines = Vec::new();
    let mut expanded = HashSet::new();
    let mut stack = vec![workflow.initial_state.clone()];

    lines.push(format!("start: {}", workflow.initial_state));
    lines.push("flow:".to_string());
    lines.push(format!(
        "  {}",
        format_state(workflow, &workflow.initial_state)
    ));

    expanded.insert(workflow.initial_state.clone());
    render_children(
        workflow,
        &workflow.initial_state,
        "  ",
        &graph,
        &mut expanded,
        &mut stack,
        &mut lines,
    );

    if !wildcard_targets.is_empty() {
        lines.push("global transitions:".to_string());
        for target in wildcard_targets {
            lines.push(format!("  * -> {}", format_state(workflow, &target)));
            expanded.insert(target);
        }
    }

    let mut unreachable = workflow
        .states
        .iter()
        .filter(|state| !expanded.contains(*state))
        .cloned()
        .collect::<Vec<_>>();
    unreachable.sort();
    unreachable.dedup();
    if !unreachable.is_empty() {
        lines.push(format!("unreachable states: {}", unreachable.join(", ")));
    }

    lines
}

fn build_graph(workflow: &WorkflowDefinition) -> (HashMap<String, Vec<String>>, Vec<String>) {
    let mut graph = HashMap::<String, Vec<String>>::new();
    let mut wildcard_targets = Vec::new();

    for state in &workflow.states {
        graph.insert(state.clone(), Vec::new());
    }

    for transition in &workflow.transitions {
        if transition.from == WILDCARD_STATE {
            wildcard_targets.push(transition.to.clone());
            continue;
        }
        graph
            .entry(transition.from.clone())
            .or_default()
            .push(transition.to.clone());
    }

    for targets in graph.values_mut() {
        targets.sort();
        targets.dedup();
    }
    wildcard_targets.sort();
    wildcard_targets.dedup();

    (graph, wildcard_targets)
}

fn render_children(
    workflow: &WorkflowDefinition,
    current: &str,
    prefix: &str,
    graph: &HashMap<String, Vec<String>>,
    expanded: &mut HashSet<String>,
    stack: &mut Vec<String>,
    lines: &mut Vec<String>,
) {
    let Some(children) = graph.get(current) else {
        return;
    };
    for (index, child) in children.iter().enumerate() {
        let is_last = index + 1 == children.len();
        let branch = if is_last { "└─" } else { "├─" };
        let child_prefix = if is_last { "   " } else { "│  " };

        if stack.iter().any(|state| state == child) {
            lines.push(format!("{prefix}{branch} ↺ {child}"));
            continue;
        }

        if expanded.contains(child) {
            lines.push(format!(
                "{prefix}{branch} ↪ {}",
                format_state(workflow, child)
            ));
            continue;
        }

        lines.push(format!(
            "{prefix}{branch} {}",
            format_state(workflow, child)
        ));
        expanded.insert(child.clone());
        stack.push(child.clone());
        render_children(
            workflow,
            child,
            &format!("{prefix}{child_prefix}"),
            graph,
            expanded,
            stack,
            lines,
        );
        stack.pop();
    }
}

fn format_state(workflow: &WorkflowDefinition, state: &str) -> String {
    if workflow.is_terminal_state(state) {
        return format!("{state} [terminal]");
    }
    state.to_string()
}
