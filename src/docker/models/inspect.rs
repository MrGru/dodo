//! The read-only detail an Inspect opens: a handful of key fields plus the
//! engine's own JSON, for any of the four resource types.
//!
//! # Why this reads JSON rather than a `bollard` type
//!
//! The four inspect endpoints return four unrelated wire structs
//! (`ContainerInspectResponse`, `ImageInspect`, `Volume`, `NetworkInspect`).
//! Extracting "the fields worth showing" from each of them in
//! [`services`](crate::docker::services) would put four screens' worth of
//! presentation logic in the one module that may name `bollard`, where none of it
//! could be tested without a daemon.
//!
//! So the service does the one thing only it can — call the endpoint and
//! `serde_json::to_value` the response — and everything after that happens here,
//! against a plain [`serde_json::Value`]. The panel needs the raw JSON anyway, so
//! this costs nothing extra, keeps `bollard` out of the models exactly as the
//! module contract requires, and makes every field rule unit testable from a
//! literal JSON document. `api_explorer::models::json_tree` takes the same view
//! of a response body.
//!
//! Values stay untranslated data ([`FieldValue::Text`]); only the *labels* are
//! [`Str`]s, and the two shapes a value can take that are language —
//! a boolean and "the engine did not report this" — are their own variants so
//! the view translates them.

use serde_json::Value;

use crate::docker::models::size::format_size;
use crate::i18n::Str;

/// Which resource an [`InspectDetail`] describes. Chooses the field set and the
/// title rule.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InspectKind {
    Container,
    Image,
    Volume,
    Network,
}

/// One value in the detail's field list.
#[derive(Clone, PartialEq, Debug)]
pub enum FieldValue {
    /// Engine data shown verbatim — an id, a path, a timestamp, a size. Not
    /// language, so it is never translated.
    Text(String),
    /// A flag the view renders as the localized Yes/No.
    Flag(bool),
    /// The engine did not report this field; the view renders the localized
    /// `N/A`. Kept as a row rather than dropped so the field list has the same
    /// shape for every resource of a kind.
    Missing,
}

/// One labelled field of a detail view. No `PartialEq`/`Debug`: [`Str`] is a
/// plain translation key with neither.
#[derive(Clone)]
pub struct InspectField {
    pub label: Str,
    pub value: FieldValue,
}

/// A resource's full detail: the key fields, and the engine's response
/// pretty-printed for the raw JSON pane.
#[derive(Clone)]
pub struct InspectDetail {
    /// The resource's own name, shown beside the panel title. Empty when the
    /// engine reported none, in which case the view falls back to the row's name.
    pub title: String,
    pub fields: Vec<InspectField>,
    /// The whole inspect response, pretty-printed.
    pub json: String,
}

impl InspectDetail {
    /// Reduces one inspect response to what the panel shows.
    pub fn from_value(kind: InspectKind, value: &Value) -> Self {
        let fields = match kind {
            InspectKind::Container => container_fields(value),
            InspectKind::Image => image_fields(value),
            InspectKind::Volume => volume_fields(value),
            InspectKind::Network => network_fields(value),
        };
        InspectDetail {
            title: title(kind, value),
            fields,
            json: serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()),
        }
    }
}

/// The resource's own name as the panel header shows it.
fn title(kind: InspectKind, value: &Value) -> String {
    match kind {
        // A container name is stored with a historic leading slash.
        InspectKind::Container => container_name(value),
        // An image has no name of its own: its first tag, else its short id.
        InspectKind::Image => first_string(value, "/RepoTags")
            .unwrap_or_else(|| short_id(text(value, "/Id").unwrap_or_default())),
        InspectKind::Volume | InspectKind::Network => text(value, "/Name").unwrap_or_default(),
    }
}

