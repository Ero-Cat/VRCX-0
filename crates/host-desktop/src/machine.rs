//! Machine-level identity surfaced to remote sync presence.

/// The OS-reported hostname, empty when the platform cannot provide one.
pub fn machine_hostname() -> String {
    sysinfo::System::host_name().unwrap_or_default()
}
