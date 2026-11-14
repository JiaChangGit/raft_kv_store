//! Wire data and immutable bounds; no consensus algorithm or I/O.
use serde::{Deserialize, Serialize};

/// Node IDs are 1..=MAX_NODE (docs/MEMBERSHIP.md §5).
pub const MAX_NODE: NodeId = 7;
pub const MAX_FRAME: usize = 1024 * 1024;
pub const MAX_KEY: usize = 256;
pub const MAX_VALUE: usize = 4096;
pub const CLUSTER: &str = "raft-kv-v1";
/// Largest snapshot a node writes, loads or accepts from a leader.
pub const MAX_SNAPSHOT: usize = 64 * 1024 * 1024;
/// Largest snapshot chunk per InstallSnapshot. Hex doubles it to 512 KiB,
/// which fits the 1 MiB frame limit with room for the envelope.
pub const MAX_CHUNK: usize = 256 * 1024;

/// Lower-case hexadecimal. JSON arrays of numbers would grow bytes about 3.5x.
pub fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 15) as usize] as char);
    }
    out
}
/// Inverse of [`to_hex`]; `None` for odd length or any character outside `0-9a-f`.
pub fn from_hex(text: &str) -> Option<Vec<u8>> {
    let digit = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    };
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    bytes
        .chunks(2)
        .map(|pair| Some(digit(pair[0])? << 4 | digit(pair[1])?))
        .collect()
}
pub type NodeId = u8;
pub type Term = u64;
pub type LogIndex = u64;

pub fn valid_node(id: NodeId) -> bool {
    (1..=MAX_NODE).contains(&id)
}