fn container_fields(value: &Value) -> Vec<InspectField> {
    vec![
        field(Str::DockerFieldId, id_value(value, "/Id")),
        field(Str::DockerColumnName, opt_text(container_name(value))),
        field(
            Str::DockerColumnImage,
            value_or_missing(value, "/Config/Image"),
        ),
        field(
            Str::DockerColumnStatus,
            value_or_missing(value, "/State/Status"),
        ),
        field(Str::DockerFieldExitCode, number(value, "/State/ExitCode")),
        field(
            Str::DockerColumnCreated,
            value_or_missing(value, "/Created"),
        ),
        field(
            Str::DockerFieldStarted,
            value_or_missing(value, "/State/StartedAt"),
        ),
        field(Str::DockerFieldCommand, opt_text(command(value))),
        field(
            Str::DockerFieldRestartPolicy,
            value_or_missing(value, "/HostConfig/RestartPolicy/Name"),
        ),
        field(
            Str::Networks,
            opt_text(keys(value, "/NetworkSettings/Networks").join(", ")),
        ),
        field(Str::DockerFieldIpAddress, opt_text(ip_address(value))),
        field(Str::DockerColumnPorts, opt_text(ports(value))),
        field(Str::DockerFieldMounts, opt_text(mounts(value).join(", "))),
    ]
}

fn image_fields(value: &Value) -> Vec<InspectField> {
    vec![
        field(Str::DockerFieldId, id_value(value, "/Id")),
        field(
            Str::DockerFieldTags,
            opt_text(strings(value, "/RepoTags").join(", ")),
        ),
        field(
            Str::DockerFieldDigest,
            opt_text(first_string(value, "/RepoDigests").unwrap_or_default()),
        ),
        field(
            Str::DockerColumnCreated,
            value_or_missing(value, "/Created"),
        ),
        field(Str::DockerColumnSize, size(value, "/Size")),
        field(
            Str::DockerFieldArchitecture,
            value_or_missing(value, "/Architecture"),
        ),
        field(Str::DockerFieldOs, value_or_missing(value, "/Os")),
        field(
            Str::DockerFieldLayers,
            count(
                value
                    .pointer("/RootFS/Layers")
                    .and_then(Value::as_array)
                    .map(Vec::len),
            ),
        ),
        field(
            Str::DockerFieldCommand,
            opt_text(strings(value, "/Config/Cmd").join(" ")),
        ),
    ]
}

fn volume_fields(value: &Value) -> Vec<InspectField> {
    vec![
        field(Str::DockerColumnName, value_or_missing(value, "/Name")),
        field(Str::DockerColumnDriver, value_or_missing(value, "/Driver")),
        field(
            Str::DockerColumnMountPoint,
            value_or_missing(value, "/Mountpoint"),
        ),
        field(
            Str::DockerColumnCreated,
            value_or_missing(value, "/CreatedAt"),
        ),
        field(Str::DockerColumnScope, value_or_missing(value, "/Scope")),
        field(Str::DockerColumnSize, size(value, "/UsageData/Size")),
        field(
            Str::DockerFieldLabels,
            opt_text(pairs(value, "/Labels").join(", ")),
        ),
        field(
            Str::DockerFieldOptions,
            opt_text(pairs(value, "/Options").join(", ")),
        ),
    ]
}

fn network_fields(value: &Value) -> Vec<InspectField> {
    vec![
        field(Str::DockerFieldId, id_value(value, "/Id")),
        field(Str::DockerColumnName, value_or_missing(value, "/Name")),
        field(Str::DockerColumnDriver, value_or_missing(value, "/Driver")),
        field(Str::DockerColumnScope, value_or_missing(value, "/Scope")),
        field(
            Str::DockerColumnCreated,
            value_or_missing(value, "/Created"),
        ),
        field(Str::DockerFieldInternal, flag(value, "/Internal")),
        field(Str::DockerFieldAttachable, flag(value, "/Attachable")),
        field(
            Str::DockerFieldSubnet,
            opt_text(ipam(value, "Subnet").join(", ")),
        ),
        field(
            Str::DockerFieldGateway,
            opt_text(ipam(value, "Gateway").join(", ")),
        ),
        field(
            Str::Containers,
            count(
                value
                    .pointer("/Containers")
                    .and_then(Value::as_object)
                    .map(|map| map.len()),
            ),
        ),
    ]
}

// ---- Extraction helpers ----------------------------------------------------

fn field(label: Str, value: FieldValue) -> InspectField {
    InspectField { label, value }
}

/// A string at `pointer`, or `None` when it is absent, null or not a string.
fn text(value: &Value, pointer: &str) -> Option<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|text| !text.is_empty())
}

/// A field for a string the engine may not have reported.
fn value_or_missing(value: &Value, pointer: &str) -> FieldValue {
    match text(value, pointer) {
        Some(text) => FieldValue::Text(text),
        None => FieldValue::Missing,
    }
}

