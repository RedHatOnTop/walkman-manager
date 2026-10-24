use std::collections::BTreeMap;
use std::io::ErrorKind;

use serde::Serialize;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{tool} was not found; install it or set its path in Settings")]
    ToolMissing { tool: &'static str },
    #[error("could not start {tool}: {source}")]
    Spawn {
        tool: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("{tool} did not answer within {secs} s")]
    Timeout { tool: &'static str, secs: u64 },
    #[error("{tool} failed: {detail}")]
    Failed { tool: &'static str, detail: String },
    #[error("USB: {0}")]
    Usb(String),
    #[error("no connected device has the id {0}")]
    NoSuchDevice(String),
    #[error("{0} cannot be read over the current connection")]
    NotReachable(String),
    /// Something the app will not or could not do. `why` is what the UI says, in the owner's
    /// language; `message` is the English for logs and the command line.
    #[error("{message}")]
    Unsupported {
        why: Why,
        message: String,
        detail: Option<String>,
        vars: Vec<(&'static str, String)>,
    },
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Network(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl Error {
    pub fn unsupported(why: Why, message: impl Into<String>) -> Self {
        Error::Unsupported {
            why,
            message: message.into(),
            detail: None,
            vars: Vec::new(),
        }
    }

    /// The value the UI shows under the sentence as it is: a path, an address, a tool's words.
    /// Only an [`Error::Unsupported`] carries one.
    pub fn with_detail(mut self, value: impl Into<String>) -> Self {
        debug_assert!(matches!(self, Error::Unsupported { .. }), "with_detail on {self:?}");
        if let Error::Unsupported { detail, .. } = &mut self {
            *detail = Some(value.into());
        }
        self
    }

    /// A value the UI's sentence names, as `{key}`. Only an [`Error::Unsupported`] carries one.
    pub fn with_var(mut self, key: &'static str, value: impl ToString) -> Self {
        debug_assert!(matches!(self, Error::Unsupported { .. }), "with_var on {self:?}");
        if let Error::Unsupported { vars, .. } = &mut self {
            vars.push((key, value.to_string()));
        }
        self
    }
}

macro_rules! whys {
    ($($name:ident = $code:literal,)*) => {
        /// Why the app refused or gave up. Each code is an `err.<code>` string in the app, in
        /// every language it speaks.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Why {
            $($name,)*
        }

        impl Why {
            pub const ALL: &[Why] = &[$(Why::$name,)*];

            pub fn code(self) -> &'static str {
                match self {
                    $(Why::$name => $code,)*
                }
            }
        }
    };
}

whys! {
    Busy = "busy",
    StudioBusy = "studio-busy",
    PlanExpired = "plan-expired",
    ProposalExpired = "proposal-expired",
    ResultExpired = "result-expired",
    NothingChosen = "nothing-chosen",
    AllAnalysed = "all-analysed",
    NoRoomPlayer = "no-room-player",
    NoRoomComputer = "no-room-computer",
    MusicNotRead = "music-not-read",
    AlbumGone = "album-gone",
    UnknownPlayer = "unknown-player",
    VolumeNotMounted = "volume-not-mounted",
    NoMusicFolder = "no-music-folder",
    UnreadAnalysis = "unread-analysis",
    NoAnalysisHelper = "no-analysis-helper",
    NotAnalysisEngine = "not-analysis-engine",
    NoMusicCenter = "no-music-center",
    NoWine = "no-wine",
    NoFfmpeg = "no-ffmpeg",
    NotSensmeTable = "not-sensme-table",
    NoSensmeTable = "no-sensme-table",
    SensmeTableVersion = "sensme-table-version",
    SensmeTableDamaged = "sensme-table-damaged",
    SensmeTableOld = "sensme-table-old",
    TagsUnreadable = "tags-unreadable",
    Id3TooLarge = "id3-too-large",
    FlacUnreadable = "flac-unreadable",
    FlacBlockTooLarge = "flac-block-too-large",
    WouldLoseFrames = "would-lose-frames",
    TagWriteFailed = "tag-write-failed",
    TagFormat = "tag-format",
    ReadBackDiffers = "read-back-differs",
    AudioIncomplete = "audio-incomplete",
    AnalysisRejected = "analysis-rejected",
    AnalysisFormat = "analysis-format",
    AnalysisReadBack = "analysis-read-back",
    Id3Kind = "id3-kind",
    BackupMismatch = "backup-mismatch",
    OriginalRestored = "original-restored",
    OriginalKept = "original-kept",
    AppearedWhileCopying = "appeared-while-copying",
    SizeAfterCopy = "size-after-copy",
    MissingAfterCopy = "missing-after-copy",
    ReadBackFailed = "read-back-failed",
    EmptyQuery = "empty-query",
    NotCoverAddress = "not-cover-address",
    UnknownFile = "unknown-file",
    NoGuide = "no-guide",
    NoDirectDownload = "no-direct-download",
    NoFileName = "no-file-name",
    NotPinnedFile = "not-pinned-file",
    KitHasNoSteps = "kit-has-no-steps",
    KitNotStamped = "kit-not-stamped",
    KitRepoNotReady = "kit-repo-not-ready",
    LicenceNotFound = "licence-not-found",
    LicenceNotAgreed = "licence-not-agreed",
    LicenceChanged = "licence-changed",
    KitInputsMissing = "kit-inputs-missing",
    HostNotReady = "host-not-ready",
    NoAdbKey = "no-adb-key",
    NoUpgKey = "no-upg-key",
    NotUpgKey = "not-upg-key",
    BadInstallSet = "bad-install-set",
    InstallWrote = "install-wrote",
    BuildNotFound = "build-not-found",
    BuildNotDone = "build-not-done",
    SetChanged = "set-changed",
    InstallBusy = "install-busy",
    ToolsBusy = "tools-busy",
    GateClosed = "gate-closed",
    ConfirmMismatch = "confirm-mismatch",
    ModeNotForSet = "mode-not-for-set",
    NotBooted = "not-booted",
    NotInFastboot = "not-in-fastboot",
    StillLocked = "still-locked",
    NoReturn = "no-return",
    ReadBackMismatch = "read-back-mismatch",
    NotWiped = "not-wiped",
    NoDrive = "no-drive",
    DriveNotMounted = "drive-not-mounted",
    NotWalkmanDrive = "not-walkman-drive",
    NotAuthorised = "not-authorised",
    NoDriveReturn = "no-drive-return",
    UpgDidNotRun = "upg-did-not-run",
}

/// What the UI receives for a failed command: a stable kind for logic, the English message
/// for logs, and what to say in the owner's language: the `err.<say>` string filled with
/// `vars`, and `detail` shown under it as it is.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorReport {
    pub kind: &'static str,
    pub message: String,
    pub say: &'static str,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub vars: BTreeMap<&'static str, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Every `say` that does not come from a `Why`.
pub const SAYS: &[&str] = &[
    "tool-missing",
    "spawn",
    "timeout",
    "failed",
    "usb",
    "no-such-device",
    "not-reachable",
    "cancelled",
    "network",
    "io",
    "io-not-found",
    "io-permission",
    "io-no-space",
    "io-read-only",
    "internal",
];

impl ErrorReport {
    /// A refusal that is not an `Error`, under its own kind.
    pub fn refused(kind: &'static str, why: Why, message: impl Into<String>) -> Self {
        ErrorReport {
            kind,
            message: message.into(),
            say: why.code(),
            vars: BTreeMap::new(),
            detail: None,
        }
    }

    /// Something that should not happen, such as a worker thread that died.
    pub fn internal(e: impl std::fmt::Display) -> Self {
        let message = e.to_string();
        ErrorReport {
            kind: "internal",
            say: "internal",
            vars: BTreeMap::new(),
            detail: Some(message.clone()),
            message,
        }
    }
}

impl From<&Error> for ErrorReport {
    fn from(e: &Error) -> Self {
        let mut vars = BTreeMap::new();
        let (kind, say, detail) = match e {
            Error::ToolMissing { tool } => {
                vars.insert("tool", (*tool).to_owned());
                ("tool-missing", "tool-missing", None)
            }
            Error::Spawn { tool, source } => {
                vars.insert("tool", (*tool).to_owned());
                ("spawn", "spawn", Some(source.to_string()))
            }
            Error::Timeout { tool, secs } => {
                vars.insert("tool", (*tool).to_owned());
                vars.insert("secs", secs.to_string());
                ("timeout", "timeout", None)
            }
            Error::Failed { tool, detail } => {
                vars.insert("tool", (*tool).to_owned());
                ("failed", "failed", Some(detail.clone()))
            }
            Error::Usb(detail) => ("usb", "usb", Some(detail.clone())),
            Error::NoSuchDevice(_) => ("no-such-device", "no-such-device", None),
            Error::NotReachable(what) => ("not-reachable", "not-reachable", Some(what.clone())),
            Error::Unsupported {
                why,
                detail,
                vars: named,
                ..
            } => {
                vars.extend(named.iter().cloned());
                ("unsupported", why.code(), detail.clone())
            }
            Error::Cancelled => ("cancelled", "cancelled", None),
            Error::Network(detail) => ("network", "network", Some(detail.clone())),
            Error::Io(io) => {
                let say = match io.kind() {
                    ErrorKind::NotFound => "io-not-found",
                    ErrorKind::PermissionDenied => "io-permission",
                    ErrorKind::StorageFull => "io-no-space",
                    ErrorKind::ReadOnlyFilesystem => "io-read-only",
                    _ => "io",
                };
                // The OS words its errors in the system's language, not the app's; they add
                // nothing to a sentence that already names the cause.
                ("io", say, (say == "io").then(|| io.to_string()))
            }
        };
        ErrorReport {
            kind,
            message: e.to_string(),
            say,
            vars,
            detail,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The app's ko and en tables each hold every string the backend may ask it to say.
    #[test]
    fn every_say_is_in_both_languages() {
        let strings = include_str!("../../../app/src/i18n/strings.ts");
        let (ko, en) = strings.split_at(strings.find("\nconst en").expect("the en table"));
        let codes = Why::ALL.iter().map(|w| w.code()).chain(SAYS.iter().copied());
        for code in codes {
            let key = format!("\"err.{code}\":");
            let line = |table: &'static str, lang: &str| {
                assert_eq!(table.matches(&key).count(), 1, "{key} once in {lang}");
                table[table.find(&key).unwrap()..].lines().next().unwrap()
            };
            assert_eq!(
                names(line(ko, "ko")),
                names(line(en, "en")),
                "{key}: the same {{names}} in ko and en"
            );
        }
        let mut seen = std::collections::HashSet::new();
        for w in Why::ALL {
            assert!(seen.insert(w.code()), "{} twice", w.code());
            assert!(!SAYS.contains(&w.code()), "{} is also a fixed say", w.code());
        }
    }

    /// The `{name}`s a sentence fills in.
    fn names(line: &str) -> std::collections::BTreeSet<&str> {
        line.split('{')
            .skip(1)
            .filter_map(|s| s.split_once('}').map(|(name, _)| name))
            .filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
            .collect()
    }

    #[test]
    fn io_errors_say_what_happened() {
        let e = Error::Io(std::io::Error::from(ErrorKind::NotFound));
        let r = ErrorReport::from(&e);
        assert_eq!((r.say, r.detail), ("io-not-found", None));
        let e = Error::unsupported(Why::UnreadAnalysis, "3 files").with_var("n", 3);
        let r = ErrorReport::from(&e);
        assert_eq!(
            (r.kind, r.say, r.vars.get("n").map(String::as_str)),
            ("unsupported", "unread-analysis", Some("3"))
        );
    }
}