/// A set of node IDs; bit n set means node n is in the set. As a configuration
/// (docs/MEMBERSHIP.md §5) it lists the voters and must be non-empty.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Voters(u32);
impl Voters {
    const ALLOWED: u32 = ((1 << (MAX_NODE as u32 + 1)) - 1) & !1;
    /// A valid configuration: non-empty, only bits 1..=MAX_NODE.
    pub fn from_bits(bits: u32) -> Option<Self> {
        (bits != 0 && bits & !Self::ALLOWED == 0).then_some(Self(bits))
    }
    /// A valid configuration from strictly ascending, valid IDs.
    pub fn from_ids(ids: &[NodeId]) -> Option<Self> {
        if ids.is_empty()
            || !ids.iter().all(|id| valid_node(*id))
            || !ids.windows(2).all(|pair| pair[0] < pair[1])
        {
            return None;
        }
        Some(Self(ids.iter().fold(0, |bits, id| bits | 1 << id)))
    }
    pub fn bits(self) -> u32 {
        self.0
    }
    pub fn contains(self, id: NodeId) -> bool {
        valid_node(id) && self.0 & (1 << id) != 0
    }
    pub fn len(self) -> usize {
        self.0.count_ones() as usize
    }
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
    /// More than half: 1 of 1, 2 of 2, 2 of 3, 3 of 4, 3 of 5 ...
    pub fn majority(self) -> usize {
        self.len() / 2 + 1
    }
    /// Whether `members` holds a majority of this configuration; members
    /// outside it do not count.
    pub fn is_quorum(self, members: Voters) -> bool {
        !self.is_empty() && (self.0 & members.0).count_ones() as usize >= self.majority()
    }
    pub fn with(self, id: NodeId) -> Self {
        if valid_node(id) {
            Self(self.0 | 1 << id)
        } else {
            self
        }
    }
    pub fn without(self, id: NodeId) -> Self {
        if valid_node(id) {
            Self(self.0 & !(1 << id))
        } else {
            self
        }
    }
    /// Members in ascending order.
    pub fn ids(self) -> impl Iterator<Item = NodeId> {
        (1..=MAX_NODE).filter(move |id| self.contains(*id))
    }
    pub fn to_vec(self) -> Vec<NodeId> {
        self.ids().collect()
    }
}
impl std::fmt::Display for Voters {
    /// `1,2,3`: the form used in client responses and traces.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ids: Vec<String> = self.ids().map(|id| id.to_string()).collect();
        f.write_str(&ids.join(","))
    }
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
#[serde(deny_unknown_fields)]
pub enum Command {
    NoOp,
    Client(ClientRequest),
    /// A cluster configuration (docs/MEMBERSHIP.md §5): the voters, ascending.
    /// It takes effect as soon as it is in a log; the state machine ignores it.
    Config {
        voters: Vec<NodeId>,
    },
}
impl Command {
    /// The one validity rule for commands, from the WAL or from a leader.
    pub fn valid(&self) -> bool {
        match self {
            Self::NoOp => true,
            Self::Client(request) => request.operation.valid(),
            Self::Config { voters } => Voters::from_ids(voters).is_some(),
        }
    }
    pub fn config(&self) -> Option<Voters> {
        match self {
            Self::Config { voters } => Voters::from_ids(voters),
            _ => None,
        }
    }
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
#[derive(Clone, Debug, Default)]
pub struct DurableState {
    pub term: Term,
    pub voted_for: Option<NodeId>,
    base: LogBase,
    entries: Vec<LogEntry>,
    /// The lowest index appended or truncated since the last `mark_clean`.
    /// With it, what must be persisted is known without comparing two whole
    /// logs (docs/PERFORMANCE.md §6.3). Bookkeeping, not part of the value.
    dirty_from: Option<LogIndex>,
    /// Indexes of the configuration entries in `entries`, ascending. Derived
    /// from `entries` (so equality is unaffected); finding the configuration
    /// then never scans the whole log (docs/MEMBERSHIP.md §5).
    configs: Vec<LogIndex>,
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
        let configs = entries
            .iter()
            .filter(|entry| matches!(entry.command, Command::Config { .. }))
            .map(|entry| entry.index)
            .collect();
        Self {
            term,
            voted_for,
            base,
            entries,
            dirty_from: None,
            configs,
        }
    }
    /// Entries from the first index changed since the last `mark_clean`, with
    /// the current term and vote: exactly what one WAL transaction records.
    pub fn delta(&self) -> Delta {
        let truncate_from = self.dirty_from.unwrap_or(self.last_index() + 1);
        Delta {
            term: self.term,
            voted_for: self.voted_for,
            truncate_from,
            append: self.entries_from(truncate_from).unwrap_or(&[]).to_vec(),
        }
    }
    /// Whether the log changed since the last `mark_clean`.
    pub fn log_changed(&self) -> bool {
        self.dirty_from.is_some()
    }
    pub fn mark_clean(&mut self) {
        self.dirty_from = None;
    }
    fn touch(&mut self, index: LogIndex) {
        self.dirty_from = Some(self.dirty_from.map_or(index, |dirty| dirty.min(index)));
    }
    /// Apply a delta produced by [`DurableState::delta`] on an equal state.
    /// The caller validates first; see `raft-storage`.
    pub fn apply(&mut self, delta: &Delta) {
        self.term = delta.term;
        self.voted_for = delta.voted_for;
        if delta.truncate_from <= self.last_index() {
            self.truncate_from(delta.truncate_from);
        }
        for entry in &delta.append {
            self.push(entry.clone());
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
        // Falling back to the previous configuration (dissertation §4.1).
        self.configs.retain(|config| *config < index);
        self.touch(index);
    }
    pub fn push(&mut self, entry: LogEntry) {
        self.touch(entry.index);
        if matches!(entry.command, Command::Config { .. }) {
            self.configs.push(entry.index);
        }
        self.entries.push(entry);
    }
    /// The latest configuration entry held after the base, as (index, voters).
    /// `None` when the log holds none; the snapshot's voters then apply.
    pub fn latest_config(&self) -> Option<(LogIndex, Voters)> {
        self.configs
            .last()
            .and_then(|index| Some((*index, self.entry(*index)?.command.config()?)))
    }
    /// The latest configuration entry held at or before `index`.
    pub fn config_at(&self, index: LogIndex) -> Option<(LogIndex, Voters)> {
        let count = self.configs.partition_point(|config| *config <= index);
        let at = *self.configs.get(count.checked_sub(1)?)?;
        Some((at, self.entry(at)?.command.config()?))
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
                    && e.command.valid()
            })
            && self
                .entries
                .windows(2)
                .all(|pair| pair[0].term <= pair[1].term)
    }
}

