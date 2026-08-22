//! The durable control trace (contract §11): every accepted non-observe action
//! taken during a replay, ordered by logical replay time, so the replay plus
//! its trace reproduce the session — and a replay whose actions were not
//! recorded is not a deterministic fixture.
//!
//! The market recording stays immutable. The trace is a sidecar next to it,
//! one JSON object per line, appended intent-first: the dispatcher writes the
//! intent before the action changes state and the terminal result after. A
//! live session has no trace — it is not a fixture — and then [`ControlTrace`]
//! is [`NoTrace`], which records nothing and refuses nothing.

use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
};

use quantick_control::{
    canonical::{Sha256Digest, canonical_sha256},
    id::{CapabilityId, ErrorCode},
    wire::{ActorKind, ModuleRevision, WireU64},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) const TRACE_VERSION: u32 = 1;
/// The sidecar's name beside a recording: `<session file>.control-trace.jsonl`.
pub(crate) const TRACE_SUFFIX: &str = ".control-trace.jsonl";

/// One trace line. `result_code` and `result_digest` are absent on the intent
/// line and present on the result line that follows it; a trace whose last
/// intent has no result is incomplete, and the replay it belongs to is not
/// fixture-eligible.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct TraceEntry {
    pub trace_version: u32,
    /// Logical replay time: milliseconds since the session's first trade,
    /// in the recording's own clock. Wall-clock time never enters.
    pub replay_elapsed_ms: i64,
    pub sequence: WireU64,
    pub actor_kind: ActorKind,
    pub client_name: String,
    pub capability_id: CapabilityId,
    pub capability_version: u32,
    pub canonical_input: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expected_revisions: Vec<ModuleRevision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_code: Option<ErrorCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_digest: Option<Sha256Digest>,
}

/// The port the dispatcher talks to. The replay feed supplies a file-backed
/// one while a session plays; a live tab supplies [`NoTrace`].
pub(crate) trait ControlTrace: Send {
    /// Record the intent before the action changes state. A failure here
    /// fails the action before dispatch, per contract §11.
    fn append_intent(&mut self, entry: &TraceEntry) -> Result<(), TraceError>;
    /// Record the terminal result of the intent with that sequence.
    fn append_result(&mut self, entry: &TraceEntry) -> Result<(), TraceError>;
}

/// A live session: no trace, no refusal.
pub(crate) struct NoTrace;

impl ControlTrace for NoTrace {
    fn append_intent(&mut self, _entry: &TraceEntry) -> Result<(), TraceError> {
        Ok(())
    }

    fn append_result(&mut self, _entry: &TraceEntry) -> Result<(), TraceError> {
        Ok(())
    }
}

/// The sidecar beside a recording, opened for append.
pub(crate) struct ReplayTraceFile {
    #[cfg(test)]
    path: PathBuf,
    file: File,
}

impl ReplayTraceFile {
    /// The sidecar path for a recording.
    pub fn path_for(session_path: &Path) -> PathBuf {
        let mut name = session_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        name.push_str(TRACE_SUFFIX);
        session_path.with_file_name(name)
    }

    /// Open (creating if absent) the sidecar for a recording.
    pub fn open(session_path: &Path) -> Result<Self, TraceError> {
        let path = Self::path_for(session_path);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|error| TraceError::Io(format!("open control trace: {error}")))?;
        Ok(Self {
            #[cfg(test)]
            path,
            file,
        })
    }

    #[cfg(test)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn append(&mut self, entry: &TraceEntry) -> Result<(), TraceError> {
        let mut line = serde_json::to_vec(entry)
            .map_err(|error| TraceError::Io(format!("encode control trace entry: {error}")))?;
        line.push(b'\n');
        self.file
            .write_all(&line)
            .and_then(|()| self.file.sync_data())
            .map_err(|error| TraceError::Io(format!("append control trace entry: {error}")))
    }
}

impl ControlTrace for ReplayTraceFile {
    fn append_intent(&mut self, entry: &TraceEntry) -> Result<(), TraceError> {
        self.append(entry)
    }

    fn append_result(&mut self, entry: &TraceEntry) -> Result<(), TraceError> {
        self.append(entry)
    }
}

/// The entries of a sidecar, read back for re-injection. Intents and results
/// are paired by sequence; an intent without a result makes the trace
/// incomplete, which the caller must treat as fixture-ineligible.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct TraceReplay {
    /// Completed actions in replay-time order: the intent, carrying the
    /// result code its result line reported.
    pub completed: Vec<TraceEntry>,
    /// Sequences whose intent was written but whose result never was.
    pub incomplete: Vec<u64>,
}

