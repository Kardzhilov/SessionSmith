//! Registration and termination of child processes owned by a job.

use std::{
    collections::BTreeMap,
    process::{Child, Command},
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChildSnapshot {
    pub pid: u32,
    pub label: String,
}

/// Tracks child processes that must be terminated when their job is cancelled.
#[derive(Clone, Default)]
pub struct ChildRegistry {
    children: Arc<Mutex<BTreeMap<u32, ChildSnapshot>>>,
}

impl ChildRegistry {
    pub fn register(&self, child: &Child, label: impl Into<String>) -> ChildRegistration {
        self.register_pid(child.id(), label)
    }

    /// Register a PID when the caller owns the corresponding child handle.
    /// Keep the returned registration alive until that child has been reaped.
    pub fn register_pid(&self, pid: u32, label: impl Into<String>) -> ChildRegistration {
        let child = ChildSnapshot {
            pid,
            label: label.into(),
        };
        self.children
            .lock()
            .expect("child registry mutex poisoned")
            .insert(pid, child);
        ChildRegistration {
            registry: self.clone(),
            pid,
        }
    }

    pub fn active_children(&self) -> Vec<ChildSnapshot> {
        self.children
            .lock()
            .expect("child registry mutex poisoned")
            .values()
            .cloned()
            .collect()
    }

    /// Request termination for every registered process. On Unix, commands
    /// configured with [`configure_command`] are terminated as process groups.
    pub fn kill_all(&self) -> usize {
        let pids: Vec<_> = self
            .children
            .lock()
            .expect("child registry mutex poisoned")
            .keys()
            .copied()
            .collect();
        for pid in &pids {
            terminate(*pid);
        }
        pids.len()
    }

    fn remove(&self, pid: u32) {
        self.children
            .lock()
            .expect("child registry mutex poisoned")
            .remove(&pid);
    }
}

/// Deregisters a child when it is dropped after the caller reaps that child.
#[must_use = "keep the registration alive until the child has been reaped"]
pub struct ChildRegistration {
    registry: ChildRegistry,
    pid: u32,
}

impl Drop for ChildRegistration {
    fn drop(&mut self) {
        self.registry.remove(self.pid);
    }
}

/// Configure a command so cancellation can terminate its complete process tree.
#[cfg(unix)]
pub fn configure_command(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    command.process_group(0);
}

/// Windows' taskkill handles descendants directly, so no command setup is needed.
#[cfg(windows)]
pub fn configure_command(_: &mut Command) {}

#[cfg(unix)]
fn terminate(pid: u32) {
    let group = format!("-{pid}");
    let killed_group = Command::new("kill")
        .args(["-TERM", "--", &group])
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if !killed_group {
        let _ = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status();
    }
}

#[cfg(windows)]
fn terminate(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status();
}

#[cfg(not(any(unix, windows)))]
fn terminate(_: u32) {}

#[cfg(test)]
mod tests {
    use super::ChildRegistry;

    #[test]
    fn registration_is_removed_when_the_guard_drops() {
        let registry = ChildRegistry::default();
        let registration = registry.register_pid(42, "test child");
        assert_eq!(registry.active_children().len(), 1);

        drop(registration);
        assert!(registry.active_children().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn kill_all_terminates_a_configured_process_group() {
        use std::process::Command;

        let registry = ChildRegistry::default();
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 30"]);
        super::configure_command(&mut command);
        let mut child = command.spawn().expect("sleep process should start");
        let registration = registry.register(&child, "test process group");

        assert_eq!(registry.kill_all(), 1);
        assert!(!child.wait().expect("child should be reaped").success());
        drop(registration);
        assert!(registry.active_children().is_empty());
    }
}