impl PartialEq for DurableState {
    /// The persistent value only; `configs` and `dirty_from` are derived bookkeeping.
    fn eq(&self, other: &Self) -> bool {
        self.term == other.term
            && self.voted_for == other.voted_for
            && self.base == other.base
            && self.entries == other.entries
    }
}
impl Eq for DurableState {}

/// One step's change to the persistent state: the new term and vote, and the
/// log from `truncate_from` on (everything at or after it is replaced by
/// `append`). `truncate_from` is one past the last index when nothing is removed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delta {
    pub term: Term,
    pub voted_for: Option<NodeId>,
    pub truncate_from: LogIndex,
    pub append: Vec<LogEntry>,
}

/// Reflected CRC32C/Castagnoli: polynomial 0x82F63B78, init/final xor 0xFFFFFFFF.
pub fn crc32c(bytes: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0x82f6_3b78 & (0_u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

/// A state machine image covering every entry up to and including `base`.
/// `data` is opaque here; the node's state machine (or the simulator's model)
/// produces and interprets it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub base: LogBase,
    pub voters: u32,
    pub data: std::sync::Arc<[u8]>,
}
impl Snapshot {
    /// `voters` is the configuration at `base` (docs/MEMBERSHIP.md §5).
    pub fn new(base: LogBase, voters: Voters, data: Vec<u8>) -> Self {
        Self {
            base,
            voters: voters.bits(),
            data: data.into(),
        }
    }
    pub fn crc(&self) -> u32 {
        crc32c(&self.data)
    }
    pub fn valid(&self) -> bool {
        self.base.index > 0
            && self.base.term > 0
            && Voters::from_bits(self.voters).is_some()
            && !self.data.is_empty()
            && self.data.len() <= MAX_SNAPSHOT
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
    /// `last_log_index` is the follower's last index; on failure the leader
    /// caps nextIndex with it (dissertation §4.2.1).
    AppendResponse {
        term: Term,
        success: bool,
        matched_through: LogIndex,
        last_log_index: LogIndex,
    },
    /// Figure 13, plus `voters`, `total_len` and `total_crc` so a follower can
    /// bound memory on the first chunk and verify the whole on the last.
    InstallSnapshot {
        term: Term,
        leader_id: NodeId,
        last_included_index: LogIndex,
        last_included_term: Term,
        voters: u32,
        total_len: u64,
        total_crc: u32,
        offset: u64,
        /// Lower-case hex of at most [`MAX_CHUNK`] bytes.
        data: String,
        done: bool,
    },
    /// `next_offset` tells the leader where to continue. `done` means the
    /// follower durably holds everything through `last_included_index`.
    InstallSnapshotResponse {
        term: Term,
        success: bool,
        last_included_index: LogIndex,
        next_offset: u64,
        done: bool,
    },
}
impl Rpc {
    pub fn term(&self) -> Term {
        match self {
            Self::RequestVote { term, .. }
            | Self::VoteResponse { term, .. }
            | Self::AppendEntries { term, .. }
            | Self::AppendResponse { term, .. }
            | Self::InstallSnapshot { term, .. }
            | Self::InstallSnapshotResponse { term, .. } => *term,
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
                Rpc::InstallSnapshot {
                    term,
                    leader_id,
                    last_included_index,
                    last_included_term,
                    voters,
                    total_len,
                    offset,
                    data,
                    done,
                    ..
                } => {
                    let length = (data.len() / 2) as u64;
                    *leader_id == self.from
                        && *last_included_index > 0
                        && *last_included_term > 0
                        && last_included_term <= term
                        && Voters::from_bits(*voters).is_some()
                        && (1..=MAX_SNAPSHOT as u64).contains(total_len)
                        && data.len() <= 2 * MAX_CHUNK
                        && from_hex(data).is_some()
                        && offset
                            .checked_add(length)
                            .is_some_and(|end| end <= *total_len && (end == *total_len) == *done)
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

    /// Written without #[should_panic]: the acceptance checker requires the plain
    /// "test NAME ... ok" result line, which libtest prints differently for it.
    #[test]
    fn truncating_into_the_base_is_refused() {
        let base = LogBase { index: 3, term: 2 };
        let log = DurableState::with_base(2, None, base, vec![noop(4, 2)]);
        let panic = std::panic::catch_unwind(|| log.clone().truncate_from(3))
            .expect_err("truncating at the base must panic");
        let message = panic
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| panic.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        assert!(
            message.contains("cannot truncate compacted entries"),
            "{message}"
        );
        let mut held = log.clone();
        held.truncate_from(4);
        assert_eq!(held.last_index(), 3, "truncating after the base is fine");
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
    fn hex_round_trips_and_rejects_bad_input() {
        let bytes: Vec<u8> = (0..=255).collect();
        assert_eq!(from_hex(&to_hex(&bytes)).unwrap(), bytes);
        assert_eq!(to_hex(&[0x0f, 0xa0]), "0fa0");
        for bad in ["0", "0g", "0A", " 0"] {
            assert_eq!(from_hex(bad), None, "{bad:?}");
        }
        assert_eq!(from_hex(""), Some(vec![]));
    }

    #[test]
    fn install_snapshot_envelopes_are_bounded() {
        let chunk = |offset: u64, data: &str, done: bool, total_len: u64| Envelope {
            version: 1,
            cluster_id: CLUSTER.into(),
            from: 1,
            to: 2,
            rpc_id: 1,
            body: Rpc::InstallSnapshot {
                term: 3,
                leader_id: 1,
                last_included_index: 5,
                last_included_term: 2,
                voters: 0b1110,
                total_len,
                total_crc: 0,
                offset,
                data: data.into(),
                done,
            },
        };
        assert!(chunk(0, "aabb", false, 4).valid_for(2));
        assert!(chunk(2, "ccdd", true, 4).valid_for(2));
        for (what, envelope) in [
            ("done before the end", chunk(0, "aabb", true, 4)),
            ("not done at the end", chunk(2, "ccdd", false, 4)),
            ("past the end", chunk(3, "ccdd", true, 4)),
            ("not hex", chunk(0, "zz", false, 4)),
            ("empty snapshot", chunk(0, "", true, 0)),
            ("too large", chunk(0, "aa", false, MAX_SNAPSHOT as u64 + 1)),
            (
                "chunk too large",
                chunk(
                    0,
                    &"a".repeat(2 * MAX_CHUNK + 2),
                    false,
                    MAX_SNAPSHOT as u64,
                ),
            ),
        ] {
            assert!(!envelope.valid_for(2), "{what}");
        }
        let mut wrong = chunk(0, "aabb", false, 4);
        if let Rpc::InstallSnapshot {
            last_included_term, ..
        } = &mut wrong.body
        {
            *last_included_term = 4;
        }
        assert!(!wrong.valid_for(2), "snapshot term above the leader's term");
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

    fn config(index: LogIndex, term: Term, voters: &[NodeId]) -> LogEntry {
        LogEntry {
            index,
            term,
            command: Command::Config {
                voters: voters.to_vec(),
            },
        }
    }

    /// docs/MEMBERSHIP.md §5: configurations are validated in one place.
    #[test]
    fn voters_validate_ids_and_bits() {
        assert_eq!(Voters::from_ids(&[1, 2, 3]).unwrap().bits(), 0b1110);
        assert_eq!(Voters::from_ids(&[7]).unwrap().bits(), 1 << 7);
        for (what, ids) in [
            ("empty", &[][..]),
            ("node 0", &[0, 1][..]),
            ("node 8", &[1, 8][..]),
            ("duplicate", &[1, 1][..]),
            ("not ascending", &[2, 1][..]),
        ] {
            assert_eq!(Voters::from_ids(ids), None, "{what}");
            let command = Command::Config {
                voters: ids.to_vec(),
            };
            assert!(!command.valid(), "{what}");
        }
        assert_eq!(
            Voters::from_bits(0b1110).map(Voters::to_vec),
            Some(vec![1, 2, 3])
        );
        for bits in [0, 1, 1 << 8, 1 << 31, 0b1_0000_0010] {
            assert_eq!(Voters::from_bits(bits), None, "{bits:#b}");
        }
        let three = Voters::from_ids(&[1, 2, 3]).unwrap();
        assert_eq!(three.to_string(), "1,2,3");
        assert_eq!(three.with(5).to_vec(), vec![1, 2, 3, 5]);
        assert_eq!(three.without(2).to_vec(), vec![1, 3]);
        assert_eq!(three.with(8), three, "invalid IDs are never added");
        assert!(three.contains(3) && !three.contains(4) && !three.contains(0));
        let bad = serde_json::from_str::<Command>(r#"{"Config":{"voters":[1],"extra":1}}"#);
        assert!(bad.is_err(), "unknown fields are refused");
        let good: Command = serde_json::from_str(r#"{"Config":{"voters":[1,2]}}"#).unwrap();
        assert_eq!(good.config(), Voters::from_ids(&[1, 2]));
    }

    /// Commit, election and ReadIndex all use this one rule; it replaces the
    /// fixed "two of three" (docs/MEMBERSHIP.md §13).
    #[test]
    fn majority_follows_the_configuration() {
        // Any set of nodes 1..=n, including the empty one (n = 0).
        let first_n = |n: u8| (1..=n).fold(Voters::default(), Voters::with);
        for (size, majority) in [(1, 1), (2, 2), (3, 2), (4, 3), (5, 3), (6, 4), (7, 4)] {
            let voters = first_n(size);
            assert_eq!(voters.majority(), majority, "size {size}");
            let just_enough = first_n(majority as u8);
            assert!(voters.is_quorum(just_enough), "size {size}");
            let one_short = first_n(majority as u8 - 1);
            assert!(!voters.is_quorum(one_short), "size {size}");
        }
        // Members outside the configuration never count towards its majority.
        let three = first_n(3);
        let outsiders = Voters::from_ids(&[4, 5, 6, 7]).unwrap();
        assert!(!three.is_quorum(outsiders));
        assert!(three.is_quorum(outsiders.with(1).with(3)));
        assert!(
            !Voters::default().is_quorum(three),
            "no configuration, no quorum"
        );
        // A set cannot hold a member twice, so a duplicate cannot inflate a count.
        assert_eq!(Voters::default().with(2).with(2).len(), 1);
    }

    /// Dissertation §4.1: the latest configuration in the log applies, and a
    /// truncated configuration entry falls back to the previous one.
    #[test]
    fn latest_configuration_tracks_appends_and_truncation() {
        let mut log = DurableState::new(
            3,
            None,
            vec![config(1, 1, &[1]), noop(2, 2), config(3, 2, &[1, 2])],
        );
        assert!(log.valid());
        assert_eq!(
            log.latest_config(),
            Some((3, Voters::from_ids(&[1, 2]).unwrap()))
        );
        log.push(config(4, 3, &[1, 2, 3]));
        log.push(noop(5, 3));
        assert_eq!(log.latest_config().map(|(i, v)| (i, v.len())), Some((4, 3)));
        assert_eq!(log.config_at(3).map(|(i, _)| i), Some(3));
        assert_eq!(log.config_at(2).map(|(i, _)| i), Some(1), "the earlier one");
        assert_eq!(log.config_at(9).map(|(i, _)| i), Some(4));
        log.truncate_from(4);
        assert_eq!(
            log.latest_config(),
            Some((3, Voters::from_ids(&[1, 2]).unwrap()))
        );
        log.truncate_from(2);
        assert_eq!(
            log.latest_config(),
            Some((1, Voters::from_ids(&[1]).unwrap()))
        );
        let compacted = adopt_snapshot(&log, LogBase { index: 1, term: 1 });
        assert_eq!(compacted.latest_config(), None, "now only in the snapshot");
        let rebuilt = DurableState::with_base(
            3,
            None,
            LogBase { index: 1, term: 1 },
            vec![config(2, 2, &[1, 2])],
        );
        assert_eq!(rebuilt.latest_config().map(|(i, _)| i), Some(2));
        assert_eq!(rebuilt, {
            let mut pushed =
                DurableState::with_base(3, None, LogBase { index: 1, term: 1 }, vec![]);
            pushed.push(config(2, 2, &[1, 2]));
            pushed
        });
        let invalid = DurableState::new(1, None, vec![config(1, 1, &[2, 1])]);
        assert!(
            !invalid.valid(),
            "an invalid configuration is an invalid log"
        );
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