impl TraceReplay {
    /// Read a sidecar if one exists. A missing file is an empty, complete
    /// trace: the replay simply had no actions.
    pub fn load(session_path: &Path) -> Result<Self, TraceError> {
        let path = ReplayTraceFile::path_for(session_path);
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(TraceError::Io(format!("open control trace: {error}"))),
        };
        let mut intents: Vec<TraceEntry> = Vec::new();
        let mut completed = Vec::new();
        for (index, line) in BufReader::new(file).lines().enumerate() {
            let line =
                line.map_err(|error| TraceError::Io(format!("read control trace: {error}")))?;
            if line.trim().is_empty() {
                continue;
            }
            let entry: TraceEntry = serde_json::from_str(&line).map_err(|error| {
                TraceError::Malformed(format!("control trace line {}: {error}", index + 1))
            })?;
            if entry.trace_version != TRACE_VERSION {
                return Err(TraceError::Malformed(format!(
                    "control trace line {} has version {}, expected {TRACE_VERSION}",
                    index + 1,
                    entry.trace_version
                )));
            }
            if entry.result_code.is_some() {
                if let Some(position) = intents
                    .iter()
                    .position(|intent| intent.sequence == entry.sequence)
                {
                    intents.remove(position);
                    completed.push(entry);
                }
            } else {
                intents.push(entry);
            }
        }
        completed.sort_by_key(|entry| (entry.replay_elapsed_ms, entry.sequence.get()));
        Ok(Self {
            completed,
            incomplete: intents.into_iter().map(|e| e.sequence.get()).collect(),
        })
    }

    pub fn is_complete(&self) -> bool {
        self.incomplete.is_empty()
    }
}

/// The canonical digest of a result value, as the trace records it.
pub(crate) fn result_digest(result: &Value) -> Option<Sha256Digest> {
    canonical_sha256(result).ok()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum TraceError {
    Io(String),
    Malformed(String),
}

impl std::fmt::Display for TraceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(message) | Self::Malformed(message) => formatter.write_str(message),
        }
    }
}

#[cfg(test)]
mod tests {
    use quantick_control::error::codes;
    use serde_json::json;

    use super::*;

    fn entry(sequence: u64, elapsed_ms: i64) -> TraceEntry {
        TraceEntry {
            trace_version: TRACE_VERSION,
            replay_elapsed_ms: elapsed_ms,
            sequence: WireU64::new(sequence),
            actor_kind: ActorKind::HumanUi,
            client_name: "quantick-ui".to_owned(),
            capability_id: CapabilityId::new("attention.mark.create").unwrap(),
            capability_version: 1,
            canonical_input: json!({}),
            expected_revisions: Vec::new(),
            result_code: None,
            result_digest: None,
        }
    }

    #[test]
    fn intents_and_results_pair_up_and_an_unfinished_intent_is_reported() {
        let directory = std::env::temp_dir().join(format!(
            "quantick-control-trace-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let session = directory.join("WINJ26-2026-03-16.csv");
        std::fs::write(&session, "# not read by the trace\n").unwrap();

        let mut trace = ReplayTraceFile::open(&session).unwrap();
        assert_eq!(
            trace.path(),
            directory.join("WINJ26-2026-03-16.csv.control-trace.jsonl")
        );
        trace.append_intent(&entry(1, 5_000)).unwrap();
        let mut done = entry(1, 5_000);
        done.result_code = Some(ErrorCode::new(codes::INVALID_REQUEST).unwrap());
        done.result_digest = result_digest(&json!({"sequence": "7"}));
        trace.append_result(&done).unwrap();
        // A second intent, never completed.
        trace.append_intent(&entry(2, 9_000)).unwrap();
        drop(trace);

        let replay = TraceReplay::load(&session).unwrap();
        assert_eq!(replay.completed.len(), 1);
        assert_eq!(replay.completed[0].sequence.get(), 1);
        assert_eq!(
            replay.completed[0].result_code.as_ref().unwrap().as_str(),
            codes::INVALID_REQUEST
        );
        assert_eq!(replay.incomplete, vec![2]);
        assert!(
            !replay.is_complete(),
            "an unfinished intent is not a fixture"
        );

        let none = TraceReplay::load(&directory.join("never-recorded.csv")).unwrap();
        assert!(none.is_complete());
        assert!(none.completed.is_empty());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_live_session_has_no_trace_and_refuses_nothing() {
        let mut none = NoTrace;
        none.append_intent(&entry(1, 0)).unwrap();
        none.append_result(&entry(1, 0)).unwrap();
    }
}
