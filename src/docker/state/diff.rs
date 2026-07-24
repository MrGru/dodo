//! The pure row-diff helpers background polling leans on (round 4).
//!
//! Auto-refresh must not blank or wholesale-replace a table it is only nudging.
//! Two things make that possible and both are plain data, so they live here and
//! are unit-tested without GPUI:
//!
//! - [`carry_cpu`] forward-fills each surviving container's last CPU reading onto
//!   a freshly-listed set. `list_containers` always reports `cpu_percent: None`
//!   (the value is measured separately, per row), so without this every poll
//!   would flash the CPU column back to a dash until the sweep refilled it.
//! - [`rows_changed`] reports whether a freshly-listed set differs from the one
//!   on screen. A poll that changed nothing skips its `cx.notify()` — no
//!   re-render, no wasted work — which is the whole point of diffing rather than
//!   replacing.

use std::collections::HashMap;

use crate::docker::models::container::Container;

/// Forward-fills each incoming row's CPU percent from the current set, keyed by
/// id. A row with no match in the current set (a newly-appeared container) keeps
/// its incoming value (`None`), so the sweep measures it fresh.
pub fn carry_cpu(current: &[Container], incoming: &mut [Container]) {
    if current.is_empty() {
        return;
    }
    let previous: HashMap<&str, Option<f64>> = current
        .iter()
        .map(|row| (row.id.as_str(), row.cpu_percent))
        .collect();
    for row in incoming.iter_mut() {
        if let Some(&cpu) = previous.get(row.id.as_str()) {
            row.cpu_percent = cpu;
        }
    }
}

/// Whether two display-ordered row sets differ by value. Both slices are assumed
/// already sorted the same way, so this is a straight element-wise compare — the
/// signal a poll uses to decide whether a re-render is warranted.
pub fn rows_changed<T: PartialEq>(current: &[T], incoming: &[T]) -> bool {
    current != incoming
}

#[cfg(test)]
mod tests {
    use super::{carry_cpu, rows_changed};
    use crate::docker::models::container::Container;
    use crate::docker::models::status::ContainerStatus;

    fn container(id: &str, status: ContainerStatus, cpu: Option<f64>) -> Container {
        Container {
            id: id.into(),
            name: format!("name-{id}"),
            image: "img".into(),
            status,
            ports: Vec::new(),
            compose_project: None,
            started_at: None,
            cpu_percent: cpu,
        }
    }

    #[test]
    fn carry_cpu_forward_fills_surviving_rows() {
        let current = vec![
            container("1", ContainerStatus::Running, Some(12.5)),
            container("2", ContainerStatus::Running, Some(3.0)),
        ];
        // A fresh list reports None for CPU on every row, plus a new row "3".
        let mut incoming = vec![
            container("1", ContainerStatus::Running, None),
            container("2", ContainerStatus::Running, None),
            container("3", ContainerStatus::Running, None),
        ];
        carry_cpu(&current, &mut incoming);
        assert_eq!(incoming[0].cpu_percent, Some(12.5));
        assert_eq!(incoming[1].cpu_percent, Some(3.0));
        // The brand-new row has no prior reading to carry.
        assert_eq!(incoming[2].cpu_percent, None);
    }

    #[test]
    fn carry_cpu_is_a_noop_against_an_empty_current_set() {
        let mut incoming = vec![container("1", ContainerStatus::Running, None)];
        carry_cpu(&[], &mut incoming);
        assert_eq!(incoming[0].cpu_percent, None);
    }

    #[test]
    fn rows_changed_only_when_values_differ() {
        let a = vec![container("1", ContainerStatus::Running, Some(1.0))];
        let same = vec![container("1", ContainerStatus::Running, Some(1.0))];
        assert!(!rows_changed(&a, &same));

        // Status flip is a change.
        let status = vec![container("1", ContainerStatus::Exited, Some(1.0))];
        assert!(rows_changed(&a, &status));

        // A row added is a change.
        let added = vec![
            container("1", ContainerStatus::Running, Some(1.0)),
            container("2", ContainerStatus::Running, None),
        ];
        assert!(rows_changed(&a, &added));
    }
}
