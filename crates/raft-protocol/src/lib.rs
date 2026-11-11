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
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableState {
    pub term: Term,
    pub voted_for: Option<NodeId>,
    pub log: Vec<LogEntry>,
}
impl DurableState {
    pub fn last_index(&self) -> LogIndex {
        self.log.len() as u64
    }
    pub fn term_at(&self, index: LogIndex) -> Option<Term> {
        if index == 0 {
            Some(0)
        } else {
            self.log.get((index - 1) as usize).map(|e| e.term)
        }
    }
    pub fn valid(&self) -> bool {
        self.voted_for.is_none_or(valid_node)
            && self.log.windows(2).all(|pair| pair[0].term <= pair[1].term)
            && self.log.iter().enumerate().all(|(i, e)| {
                e.index == i as u64 + 1
                    && e.term > 0
                    && e.term <= self.term
                    && match &e.command {
                        Command::NoOp => true,
                        Command::Client(r) => r.operation.valid(),
                    }
            })
    }
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
    #[test]
    fn malformed_protocol_and_decreasing_terms_rejected() {
        let mut state = DurableState {
            term: 3,
            voted_for: None,
            log: vec![
                LogEntry {
                    index: 1,
                    term: 3,
                    command: Command::NoOp,
                },
                LogEntry {
                    index: 2,
                    term: 2,
                    command: Command::NoOp,
                },
            ],
        };
        assert!(!state.valid());
        state.log[1].term = 3;
        assert!(state.valid());
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