/// A field for a string this module built: empty means the engine reported
/// nothing to build it from.
fn opt_text(text: String) -> FieldValue {
    if text.is_empty() {
        FieldValue::Missing
    } else {
        FieldValue::Text(text)
    }
}

/// An id shortened the way the tables show it (12 hex characters, no algorithm
/// prefix), so the panel and the row name the same thing.
fn id_value(value: &Value, pointer: &str) -> FieldValue {
    match text(value, pointer) {
        Some(id) => FieldValue::Text(short_id(id)),
        None => FieldValue::Missing,
    }
}

/// The short form of a content-addressable id: the `sha256:` prefix dropped and
/// the first 12 hex characters kept.
pub fn short_id(id: String) -> String {
    let hex = id.split_once(':').map(|(_, hex)| hex).unwrap_or(&id);
    hex.chars().take(12).collect()
}

fn number(value: &Value, pointer: &str) -> FieldValue {
    match value.pointer(pointer).and_then(Value::as_i64) {
        Some(number) => FieldValue::Text(number.to_string()),
        None => FieldValue::Missing,
    }
}

fn count(count: Option<usize>) -> FieldValue {
    match count {
        Some(count) => FieldValue::Text(count.to_string()),
        None => FieldValue::Missing,
    }
}

/// A byte count formatted the way the Size columns do. The engine's `-1`
/// "not calculated" sentinel reads as missing rather than `0B`.
fn size(value: &Value, pointer: &str) -> FieldValue {
    match value.pointer(pointer).and_then(Value::as_i64) {
        Some(bytes) if bytes >= 0 => FieldValue::Text(format_size(bytes)),
        _ => FieldValue::Missing,
    }
}

fn flag(value: &Value, pointer: &str) -> FieldValue {
    match value.pointer(pointer).and_then(Value::as_bool) {
        Some(flag) => FieldValue::Flag(flag),
        None => FieldValue::Missing,
    }
}

/// Every string in the array at `pointer`, skipping non-strings and the
/// engine's `<none>` placeholders.
fn strings(value: &Value, pointer: &str) -> Vec<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .filter(|item| !item.is_empty() && *item != "<none>:<none>")
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn first_string(value: &Value, pointer: &str) -> Option<String> {
    strings(value, pointer).into_iter().next()
}

/// The keys of the object at `pointer`, in the order serde preserved them.
fn keys(value: &Value, pointer: &str) -> Vec<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_object)
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default()
}

