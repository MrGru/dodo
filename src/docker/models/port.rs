//! A published port mapping and how it is written in the Ports column.
//!
//! The engine lists a mapping once per address family, so the same
//! `host → container` pair often arrives twice (IPv4 and IPv6); [`format_ports`]
//! collapses those. Only published ports (those with a host port) are shown —
//! an exposed-but-unpublished port is not a mapping a user can reach.

/// One published port: a host port forwarded to a container port over a
/// protocol.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PortMapping {
    /// The host-side port. `None` means the container port is exposed but not
    /// published to the host, so it is not rendered.
    pub host: Option<u16>,
    pub container: u16,
    /// The transport protocol as the engine reports it (`"tcp"`, `"udp"`,
    /// `"sctp"`). Kept verbatim; it is a wire token, not prose.
    pub protocol: String,
}

/// Renders the published mappings as `host→container/proto`, de-duplicated and
/// joined with commas. Returns an empty string when nothing is published, which
/// the view shows as a dash.
pub fn format_ports(ports: &[PortMapping]) -> String {
    let mut seen: Vec<String> = Vec::new();
    for port in ports {
        // Only published ports are reachable, so only those are shown.
        let Some(host) = port.host else {
            continue;
        };
        let rendered = format!("{host}→{}/{}", port.container, port.protocol);
        if !seen.contains(&rendered) {
            seen.push(rendered);
        }
    }
    seen.join(", ")
}

#[cfg(test)]
mod tests {
    use super::{PortMapping, format_ports};

    fn mapping(host: Option<u16>, container: u16, proto: &str) -> PortMapping {
        PortMapping {
            host,
            container,
            protocol: proto.to_string(),
        }
    }

    #[test]
    fn a_published_port_reads_host_to_container() {
        let ports = vec![mapping(Some(8080), 80, "tcp")];
        assert_eq!(format_ports(&ports), "8080→80/tcp");
    }

    #[test]
    fn multiple_distinct_mappings_are_joined() {
        let ports = vec![
            mapping(Some(1025), 1025, "tcp"),
            mapping(Some(1080), 1080, "tcp"),
        ];
        assert_eq!(format_ports(&ports), "1025→1025/tcp, 1080→1080/tcp");
    }

    #[test]
    fn duplicate_mappings_from_two_address_families_collapse() {
        let ports = vec![
            mapping(Some(5432), 5432, "tcp"),
            mapping(Some(5432), 5432, "tcp"),
        ];
        assert_eq!(format_ports(&ports), "5432→5432/tcp");
    }

    #[test]
    fn unpublished_ports_are_omitted() {
        let ports = vec![mapping(None, 6379, "tcp"), mapping(Some(80), 80, "tcp")];
        assert_eq!(format_ports(&ports), "80→80/tcp");
    }

    #[test]
    fn nothing_published_is_empty() {
        assert_eq!(format_ports(&[]), "");
        assert_eq!(format_ports(&[mapping(None, 80, "tcp")]), "");
    }
}
