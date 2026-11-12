//! Wire data and immutable bounds; no consensus algorithm or I/O.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const NODES: [u8; 3] = [1, 2, 3];
pub const QUORUM: usize = 2;
pub const MAX_FRAME: usize = 1024 * 1024;
pub const MAX_KEY: usize = 256;
pub const MAX_VALUE: usize = 4096;
pub const CLUSTER: &str = "raft-kv-v1";
pub type NodeId = u8;
pub type Term = u64;
pub type LogIndex = u64;

pub fn valid_node(id: NodeId) -> bool {
    NODES.contains(&id)
}
pub fn has_quorum(voters: &[NodeId]) -> bool {
    voters.iter().all(|id| valid_node(*id))
        && voters.iter().copied().collect::<BTreeSet<_>>().len() >= QUORUM
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Operation {
    Put { key: String, value: String },
    Delete { key: String },
    Get { key: String },
}
impl Operation {
    pub fn valid(&self) -> bool {
        let key = match self {
            Self::Put { key, .. } | Self::Delete { key } | Self::Get { key } => key,
        };
        key.len() <= MAX_KEY
            && match self {
                Self::Put { value, .. } => value.len() <= MAX_VALUE,
                _ => true,
            }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientRequest {
    pub client_id: u128,
    pub sequence: u64,
    pub operation: Operation,
}
/// Versioned client wire envelope; only validated canonical commands enter the log.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientWireRequest {
    pub version: u8,
    pub client_id: u128,
    pub sequence: u64,
    pub operation: Operation,
}
impl ClientWireRequest {
    pub fn canonical(self) -> Result<ClientRequest, &'static str> {
        if self.version != 1 || !self.operation.valid() {
            return Err("INVALID_ARGUMENT");
        }
        Ok(ClientRequest {
            client_id: self.client_id,
            sequence: self.sequence,
            operation: self.operation,
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    NoOp,
    Client(ClientRequest),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogEntry {
    pub index: LogIndex,
    pub term: Term,
    pub command: Command,
}
/// Position just before the first entry still held in the log: the last index
/// covered by the latest snapshot and its term. `(0, 0)` means no snapshot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogBase {
    pub index: LogIndex,
    pub term: Term,
}

/// Persistent Raft state (Figure 2). The log holds only entries after `base`;
/// they are private so that every access must state what happens when the
/// requested index was compacted away (dissertation section 5.1.3).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DurableState {
    pub term: Term,
    pub voted_for: Option<NodeId>,
    base: LogBase,
    entries: Vec<LogEntry>,
}
impl DurableState {
    /// A log that starts at index 1 (no snapshot).
    pub fn new(term: Term, voted_for: Option<NodeId>, entries: Vec<LogEntry>) -> Self {
        Self::with_base(term, voted_for, LogBase::default(), entries)
    }
    /// Construct without validation; call [`DurableState::valid`] where input is untrusted.
    pub fn with_base(
        term: Term,
        voted_for: Option<NodeId>,
        base: LogBase,
        entries: Vec<LogEntry>,
    ) -> Self {
        Self {
            term,
            voted_for,
            base,
            entries,
        }
    }
    pub fn base(&self) -> LogBase {
        self.base
    }
    /// Entries after the base, in index order.
    pub fn entries(&self) -> &[LogEntry] {
        &self.entries
    }
    pub fn last_index(&self) -> LogIndex {
        self.base.index + self.entries.len() as u64
    }
    /// `None` for an index that was compacted (below the base) or does not exist yet.
    pub fn term_at(&self, index: LogIndex) -> Option<Term> {
        if index == self.base.index {
            Some(self.base.term)
        } else {
            self.entry(index).map(|entry| entry.term)
        }
    }
    /// `None` unless `index` is after the base and not beyond the last entry.
    pub fn entry(&self, index: LogIndex) -> Option<&LogEntry> {
        let offset = index.checked_sub(self.base.index + 1)?;
        self.entries.get(usize::try_from(offset).ok()?)
    }
    /// Entries from `index` to the end; `None` if `index` is compacted or past the end + 1.
    pub fn entries_from(&self, index: LogIndex) -> Option<&[LogEntry]> {
        let offset = usize::try_from(index.checked_sub(self.base.index + 1)?).ok()?;
        self.entries.get(offset..)
    }
    /// Remove `index` and everything after it. Compacted entries are never touched.
    pub fn truncate_from(&mut self, index: LogIndex) {
        assert!(index > self.base.index, "cannot truncate compacted entries");
        self.entries
            .truncate((index - self.base.index - 1) as usize);
    }
    pub fn push(&mut self, entry: LogEntry) {
        self.entries.push(entry);
    }
    pub fn valid(&self) -> bool {
        self.voted_for.is_none_or(valid_node)
            && (self.base.index == 0) == (self.base.term == 0)
            && self.base.term <= self.term
            && self.entries.iter().enumerate().all(|(i, e)| {
                e.index == self.base.index + i as u64 + 1
                    && e.term > 0
                    && e.term >= self.base.term
                    && e.term <= self.term
                    && match &e.command {
                        Command::NoOp => true,
                        Command::Client(r) => r.operation.valid(),
                    }
            })
            && self
                .entries
                .windows(2)
                .all(|pair| pair[0].term <= pair[1].term)
    }
}

/// Figure 13 steps 6 and 7: move `state` onto a snapshot that ends at `base`.
/// Entries after the base are kept only if the log already agrees at the base;
/// otherwise the whole log is superseded. Term and vote are unchanged.
pub fn adopt_snapshot(state: &DurableState, base: LogBase) -> DurableState {
    let entries = if state.term_at(base.index) == Some(base.term) {
        state
            .entries_from(base.index + 1)
            .map(<[LogEntry]>::to_vec)
            .unwrap_or_default()
    } else {
        vec![]
    };
    DurableState::with_base(state.term, state.voted_for, base, entries)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Rpc {
    RequestVote {
        term: Term,
        candidate_id: NodeId,
        last_log_index: LogIndex,
        last_log_term: Term,
    },
    VoteResponse {
        term: Term,
        granted: bool,
    },
    AppendEntries {
        term: Term,
        leader_id: NodeId,
        prev_log_index: LogIndex,
        prev_log_term: Term,
        entries: Vec<LogEntry>,
        leader_commit: LogIndex,
    },
    AppendResponse {
        term: Term,
        success: bool,
        matched_through: LogIndex,
    },
}
impl Rpc {
    pub fn term(&self) -> Term {
        match self {
            Self::RequestVote { term, .. }
            | Self::VoteResponse { term, .. }
            | Self::AppendEntries { term, .. }
            | Self::AppendResponse { term, .. } => *term,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub version: u8,
    pub cluster_id: String,
    pub from: NodeId,
    pub to: NodeId,
    pub rpc_id: u64,
    pub body: Rpc,
}
impl Envelope {
    pub fn valid_for(&self, id: NodeId) -> bool {
        self.version == 1
            && self.cluster_id == CLUSTER
            && valid_node(self.from)
            && self.from != id
            && self.to == id
            && match &self.body {
                Rpc::RequestVote { candidate_id, .. } => *candidate_id == self.from,
                Rpc::AppendEntries {
                    leader_id,
                    prev_log_index,
                    prev_log_term,
                    entries,
                    term,
                    ..
                } => {
                    *leader_id == self.from
                        && entries.len() <= 32
                        && prev_log_index.checked_add(entries.len() as u64).is_some()
                        && entries.windows(2).all(|p| p[0].term <= p[1].term)
                        && entries.iter().enumerate().all(|(i, e)| {
                            e.index == prev_log_index + i as u64 + 1
                                && e.term >= *prev_log_term
                                && e.term > 0
                                && e.term <= *term
                        })
                }
                _ => true,
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn noop(index: LogIndex, term: Term) -> LogEntry {
        LogEntry {
            index,
            term,
            command: Command::NoOp,
        }
    }

    /// Every accessor around a non-zero base: compacted, at base, held, past the end.
    #[test]
    fn log_accessors_respect_the_base() {
        let base = LogBase { index: 5, term: 2 };
        let mut log = DurableState::with_base(4, Some(1), base, vec![noop(6, 2), noop(7, 4)]);
        assert!(log.valid());
        assert_eq!(log.last_index(), 7);
        assert_eq!(log.term_at(4), None, "compacted");
        assert_eq!(log.term_at(5), Some(2), "the base itself");
        assert_eq!(log.term_at(7), Some(4));
        assert_eq!(log.term_at(8), None, "not written yet");
        assert_eq!(log.entry(5), None, "the base entry is not held");
        assert_eq!(log.entry(6), Some(&noop(6, 2)));
        assert_eq!(log.entry(8), None);
        assert_eq!(log.entries_from(5), None);
        assert_eq!(log.entries_from(6).unwrap().len(), 2);
        assert_eq!(
            log.entries_from(8),
            Some(&[][..]),
            "one past the end is empty"
        );
        assert_eq!(log.entries_from(9), None);
        log.truncate_from(7);
        assert_eq!(log.last_index(), 6);
        log.push(noop(7, 4));
        assert_eq!(log.entry(7), Some(&noop(7, 4)));

        let empty = DurableState::default();
        assert_eq!(
            (empty.last_index(), empty.term_at(0), empty.entry(0)),
            (0, Some(0), None)
        );
        assert_eq!(empty.entries_from(1), Some(&[][..]));
    }

    #[test]
    #[should_panic(expected = "cannot truncate compacted entries")]
    fn truncating_into_the_base_is_refused() {
        let mut log =
            DurableState::with_base(2, None, LogBase { index: 3, term: 2 }, vec![noop(4, 2)]);
        log.truncate_from(3);
    }

    #[test]
    fn base_consistency_is_validated() {
        let held = vec![noop(4, 2)];
        let at = |index, term| LogBase { index, term };
        assert!(DurableState::with_base(2, None, at(3, 2), held.clone()).valid());
        for (what, state) in [
            (
                "index 0 needs term 0",
                DurableState::with_base(2, None, at(0, 1), vec![]),
            ),
            (
                "a real base needs a term",
                DurableState::with_base(2, None, at(3, 0), vec![]),
            ),
            (
                "base term above current term",
                DurableState::with_base(1, None, at(3, 2), vec![]),
            ),
            (
                "gap after the base",
                DurableState::with_base(2, None, at(2, 2), held.clone()),
            ),
            (
                "entry term below base term",
                DurableState::with_base(3, None, at(3, 3), held.clone()),
            ),
        ] {
            assert!(!state.valid(), "{what}");
        }
    }

    #[test]
    fn adopt_snapshot_keeps_suffix_only_when_the_base_matches() {
        let log = DurableState::new(3, Some(2), vec![noop(1, 1), noop(2, 2), noop(3, 3)]);
        let matching = adopt_snapshot(&log, LogBase { index: 2, term: 2 });
        assert_eq!(matching.base(), LogBase { index: 2, term: 2 });
        assert_eq!(matching.entries(), &[noop(3, 3)]);
        assert_eq!((matching.term, matching.voted_for), (3, Some(2)));

        let conflicting = adopt_snapshot(&log, LogBase { index: 2, term: 3 });
        assert!(
            conflicting.entries().is_empty(),
            "different term at the base"
        );
        let beyond = adopt_snapshot(&log, LogBase { index: 9, term: 3 });
        assert!(
            beyond.entries().is_empty(),
            "snapshot newer than the whole log"
        );
        assert_eq!(beyond.last_index(), 9);
        let at_end = adopt_snapshot(&log, LogBase { index: 3, term: 3 });
        assert!(at_end.entries().is_empty() && at_end.last_index() == 3);
    }

    #[test]
    fn malformed_protocol_and_decreasing_terms_rejected() {
        let noop = |index, term| LogEntry {
            index,
            term,
            command: Command::NoOp,
        };
        assert!(!DurableState::new(3, None, vec![noop(1, 3), noop(2, 2)]).valid());
        assert!(DurableState::new(3, None, vec![noop(1, 3), noop(2, 3)]).valid());
        assert!(
            serde_json::from_str::<ClientRequest>(
                r#"{"client_id":1,"sequence":0,"operation":{"Get":{"key":"x","unexpected":true}}}"#
            )
            .is_err()
        );
        assert!(
            !Operation::Get {
                key: "界".repeat(86)
            }
            .valid()
        );
        assert!(
            Operation::Get {
                key: "界".repeat(85)
            }
            .valid()
        );
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientResponse {
    pub status: String,
    pub result: Option<String>,
    pub leader_hint: Option<NodeId>,
    pub committed_index: Option<LogIndex>,
}
impl ClientResponse {
    pub fn error(status: &str, leader_hint: Option<NodeId>) -> Self {
        Self {
            status: status.into(),
            result: None,
            leader_hint,
            committed_index: None,
        }
    }
}