/// The object at `pointer` as `key=value` pairs — how labels and driver options
/// read.
fn pairs(value: &Value, pointer: &str) -> Vec<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .map(|(key, value)| match value.as_str() {
                    Some(text) if !text.is_empty() => format!("{key}={text}"),
                    _ => key.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The container name with the historic leading slash removed.
fn container_name(value: &Value) -> String {
    text(value, "/Name")
        .map(|name| name.trim_start_matches('/').to_string())
        .unwrap_or_default()
}

/// The entrypoint and its arguments as one command line.
fn command(value: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(path) = text(value, "/Path") {
        parts.push(path);
    }
    parts.extend(strings(value, "/Args"));
    parts.join(" ")
}

/// The container's IP: the default bridge address when it has one, else the
/// first address across its attached networks.
fn ip_address(value: &Value) -> String {
    if let Some(address) = text(value, "/NetworkSettings/IPAddress") {
        return address;
    }
    value
        .pointer("/NetworkSettings/Networks")
        .and_then(Value::as_object)
        .and_then(|networks| {
            networks
                .values()
                .find_map(|network| text(network, "/IPAddress"))
        })
        .unwrap_or_default()
}

/// The published port bindings as `host→container/proto`, matching the Ports
/// column's notation. Exposed-but-unpublished ports are left out, as there.
fn ports(value: &Value) -> String {
    let Some(map) = value
        .pointer("/NetworkSettings/Ports")
        .and_then(Value::as_object)
    else {
        return String::new();
    };
    let mut rendered: Vec<String> = Vec::new();
    for (port, bindings) in map {
        let Some(bindings) = bindings.as_array() else {
            continue;
        };
        for binding in bindings {
            let Some(host) = text(binding, "/HostPort") else {
                continue;
            };
            let mapping = format!("{host}→{port}");
            if !rendered.contains(&mapping) {
                rendered.push(mapping);
            }
        }
    }
    rendered.join(", ")
}

/// Where each mount lands inside the container, named by its volume where it
/// has one — the same distinction the Volumes page's usage count draws.
fn mounts(value: &Value) -> Vec<String> {
    value
        .pointer("/Mounts")
        .and_then(Value::as_array)
        .map(|mounts| {
            mounts
                .iter()
                .filter_map(|mount| {
                    let destination = text(mount, "/Destination")?;
                    Some(match text(mount, "/Name") {
                        Some(name) => format!("{name}→{destination}"),
                        None => destination,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// One field of every IPAM config entry (`Subnet`, `Gateway`).
fn ipam(value: &Value, field: &str) -> Vec<String> {
    value
        .pointer("/IPAM/Config")
        .and_then(Value::as_array)
        .map(|configs| {
            configs
                .iter()
                .filter_map(|config| text(config, &format!("/{field}")))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{FieldValue, InspectDetail, InspectKind, short_id};
    use crate::i18n::Str;
    use serde_json::json;

    /// The value of the field carrying `label`, for readable assertions.
    fn field(detail: &InspectDetail, label: Str) -> FieldValue {
        detail
            .fields
            .iter()
            .find(|field| std::mem::discriminant(&field.label) == std::mem::discriminant(&label))
            .map(|field| field.value.clone())
            .expect("the field set is missing an expected label")
    }

    fn text(detail: &InspectDetail, label: Str) -> String {
        match field(detail, label) {
            FieldValue::Text(text) => text,
            other => panic!("expected text, got {other:?}"),
        }
    }

    fn container() -> serde_json::Value {
        json!({
            "Id": "sha256:0123456789abcdefdeadbeef",
            "Name": "/web",
            "Created": "2026-07-01T10:00:00Z",
            "Path": "nginx",
            "Args": ["-g", "daemon off;"],
            "State": { "Status": "running", "StartedAt": "2026-07-02T08:00:00Z", "ExitCode": 0 },
            "Config": { "Image": "nginx:latest" },
            "HostConfig": { "RestartPolicy": { "Name": "unless-stopped" } },
            "NetworkSettings": {
                "IPAddress": "",
                "Networks": { "app_default": { "IPAddress": "10.89.0.4" } },
                "Ports": {
                    "80/tcp": [
                        { "HostIp": "0.0.0.0", "HostPort": "8080" },
                        { "HostIp": "::", "HostPort": "8080" }
                    ],
                    "9000/tcp": null
                }
            },
            "Mounts": [
                { "Name": "pgdata", "Destination": "/var/lib/data" },
                { "Destination": "/etc/conf" }
            ]
        })
    }

    #[test]
    fn a_container_detail_carries_its_key_fields() {
        let detail = InspectDetail::from_value(InspectKind::Container, &container());
        assert_eq!(detail.title, "web");
        assert_eq!(text(&detail, Str::DockerFieldId), "0123456789ab");
        assert_eq!(text(&detail, Str::DockerColumnName), "web");
        assert_eq!(text(&detail, Str::DockerColumnImage), "nginx:latest");
        assert_eq!(text(&detail, Str::DockerColumnStatus), "running");
        assert_eq!(
            text(&detail, Str::DockerFieldCommand),
            "nginx -g daemon off;"
        );
        assert_eq!(
            text(&detail, Str::DockerFieldRestartPolicy),
            "unless-stopped"
        );
        assert_eq!(text(&detail, Str::Networks), "app_default");
    }

    #[test]
    fn a_containers_address_falls_back_to_its_attached_network() {
        let detail = InspectDetail::from_value(InspectKind::Container, &container());
        assert_eq!(text(&detail, Str::DockerFieldIpAddress), "10.89.0.4");
    }

    #[test]
    fn published_ports_collapse_and_unpublished_ones_are_omitted() {
        let detail = InspectDetail::from_value(InspectKind::Container, &container());
        // Both address families render one mapping; the unpublished 9000 is gone.
        assert_eq!(text(&detail, Str::DockerColumnPorts), "8080→80/tcp");
    }

    #[test]
    fn a_mount_reads_by_its_volume_when_it_has_one() {
        let detail = InspectDetail::from_value(InspectKind::Container, &container());
        assert_eq!(
            text(&detail, Str::DockerFieldMounts),
            "pgdata→/var/lib/data, /etc/conf"
        );
    }

    #[test]
    fn fields_the_engine_did_not_report_are_missing_not_blank() {
        let detail = InspectDetail::from_value(InspectKind::Container, &json!({ "Id": "abc" }));
        assert_eq!(field(&detail, Str::DockerColumnImage), FieldValue::Missing);
        assert_eq!(field(&detail, Str::DockerColumnPorts), FieldValue::Missing);
        // Every container field is still present, so the panel keeps its shape.
        assert_eq!(detail.fields.len(), 13);
    }

    #[test]
    fn an_image_detail_names_itself_by_its_first_tag() {
        let value = json!({
            "Id": "sha256:feedfacefeedfacefeed",
            "RepoTags": ["nginx:1.27", "nginx:latest"],
            "RepoDigests": ["nginx@sha256:aaaa"],
            "Created": "2026-06-01T00:00:00Z",
            "Size": 187_000_000,
            "Architecture": "arm64",
            "Os": "linux",
            "RootFS": { "Layers": ["a", "b", "c"] },
            "Config": { "Cmd": ["nginx", "-g", "daemon off;"] }
        });
        let detail = InspectDetail::from_value(InspectKind::Image, &value);
        assert_eq!(detail.title, "nginx:1.27");
        assert_eq!(
            text(&detail, Str::DockerFieldTags),
            "nginx:1.27, nginx:latest"
        );
        assert_eq!(text(&detail, Str::DockerFieldDigest), "nginx@sha256:aaaa");
        assert_eq!(text(&detail, Str::DockerColumnSize), "187.0MB");
        assert_eq!(text(&detail, Str::DockerFieldLayers), "3");
    }

    #[test]
    fn an_untagged_image_falls_back_to_its_short_id() {
        let value = json!({ "Id": "sha256:feedfacefeedface", "RepoTags": ["<none>:<none>"] });
        let detail = InspectDetail::from_value(InspectKind::Image, &value);
        assert_eq!(detail.title, "feedfacefeed");
        assert_eq!(field(&detail, Str::DockerFieldTags), FieldValue::Missing);
    }

    #[test]
    fn a_volume_detail_reads_its_labels_as_pairs() {
        let value = json!({
            "Name": "pgdata",
            "Driver": "local",
            "Mountpoint": "/var/lib/containers/pgdata/_data",
            "CreatedAt": "2026-05-05T12:00:00Z",
            "Scope": "local",
            "UsageData": { "Size": -1 },
            "Labels": { "app": "web", "tier": "" }
        });
        let detail = InspectDetail::from_value(InspectKind::Volume, &value);
        assert_eq!(detail.title, "pgdata");
        assert_eq!(text(&detail, Str::DockerFieldLabels), "app=web, tier");
        // -1 is the engine's "not calculated" sentinel, not a zero-byte volume.
        assert_eq!(field(&detail, Str::DockerColumnSize), FieldValue::Missing);
    }

    #[test]
    fn a_network_detail_carries_its_flags_and_ipam() {
        let value = json!({
            "Id": "0123456789abcdef",
            "Name": "app_default",
            "Driver": "bridge",
            "Scope": "local",
            "Internal": false,
            "Attachable": true,
            "IPAM": { "Config": [{ "Subnet": "10.89.0.0/24", "Gateway": "10.89.0.1" }] },
            "Containers": { "abc": {}, "def": {} }
        });
        let detail = InspectDetail::from_value(InspectKind::Network, &value);
        assert_eq!(
            field(&detail, Str::DockerFieldInternal),
            FieldValue::Flag(false)
        );
        assert_eq!(
            field(&detail, Str::DockerFieldAttachable),
            FieldValue::Flag(true)
        );
        assert_eq!(text(&detail, Str::DockerFieldSubnet), "10.89.0.0/24");
        assert_eq!(text(&detail, Str::DockerFieldGateway), "10.89.0.1");
        assert_eq!(text(&detail, Str::Containers), "2");
    }

    #[test]
    fn the_raw_json_is_pretty_printed() {
        let detail = InspectDetail::from_value(InspectKind::Volume, &json!({ "Name": "data" }));
        assert_eq!(detail.json, "{\n  \"Name\": \"data\"\n}");
    }

    #[test]
    fn short_id_drops_the_algorithm_and_truncates() {
        assert_eq!(short_id("sha256:0123456789abcdef".into()), "0123456789ab");
        assert_eq!(short_id("0123456789abcdef".into()), "0123456789ab");
        assert_eq!(short_id("short".into()), "short");
    }
}
