//! Deterministic applied state. The caller supplies only an already committed prefix.
use raft_protocol::{ClientResponse, Command, LogEntry, Operation};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Session {
    operation: Operation,
    response: ClientResponse,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StateMachine {
    values: BTreeMap<String, String>,
    sessions: BTreeMap<(u128, u64), Session>,
    highest_sequence: BTreeMap<u128, u64>,
    applied_index: u64,
    effect_count: u64,
}
impl StateMachine {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }
    pub fn values(&self) -> &BTreeMap<String, String> {
        &self.values
    }
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }
    pub fn applied_index(&self) -> u64 {
        self.applied_index
    }
    /// First executions of logical requests, including GET; duplicate log entries do not increase it.
    pub fn effect_count(&self) -> u64 {
        self.effect_count
    }
    pub fn apply(&mut self, entry: &LogEntry) -> ClientResponse {
        assert_eq!(
            entry.index,
            self.applied_index + 1,
            "apply must consume an uninterrupted committed prefix"
        );
        self.applied_index = entry.index;
        let Command::Client(request) = &entry.command else {
            return ClientResponse {
                status: "OK".into(),
                result: None,
                leader_hint: None,
                committed_index: Some(entry.index),
            };
        };
        let id = (request.client_id, request.sequence);
        if let Some(session) = self.sessions.get(&id) {
            return if session.operation == request.operation {
                session.response.clone()
            } else {
                rejection("REQUEST_ID_REUSE", entry.index)
            };
        }
        if !request.operation.valid()
            || self
                .highest_sequence
                .get(&request.client_id)
                .is_some_and(|last| request.sequence <= *last)
        {
            return rejection("INVALID_ARGUMENT", entry.index);
        }
        let result = match &request.operation {
            Operation::Put { key, value } => self.values.insert(key.clone(), value.clone()),
            Operation::Delete { key } => self.values.remove(key),
            Operation::Get { key } => self.values.get(key).cloned(),
        };
        self.effect_count += 1;
        let response = ClientResponse {
            status: "OK".into(),
            result,
            leader_hint: None,
            committed_index: Some(entry.index),
        };
        self.sessions.insert(
            id,
            Session {
                operation: request.operation.clone(),
                response: response.clone(),
            },
        );
        self.highest_sequence
            .insert(request.client_id, request.sequence);
        response
    }
}
fn rejection(status: &str, index: u64) -> ClientResponse {
    ClientResponse {
        status: status.into(),
        result: None,
        leader_hint: None,
        committed_index: Some(index),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use raft_protocol::ClientRequest;
    fn entry(index: u64, client: u128, sequence: u64, operation: Operation) -> LogEntry {
        LogEntry {
            index,
            term: 1,
            command: Command::Client(ClientRequest {
                client_id: client,
                sequence,
                operation,
            }),
        }
    }
    fn put(value: &str) -> Operation {
        Operation::Put {
            key: "x".into(),
            value: value.into(),
        }
    }

    #[test]
    fn retry_same_request_has_single_effect() {
        let mut kv = StateMachine::default();
        let first = entry(1, 1, 1, put("A"));
        let result = kv.apply(&first);
        assert_eq!(result.result, None);
        let later = kv.apply(&entry(2, 2, 1, put("B")));
        assert_eq!(later.result, Some("A".into()));
        let mut retry = first.clone();
        retry.index = 3;
        assert_eq!(kv.apply(&retry), result);
        assert_eq!(kv.get("x"), Some("B"));
        assert_eq!(kv.effect_count(), 2);
        assert_eq!(kv.session_count(), 2);
        assert_eq!(kv.applied_index(), 3);
    }

    #[test]
    fn same_id_different_command_rejected() {
        let mut kv = StateMachine::default();
        kv.apply(&entry(1, 1, 1, put("A")));
        assert_eq!(
            kv.apply(&entry(2, 1, 1, put("B"))).status,
            "REQUEST_ID_REUSE"
        );
        assert_eq!(
            kv.apply(&entry(3, 1, 1, Operation::Get { key: "x".into() }))
                .status,
            "REQUEST_ID_REUSE"
        );
        assert_eq!(kv.get("x"), Some("A"));
        assert_eq!(kv.effect_count(), 1);
        assert_eq!(kv.session_count(), 1);
    }

    #[test]
    fn duplicate_get_is_original_result_new_sequence_is_new_read() {
        let mut kv = StateMachine::default();
        kv.apply(&entry(1, 1, 1, put("A")));
        let get = entry(2, 1, 2, Operation::Get { key: "x".into() });
        let original = kv.apply(&get);
        kv.apply(&entry(3, 2, 1, put("B")));
        let mut retry = get;
        retry.index = 4;
        assert_eq!(kv.apply(&retry), original);
        assert_eq!(original.result, Some("A".into()));
        assert_eq!(
            kv.apply(&entry(5, 1, 3, Operation::Get { key: "x".into() }))
                .result,
            Some("B".into())
        );
    }

    #[test]
    fn committed_prefix_replay_reconstructs_sessions_and_results() {
        let entries = [
            entry(1, 1, 0, put("A")),
            entry(2, 1, 1, Operation::Delete { key: "x".into() }),
            entry(3, 1, 0, put("A")),
            entry(
                4,
                2,
                2,
                Operation::Get {
                    key: "missing".into(),
                },
            ),
        ];
        let mut original = StateMachine::default();
        let results: Vec<_> = entries.iter().map(|e| original.apply(e)).collect();
        let mut replay = StateMachine::default();
        let replay_results: Vec<_> = entries.iter().map(|e| replay.apply(e)).collect();
        assert_eq!(original, replay);
        assert_eq!(results, replay_results);
        assert_eq!(replay.get("x"), None);
        assert_eq!(results[1].result, Some("A".into()));
        assert_eq!(replay.effect_count(), 3);
    }

    #[test]
    fn bounds_are_bytes_and_unseen_old_sequences_are_rejected() {
        let mut kv = StateMachine::default();
        kv.apply(&entry(1, 1, 5, put("A")));
        assert_eq!(
            kv.apply(&entry(2, 1, 4, put("B"))).status,
            "INVALID_ARGUMENT"
        );
        let huge = Operation::Put {
            key: "字".repeat(86),
            value: "B".into(),
        };
        assert_eq!(kv.apply(&entry(3, 2, 1, huge)).status, "INVALID_ARGUMENT");
        assert_eq!(kv.get("x"), Some("A"));
        assert_eq!(kv.session_count(), 1);
    }
}
