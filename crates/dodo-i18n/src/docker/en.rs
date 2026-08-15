//! The English column of the Docker tool.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::Docker => "Docker".into(),
        Text::Containers => "Containers".into(),
        Text::Images => "Images".into(),
        Text::Volumes => "Volumes".into(),
        Text::Networks => "Networks".into(),
        Text::SearchPlaceholder => "Search containers".into(),
        Text::Refresh => "Refresh".into(),
        Text::Filter => "Filter".into(),
        Text::Create => "Create".into(),
        Text::ColumnName => "Name".into(),
        Text::ColumnImage => "Image".into(),
        Text::ColumnStatus => "Status".into(),
        Text::ColumnCpu => "CPU %".into(),
        Text::ColumnPorts => "Ports".into(),
        Text::ColumnLastStarted => "Last Started".into(),
        Text::ColumnActions => "Actions".into(),
        Text::StatusRunning => "Running".into(),
        Text::StatusExited => "Exited".into(),
        Text::StatusCreated => "Created".into(),
        Text::StatusRestarting => "Restarting".into(),
        Text::StatusPaused => "Paused".into(),
        Text::StatusDead => "Dead".into(),
        Text::StatusRemoving => "Removing".into(),
        Text::StatusStopping => "Stopping".into(),
        Text::StatusUnknown => "Unknown".into(),
        Text::Start => "Start".into(),
        Text::Stop => "Stop".into(),
        Text::Restart => "Restart".into(),
        Text::DeleteTitle => "Delete container?".into(),
        Text::DeleteMessage(name) => {
            format!("Permanently remove \"{name}\"? This cannot be undone.").into()
        }
        Text::Cancel => "Cancel".into(),
        Text::NoContainers => "No containers found.".into(),
        Text::NoContainersHint => "Containers you create will appear here.".into(),
        Text::Retry => "Retry".into(),
        Text::ConnectionError(detail) => {
            format!("Could not reach the Docker engine: {detail}").into()
        }
        Text::OperationError(detail) => {
            format!("That action could not be completed: {detail}").into()
        }
        Text::SelectAll => "Select all".into(),
        Text::SelectRow => "Select container".into(),
        Text::RelNever => "Never".into(),
        Text::RelJustNow => "just now".into(),
        Text::RelSecondsAgo(n) => format!("{n} second{} ago", if n == 1 { "" } else { "s" }).into(),
        Text::RelMinutesAgo(n) => format!("{n} minute{} ago", if n == 1 { "" } else { "s" }).into(),
        Text::RelHoursAgo(n) => format!("{n} hour{} ago", if n == 1 { "" } else { "s" }).into(),
        Text::RelDaysAgo(n) => format!("{n} day{} ago", if n == 1 { "" } else { "s" }).into(),
        Text::RelWeeksAgo(n) => format!("{n} week{} ago", if n == 1 { "" } else { "s" }).into(),
        Text::RelMonthsAgo(n) => format!("{n} month{} ago", if n == 1 { "" } else { "s" }).into(),
        Text::RelYearsAgo(n) => format!("{n} year{} ago", if n == 1 { "" } else { "s" }).into(),
        Text::UnreachableTitle => "Can't reach the Docker engine".into(),
        Text::Ungrouped => "Ungrouped".into(),
        Text::GroupContainers(n) => {
            format!("{n} container{}", if n == 1 { "" } else { "s" }).into()
        }
        Text::GroupRunning(n) => format!("{n} running").into(),
        Text::FilterWithCount(n) => format!("Filter ({n})").into(),
        Text::FilterTitle => "Filters".into(),
        Text::FilterProject => "Compose project".into(),
        Text::FilterPublishedPorts => "Has published ports".into(),
        Text::FilterFavorites => "Favorites (coming soon)".into(),
        Text::FilterClear => "Clear filters".into(),
        Text::BulkSelected(n) => format!("{n} selected").into(),
        Text::BulkStart => "Start selected".into(),
        Text::BulkStop => "Stop selected".into(),
        Text::BulkDelete => "Delete selected".into(),
        Text::BulkClear => "Clear selection".into(),
        Text::BulkDeleteTitle => "Delete containers?".into(),
        Text::BulkDeleteMessage(n) => format!(
            "Permanently remove {n} container{}? This cannot be undone.",
            if n == 1 { "" } else { "s" }
        )
        .into(),
        Text::BulkFailures(n) => format!(
            "{n} container{} could not be updated.",
            if n == 1 { "" } else { "s" }
        )
        .into(),
        Text::ColumnRepository => "Repository".into(),
        Text::ColumnTag => "Tag".into(),
        Text::ColumnImageId => "Image ID".into(),
        Text::ColumnSize => "Size".into(),
        Text::ColumnCreated => "Created".into(),
        Text::ColumnContainersUsing => "Containers using".into(),
        Text::ColumnDriver => "Driver".into(),
        Text::ColumnMountPoint => "Mount point".into(),
        Text::ColumnScope => "Scope".into(),
        Text::SearchImages => "Search images".into(),
        Text::SearchVolumes => "Search volumes".into(),
        Text::SearchNetworks => "Search networks".into(),
        Text::NoImages => "No images".into(),
        Text::NoImagesHint => "Pull or build an image and it will appear here.".into(),
        Text::NoVolumes => "No volumes".into(),
        Text::NoVolumesHint => "Create a volume and it will appear here.".into(),
        Text::NoNetworks => "No networks".into(),
        Text::NoNetworksHint => "Create a network and it will appear here.".into(),
        Text::NotAvailable => "N/A".into(),
        Text::None => "<none>".into(),
        Text::Inspect => "Inspect".into(),
        Text::NetworkPredefined => "Predefined networks cannot be removed".into(),
        Text::ViewLogs => "View Logs".into(),
        Text::OpenTerminal => "Open Terminal".into(),
        Text::ComingSoonLabel => "Coming soon".into(),
        Text::Details => "Details".into(),
        Text::RawJson => "Raw JSON".into(),
        Text::DetailErrorTitle => "Couldn't load this".into(),
        Text::NoLogs => "No log output.".into(),
        Text::NoLogsHint => {
            "This container hasn't written anything to stdout or stderr yet.".into()
        }
        Text::LogsTail(n) => format!("Showing the last {n} lines").into(),
        Text::Yes => "Yes".into(),
        Text::No => "No".into(),
        Text::FieldId => "ID".into(),
        Text::FieldCommand => "Command".into(),
        Text::FieldStarted => "Started".into(),
        Text::FieldExitCode => "Exit code".into(),
        Text::FieldRestartPolicy => "Restart policy".into(),
        Text::FieldIpAddress => "IP address".into(),
        Text::FieldMounts => "Mounts".into(),
        Text::FieldTags => "Tags".into(),
        Text::FieldDigest => "Digest".into(),
        Text::FieldArchitecture => "Architecture".into(),
        Text::FieldOs => "OS".into(),
        Text::FieldLayers => "Layers".into(),
        Text::FieldLabels => "Labels".into(),
        Text::FieldOptions => "Options".into(),
        Text::FieldInternal => "Internal".into(),
        Text::FieldAttachable => "Attachable".into(),
        Text::FieldSubnet => "Subnet".into(),
        Text::FieldGateway => "Gateway".into(),
        Text::Pull => "Pull".into(),
        Text::Build => "Build".into(),
        Text::Stats => "Stats".into(),
        Text::OpenDetails => "Open details".into(),
        Text::Runtimes => "Runtimes".into(),
        Text::RuntimesDescription => {
            "Detect the container runtimes on this machine and control them without leaving Dodo."
                .into()
        }
        Text::RuntimePodmanMachine => "Podman Machine".into(),
        Text::RuntimeKubernetes => "Kubernetes".into(),
        Text::RuntimeContainerd => "containerd".into(),
        Text::RuntimeStatusRunning => "Running".into(),
        Text::RuntimeStatusStopped => "Stopped".into(),
        Text::RuntimeStatusNotInstalled => "Not installed".into(),
        Text::RuntimeStatusUnsupported => "Not supported on this platform".into(),
        Text::RuntimeStatusUnknown => "Unknown".into(),
        Text::RuntimeManagedExternally => {
            "Managed by your cluster provider (Docker Desktop, minikube, kind, …), not from here."
                .into()
        }
        Text::RuntimeStarting => "Starting…".into(),
        Text::RuntimeStopping => "Stopping…".into(),
        Text::RuntimeBinaryNotFound => {
            "The required command-line tool could not be found on this machine.".into()
        }
        Text::RuntimeActionUnsupported => "This action isn't available for this runtime.".into(),
    }
}
