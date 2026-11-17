//! Deterministic applied state. The caller supplies only an already committed prefix.
use raft_protocol::{ClientResponse, Command, LogEntry, MAX_VALUE, Operation};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub use raft_protocol::MAX_SNAPSHOT;

/// The latest request a client made (dissertation section 6.3). A client has at
/// most one outstanding request, so only the newest one can still be retried.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Session {
    sequence: u64,
    operation: Operation,
    response: ClientResponse,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateMachine {
    applied_index: u64,
    effect_count: u64,
    values: BTreeMap<String, String>,
    sessions: BTreeMap<u128, Session>,
}
impl StateMachine {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }
    pub fn values(&self) -> &BTreeMap<String, String> {
        &self.values
    }
    /// Number of clients with a remembered request.
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }
    pub fn applied_index(&self) -> u64 {
        self.applied_index
    }
    /// First executions of logical requests; duplicate log entries do not increase it.
    /// Since S3 a GET never enters the log, so only a Get entry already in an
    /// older log is counted here.
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
        if let Some(session) = self.sessions.get(&request.client_id) {
            if request.sequence == session.sequence {
                return if session.operation == request.operation {
                    session.response.clone()
                } else {
                    rejection("REQUEST_ID_REUSE", entry.index)
                };
            }
            if request.sequence < session.sequence {
                // Older than the client's latest request: never executed again.
                return rejection("INVALID_ARGUMENT", entry.index);
            }
        }
        if !request.operation.valid() {
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
            request.client_id,
            Session {
                sequence: request.sequence,
                operation: request.operation.clone(),
                response: response.clone(),
            },
        );
        response
    }

    /// Deterministic bytes: equal states always serialize identically, because
    /// every collection is an ordered map and fields have a fixed order.
    pub fn snapshot(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("state machine serializes")
    }

    /// Rebuild the state a snapshot taken at `applied_index` describes.
    pub fn restore(bytes: &[u8], applied_index: u64) -> Result<Self, String> {
        if bytes.len() > MAX_SNAPSHOT {
            return Err("snapshot exceeds the size limit".into());
        }
        let state: Self =
            serde_json::from_slice(bytes).map_err(|e| format!("invalid snapshot: {e}"))?;
        if state.applied_index != applied_index {
            return Err(format!(
                "snapshot describes index {} but was installed as {applied_index}",
                state.applied_index
            ));
        }
        let pairs_valid = state.values.iter().all(|(key, value)| {
            Operation::Put {
                key: key.clone(),
                value: value.clone(),
            }
            .valid()
        });
        // A response carries a value that was stored earlier, so it obeys the same bound.
        let sessions_valid = state.sessions.values().all(|session| {
            session.operation.valid()
                && session
                    .response
                    .committed_index
                    .is_some_and(|i| i <= applied_index)
                && session
                    .response
                    .result
                    .as_ref()
                    .is_none_or(|value| value.len() <= MAX_VALUE)
        });
        if !pairs_valid || !sessions_valid || state.effect_count > applied_index {
            return Err("snapshot content violates state machine bounds".into());
        }
        Ok(state)
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

    fn noop(index: u64) -> LogEntry {
        LogEntry {
            index,
            term: 1,
            command: Command::NoOp,
        }
    }

    /// A mixed history: writes, deletes, reads, a retry, an ID reuse, an older
    /// sequence, an oversized value, several clients and Raft no-ops.
    fn history() -> Vec<LogEntry> {
        let get = |key: &str| Operation::Get { key: key.into() };
        let ops = [
            (1, 1, put("A")),
            (
                2,
                7,
                Operation::Put {
                    key: "y".into(),
                    value: "Y".into(),
                },
            ),
            (1, 1, put("A")),
            (1, 2, get("x")),
            (2, 7, put("other")),
            (3, 1, Operation::Delete { key: "y".into() }),
            (1, 1, put("A")),
            (
                2,
                8,
                Operation::Put {
                    key: "z".into(),
                    value: "Z".repeat(4097),
                },
            ),
            (u128::MAX, 0, put("max-client")),
            (2, 9, get("y")),
        ];
        let mut entries = vec![noop(1)];
        for (client, sequence, operation) in ops {
            entries.push(entry(entries.len() as u64 + 1, client, sequence, operation));
        }
        entries.push(noop(entries.len() as u64 + 1));
        entries
    }

    #[test]
    fn older_sequence_after_newer_is_rejected_without_effect() {
        let mut kv = StateMachine::default();
        let first = kv.apply(&entry(1, 1, 1, put("A")));
        kv.apply(&entry(2, 1, 2, put("B")));
        // A delayed copy of request 1 committed after request 2.
        let late = kv.apply(&entry(3, 1, 1, put("A")));
        assert_eq!(late.status, "INVALID_ARGUMENT");
        assert_ne!(
            late, first,
            "only the latest request per client is remembered"
        );
        assert_eq!(kv.get("x"), Some("B"));
        assert_eq!(kv.effect_count(), 2);
        assert_eq!(kv.session_count(), 1, "one record per client");
    }

    /// G4: restoring a snapshot taken after k entries is indistinguishable from
    /// replaying those k entries, including every later response.
    #[test]
    fn snapshot_restore_equals_replay_at_every_prefix() {
        let entries = history();
        let mut full = StateMachine::default();
        let expected: Vec<_> = entries.iter().map(|e| full.apply(e)).collect();
        for k in 0..=entries.len() {
            let mut prefix = StateMachine::default();
            entries[..k].iter().for_each(|e| {
                prefix.apply(e);
            });
            let mut restored = StateMachine::restore(&prefix.snapshot(), k as u64).unwrap();
            assert_eq!(restored, prefix, "k={k}");
            let rest: Vec<_> = entries[k..].iter().map(|e| restored.apply(e)).collect();
            assert_eq!(rest, expected[k..], "k={k}");
            assert_eq!(restored, full, "k={k}");
        }
        // expected[i] answers entries[i]; entries[0] is a no-op.
        assert_eq!(
            expected[3], expected[1],
            "the retry kept its original result"
        );
        assert_eq!(expected[5].status, "REQUEST_ID_REUSE");
        assert_eq!(expected[7].status, "INVALID_ARGUMENT", "older sequence");
        assert_eq!(expected[8].status, "INVALID_ARGUMENT", "oversized value");
        assert_eq!(full.session_count(), 4);
    }

    #[test]
    fn independent_replays_produce_identical_snapshot_bytes() {
        let entries = history();
        let snapshots: Vec<_> = (0..3)
            .map(|_| {
                let mut kv = StateMachine::default();
                entries.iter().for_each(|e| {
                    kv.apply(e);
                });
                kv.snapshot()
            })
            .collect();
        assert!(snapshots.windows(2).all(|pair| pair[0] == pair[1]));
        let text = String::from_utf8(snapshots[0].clone()).unwrap();
        let field = |name: &str| text.find(&format!("\"{name}\"")).unwrap();
        assert!(field("applied_index") < field("effect_count"));
        assert!(field("effect_count") < field("values"));
        assert!(field("values") < field("sessions"));
        assert!(
            text.contains(&format!("\"{}\"", u128::MAX)),
            "u128 client IDs survive JSON"
        );
    }

    #[test]
    fn restore_rejects_inconsistent_snapshots() {
        let entries = history();
        let mut kv = StateMachine::default();
        entries.iter().for_each(|e| {
            kv.apply(e);
        });
        let index = entries.len() as u64;
        let good = String::from_utf8(kv.snapshot()).unwrap();
        assert!(StateMachine::restore(good.as_bytes(), index).is_ok());
        let bad = [
            ("installed at another index", good.clone(), index + 1),
            ("not JSON", "{".to_string(), index),
            (
                "unknown field",
                good.replacen('{', "{\"extra\":1,", 1),
                index,
            ),
            (
                // "values" precedes "sessions", so the first match is the stored value.
                "oversized stored value",
                good.replacen("\"max-client\"", &format!("\"{}\"", "M".repeat(4097)), 1),
                index,
            ),
            (
                "oversized remembered response",
                good.replacen(
                    "\"result\":\"Y\"",
                    &format!("\"result\":\"{}\"", "Y".repeat(4097)),
                    1,
                ),
                index,
            ),
            (
                "session newer than the snapshot",
                good.replacen(
                    &format!("\"committed_index\":{}", index - 1),
                    &format!("\"committed_index\":{}", index + 5),
                    1,
                ),
                index,
            ),
            (
                "more effects than entries",
                good.replacen(
                    &format!("\"effect_count\":{}", kv.effect_count()),
                    &format!("\"effect_count\":{}", index + 1),
                    1,
                ),
                index,
            ),
        ];
        for (what, text, at) in bad {
            if at == index {
                assert_ne!(text, good, "{what}: the corruption must change something");
            }
            assert!(
                StateMachine::restore(text.as_bytes(), at).is_err(),
                "{what}"
            );
        }
        let huge = vec![b' '; MAX_SNAPSHOT + 1];
        assert!(StateMachine::restore(&huge, 0).is_err());
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

#[cfg(test)]
mod restore_bounds_tests {
    use super::*;
    /// The size limit is checked before parsing: valid JSON padded past it is refused.
    #[test]
    fn an_oversize_snapshot_is_refused_even_when_it_parses() {
        let mut bytes =
            br#"{"applied_index":0,"effect_count":0,"values":{},"sessions":{}}"#.to_vec();
        bytes.resize(MAX_SNAPSHOT + 1, b' ');
        assert!(StateMachine::restore(&bytes, 0).is_err());
    }
    /// A remembered request must itself be valid: here its key is 300 bytes.
    #[test]
    fn a_session_with_an_invalid_operation_is_refused() {
        let key = "k".repeat(300);
        let text = format!(
            r#"{{"applied_index":1,"effect_count":1,"values":{{}},"sessions":{{"1":{{"sequence":1,"operation":{{"Delete":{{"key":"{key}"}}}},"response":{{"status":"OK","result":null,"leader_hint":null,"committed_index":1}}}}}}}}"#
        );
        assert!(StateMachine::restore(text.as_bytes(), 1).is_err());
    }
}
