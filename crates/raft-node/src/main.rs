//! One actor owns Raft + storage + state machine. Connection workers only move messages.
use kv_state::StateMachine;
use raft_core::{Change, Core, Effect, Event, Role, bootstrap_state};
use raft_protocol::*;
use raft_storage::Wal;
use raft_transport::{DeadlineStream, correlated_response, read_frame, read_payload, write_frame};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{self, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const QUEUE: usize = 1024;
const CONNECTIONS: usize = 128;
const CLIENT_TIMEOUT: Duration = Duration::from_secs(3);
/// docs/MEMBERSHIP.md §10: a catch-up may take about 10 s, plus the commit.
const MEMBERSHIP_TIMEOUT: Duration = Duration::from_secs(15);
/// The minimum election timeout (docs/MEMBERSHIP.md §9.4).
const MIN_ELECTION: Duration = Duration::from_millis(400);
const PEER_TIMEOUT: Duration = Duration::from_millis(350);
/// Snapshot policy (dissertation section 5.1.2): compact once the WAL is larger
/// than this many times the previous snapshot, and at least this large.
const SNAPSHOT_EXPANSION: u64 = 4;
const SNAPSHOT_MIN_WAL_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeConfig {
    id: NodeId,
    peer_addr: SocketAddr,
    client_addr: SocketAddr,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Route {
    from: NodeId,
    to: NodeId,
    address: SocketAddr,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    cluster_id: String,
    nodes: Vec<NodeConfig>,
    #[serde(default)]
    routes: Vec<Route>,
}
/// The address book (docs/MEMBERSHIP.md §6): where nodes are, not who votes.
/// Who votes is the configuration in the Raft log.
impl Config {
    fn validate(&self) -> io::Result<()> {
        let ids: BTreeSet<_> = self.nodes.iter().map(|node| node.id).collect();
        let endpoints: BTreeSet<_> = self
            .nodes
            .iter()
            .flat_map(|node| [node.peer_addr, node.client_addr])
            .collect();
        if self.cluster_id != CLUSTER
            || self.nodes.is_empty()
            || !ids.iter().all(|id| valid_node(*id))
            || ids.len() != self.nodes.len()
            || endpoints.len() != 2 * self.nodes.len()
        {
            return Err(io::Error::other(
                "config requires distinct node IDs 1..=7, each with two unique endpoints",
            ));
        }
        if !self.routes.is_empty() {
            let expected: BTreeSet<_> = ids
                .iter()
                .flat_map(|a| ids.iter().filter(move |b| *b != a).map(move |b| (*a, *b)))
                .collect();
            let actual: BTreeSet<_> = self
                .routes
                .iter()
                .map(|route| (route.from, route.to))
                .collect();
            let addresses: BTreeSet<_> = self.routes.iter().map(|route| route.address).collect();
            if self.routes.len() != expected.len()
                || actual != expected
                || addresses.len() != expected.len()
                || !addresses.is_disjoint(&endpoints)
            {
                return Err(io::Error::other(
                    "routed topology requires a unique proxy endpoint for every direction",
                ));
            }
        }
        Ok(())
    }
    fn has(&self, id: NodeId) -> bool {
        self.nodes.iter().any(|node| node.id == id)
    }
    /// `None` for a node missing from this address book: it is unreachable.
    fn outbound(&self, from: NodeId, to: NodeId) -> Option<SocketAddr> {
        if self.routes.is_empty() {
            self.nodes
                .iter()
                .find(|node| node.id == to)
                .map(|node| node.peer_addr)
        } else {
            self.routes
                .iter()
                .find(|route| route.from == from && route.to == to)
                .map(|route| route.address)
        }
    }
}
/// A membership request on the client port (docs/MEMBERSHIP.md §10).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MembershipWireRequest {
    version: u8,
    membership: MembershipWire,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
enum MembershipWire {
    Add(NodeId),
    Remove(NodeId),
}

enum Input {
    #[cfg(test)]
    Stop,
    Peer(Envelope, Option<SyncSender<Envelope>>),
    RpcTimeout(NodeId, u64),
    Client(ClientRequest, SyncSender<ClientResponse>),
    Membership(Change, SyncSender<ClientResponse>),
    Status(SyncSender<Value>),
}

struct Active(Arc<AtomicUsize>);
impl Active {
    fn acquire(active: &Arc<AtomicUsize>) -> Option<Self> {
        active
            .try_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < CONNECTIONS).then_some(count + 1)
            })
            .ok()?;
        Some(Self(active.clone()))
    }
}
impl Drop for Active {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn peer_listener(
    listener: TcpListener,
    tx: SyncSender<Input>,
    id: NodeId,
    known: BTreeSet<NodeId>,
) {
    let active = Arc::new(AtomicUsize::new(0));
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let Some(guard) = Active::acquire(&active) else {
                continue;
            };
            let tx = tx.clone();
            let known = known.clone();
            thread::spawn(move || {
                let _guard = guard;
                let mut stream = stream;
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                loop {
                    let received = read_frame::<_, Envelope>(&mut DeadlineStream::new(
                        &mut stream,
                        Instant::now() + Duration::from_secs(2),
                    ));
                    let Ok(envelope) = received else {
                        break;
                    };
                    let sender_matches = match &envelope.body {
                        Rpc::RequestVote { candidate_id, .. } => *candidate_id == envelope.from,
                        Rpc::AppendEntries { leader_id, .. }
                        | Rpc::InstallSnapshot { leader_id, .. } => *leader_id == envelope.from,
                        _ => false,
                    };
                    // Only nodes in the address book may talk to us (§6).
                    if !envelope.valid_for(id) || !sender_matches || !known.contains(&envelope.from)
                    {
                        return;
                    }
                    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
                    if tx.try_send(Input::Peer(envelope, Some(reply_tx))).is_err() {
                        return;
                    }
                    let Ok(reply) = reply_rx.recv_timeout(Duration::from_secs(2)) else {
                        return;
                    };
                    if write_frame(&mut stream, &reply).is_err() {
                        return;
                    }
                }
            });
        }
    });
}

fn client_listener(listener: TcpListener, tx: SyncSender<Input>) {
    let active = Arc::new(AtomicUsize::new(0));
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let Some(guard) = Active::acquire(&active) else {
                continue;
            };
            let tx = tx.clone();
            thread::spawn(move || {
                let _guard = guard;
                let mut stream = stream;
                let _ = stream.set_read_timeout(Some(CLIENT_TIMEOUT));
                let _ = stream.set_write_timeout(Some(CLIENT_TIMEOUT));
                let Ok(payload) = read_payload(&mut DeadlineStream::new(
                    &mut stream,
                    Instant::now() + CLIENT_TIMEOUT,
                )) else {
                    return;
                };
                if let Ok(wire) = serde_json::from_slice::<ClientWireRequest>(&payload) {
                    let Ok(request) = wire.canonical() else {
                        let _ = write_frame(
                            &mut stream,
                            &ClientResponse::error("INVALID_ARGUMENT", None),
                        );
                        return;
                    };
                    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
                    let reply = if tx.try_send(Input::Client(request, reply_tx)).is_err() {
                        ClientResponse::error("TIMEOUT_UNKNOWN", None)
                    } else {
                        reply_rx
                            .recv_timeout(CLIENT_TIMEOUT)
                            .unwrap_or_else(|_| ClientResponse::error("TIMEOUT_UNKNOWN", None))
                    };
                    let _ = write_frame(&mut stream, &reply);
                } else if let Ok(wire) = serde_json::from_slice::<MembershipWireRequest>(&payload) {
                    if wire.version != 1 {
                        let _ = write_frame(
                            &mut stream,
                            &ClientResponse::error("INVALID_ARGUMENT", None),
                        );
                        return;
                    }
                    let change = match wire.membership {
                        MembershipWire::Add(node) => Change::Add(node),
                        MembershipWire::Remove(node) => Change::Remove(node),
                    };
                    let _ = stream.set_write_timeout(Some(MEMBERSHIP_TIMEOUT));
                    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
                    let reply = if tx.try_send(Input::Membership(change, reply_tx)).is_err() {
                        ClientResponse::error("TIMEOUT_UNKNOWN", None)
                    } else {
                        // The actor answers by its own deadline; this one only
                        // guards against a stuck actor.
                        reply_rx
                            .recv_timeout(MEMBERSHIP_TIMEOUT + Duration::from_secs(1))
                            .unwrap_or_else(|_| ClientResponse::error("TIMEOUT_UNKNOWN", None))
                    };
                    let _ = write_frame(&mut stream, &reply);
                } else if serde_json::from_slice::<Value>(&payload)
                    .ok()
                    .is_some_and(|value| value.get("status") == Some(&Value::Bool(true)))
                {
                    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
                    if tx.try_send(Input::Status(reply_tx)).is_ok()
                        && let Ok(reply) = reply_rx.recv_timeout(CLIENT_TIMEOUT)
                    {
                        let _ = write_frame(&mut stream, &reply);
                    }
                } else {
                    let _ = write_frame(
                        &mut stream,
                        &ClientResponse::error("INVALID_ARGUMENT", None),
                    );
                }
            });
        }
    });
}

type RequestKey = (u128, u64);
type Waiters = Vec<(ClientRequest, SyncSender<ClientResponse>)>;
/// A GET waiting for ReadIndex (docs/READ_INDEX.md §6.4).
struct WaitingRead {
    key: String,
    reply: SyncSender<ClientResponse>,
    deadline: Instant,
}
/// A membership request waiting for the core (docs/MEMBERSHIP.md §7, §10).
struct WaitingChange {
    reply: SyncSender<ClientResponse>,
    deadline: Instant,
}
struct Actor {
    core: Core,
    wal: Wal,
    kv: StateMachine,
    config: Config,
    tx: SyncSender<Input>,
    rx: Receiver<Input>,
    incoming: BTreeMap<(NodeId, u64), Vec<SyncSender<Envelope>>>,
    pending: BTreeMap<RequestKey, Waiters>,
    outbound_active: Arc<AtomicUsize>,
    rpc_deadlines: BTreeMap<(NodeId, u64), Instant>,
    election: Option<(Instant, u64)>,
    /// When the minimum election timeout since the last reset ends (§9.4).
    contact: Option<(Instant, u64)>,
    heartbeat: Option<(Instant, u64)>,
    random: u64,
    /// Test mode: compact every N applied entries instead of by WAL size.
    snapshot_every: Option<u64>,
    snapshots_taken: u64,
    snapshots_installed: u64,
    /// Largest state machine image this node turns into a snapshot
    /// ([`MAX_SNAPSHOT`]; tests use a small one).
    snapshot_limit: usize,
    /// Times the state machine was serialized for a snapshot, kept or not.
    snapshot_attempts: u64,
    /// After an image that exceeded the limit: the applied index before which
    /// no new attempt is made (0 when the last attempt fitted).
    oversize_retry_at: u64,
    reads: BTreeMap<u64, WaitingRead>,
    next_read: u64,
    reads_served: u64,
    reads_failed: u64,
    changes: BTreeMap<u64, WaitingChange>,
    next_change: u64,
}
impl Actor {
    fn trace(&self, kind: &str, details: Value) {
        let record = json!({"kind":kind,"node_id":self.core.id,"pid":std::process::id(),
            "role":format!("{:?}",self.core.role),"term":self.core.state.term,
            "vote":self.core.state.voted_for,"last_log_index":self.core.state.last_index(),
            "durable_index":self.core.durable_index,"durable_sequence":self.wal.durable_sequence(),
            "commit_index":self.core.commit_index,"last_applied":self.core.last_applied,
            "sync_us":details.get("sync_us"),"details":details});
        let mut stdout = io::stdout().lock();
        let _ = writeln!(stdout, "{record}");
        let _ = stdout.flush();
    }
    fn reset_election(&mut self, generation: u64) {
        self.random = self
            .random
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.election = Some((
            Instant::now() + Duration::from_millis(400 + self.random % 401),
            generation,
        ));
        self.contact = Some((Instant::now() + MIN_ELECTION, generation));
    }
    fn status(&self) -> Value {
        json!({"id":self.core.id,"pid":std::process::id(),"role":format!("{:?}",self.core.role),
            "term":self.core.state.term,"commit_index":self.core.commit_index,
            "last_applied":self.kv.applied_index(),"last_log_index":self.core.state.last_index(),
            "durable_index":self.core.durable_index,"durable_sequence":self.wal.durable_sequence(),
            "leader_hint":self.core.leader_hint,"kv":self.kv.values(),
            "session_count":self.kv.session_count(),"effect_count":self.kv.effect_count(),
            "snapshot_index":self.core.state.base().index,"snapshot_term":self.core.state.base().term,
            "snapshot_bytes":self.core.snapshot().map_or(0,|s|s.data.len()),
            "wal_bytes":self.wal.wal_bytes().unwrap_or(0),
            "snapshots_taken":self.snapshots_taken,"snapshots_installed":self.snapshots_installed,
            "snapshot_attempts":self.snapshot_attempts,"snapshot_retry_at":self.oversize_retry_at,
            "reads_served":self.reads_served,"reads_failed":self.reads_failed,"reads_waiting":self.reads.len(),
            "voters":self.core.configuration().map(|(_, voters)| voters.to_vec()),
            "config_index":self.core.configuration().map(|(index, _)| index),
            "config_committed":self.core.configuration().is_some_and(|(index, _)| index <= self.core.commit_index),
            "member":self.core.configuration().is_some_and(|(_, voters)| voters.contains(self.core.id)),
            "catching_up":self.core.catching_up(),"leader_contact":self.core.leader_contact(),
            "debug_only":true})
    }
    fn reject_pending(&mut self, status: &str) {
        for waiters in std::mem::take(&mut self.pending).into_values() {
            for (_, tx) in waiters {
                let _ = tx.try_send(ClientResponse::error(status, self.core.leader_hint));
            }
        }
    }
    fn reply_request(&mut self, request: &ClientRequest, response: ClientResponse) {
        let key = (request.client_id, request.sequence);
        if let Some(waiters) = self.pending.remove(&key) {
            let mut remaining = vec![];
            for (waiting, tx) in waiters {
                if waiting.operation == request.operation {
                    let _ = tx.try_send(response.clone());
                } else {
                    remaining.push((waiting, tx));
                }
            }
            if !remaining.is_empty() {
                self.pending.insert(key, remaining);
            }
        }
    }
    fn execute(&mut self, effects: Vec<Effect>) -> io::Result<()> {
        let mut queue = VecDeque::from(effects);
        while let Some(effect) = queue.pop_front() {
            match effect {
                Effect::Persist { barrier_id, state } => {
                    let previous_last = self.wal.state().last_index();
                    let started = Instant::now();
                    if let Err(error) = self.wal.save(&state) {
                        self.core.storage_failed();
                        self.trace(
                            "storage_failure",
                            json!({"error":error.to_string(),"barrier_id":barrier_id}),
                        );
                        self.reject_pending("STORAGE_ERROR");
                        return Err(error);
                    }
                    self.trace("persist",json!({"barrier_id":barrier_id,"sync_us":started.elapsed().as_micros() as u64,
                        "fsync_us":self.wal.last_sync_us(),
                        "measurement":"WAL save includes encoding/write/sync"}));
                    if state.last_index() > previous_last
                        && matches!(
                            state.entry(state.last_index()).map(|entry| &entry.command),
                            Some(Command::Client(_))
                        )
                    {
                        self.failpoint("after_sync_before_ack");
                    }
                    for effect in self.core.persisted(barrier_id).into_iter().rev() {
                        queue.push_front(effect);
                    }
                }
                Effect::PersistSnapshot {
                    barrier_id,
                    state,
                    snapshot,
                    restore,
                } => {
                    let started = Instant::now();
                    // Check the content can be loaded before anything is replaced.
                    let restored = if restore {
                        match StateMachine::restore(&snapshot.data, snapshot.base.index) {
                            Ok(kv) => Some(kv),
                            Err(error) => {
                                self.core.storage_failed();
                                self.trace("snapshot_rejected", json!({"error":error}));
                                self.reject_pending("STORAGE_ERROR");
                                return Err(io::Error::new(io::ErrorKind::InvalidData, error));
                            }
                        }
                    } else {
                        None
                    };
                    if let Err(error) = self.wal.install(&state, &snapshot) {
                        self.core.storage_failed();
                        self.trace(
                            "storage_failure",
                            json!({"error":error.to_string(),"barrier_id":barrier_id}),
                        );
                        self.reject_pending("STORAGE_ERROR");
                        return Err(error);
                    }
                    if let Some(kv) = restored {
                        self.kv = kv;
                        // A leader's snapshot fitted; ours may fit again too.
                        self.oversize_retry_at = 0;
                        self.snapshots_installed += 1;
                    } else {
                        self.snapshots_taken += 1;
                    }
                    self.trace(
                        "snapshot_persisted",
                        json!({"barrier_id":barrier_id,"index":snapshot.base.index,
                        "term":snapshot.base.term,"bytes":snapshot.data.len(),"restore":restore,
                        "install_us":started.elapsed().as_micros() as u64}),
                    );
                    for effect in self.core.persisted(barrier_id).into_iter().rev() {
                        queue.push_front(effect);
                    }
                }
                Effect::Send(envelope) => {
                    self.trace("rpc_send",json!({"to":envelope.to,"rpc_id":envelope.rpc_id,"rpc_term":envelope.body.term()}));
                    if matches!(
                        envelope.body,
                        Rpc::VoteResponse { .. }
                            | Rpc::AppendResponse { .. }
                            | Rpc::InstallSnapshotResponse { .. }
                    ) {
                        if let Some(waiters) = self.incoming.remove(&(envelope.to, envelope.rpc_id))
                        {
                            for tx in waiters {
                                let _ = tx.try_send(envelope.clone());
                            }
                        }
                    } else if let Some(guard) = Active::acquire(&self.outbound_active) {
                        self.rpc_deadlines.insert(
                            (envelope.to, envelope.rpc_id),
                            Instant::now() + PEER_TIMEOUT + Duration::from_millis(25),
                        );
                        let Some(address) = self.config.outbound(self.core.id, envelope.to) else {
                            // Not in our address book: retried at the same bounded
                            // cadence as an unreachable peer.
                            continue;
                        };
                        let tx = self.tx.clone();
                        thread::spawn(move || {
                            let _guard = guard;
                            let result = (|| -> io::Result<Envelope> {
                                let deadline = Instant::now() + PEER_TIMEOUT;
                                let mut stream =
                                    TcpStream::connect_timeout(&address, PEER_TIMEOUT)?;
                                let mut bounded = DeadlineStream::new(&mut stream, deadline);
                                write_frame(&mut bounded, &envelope)?;
                                read_frame(&mut bounded)
                            })();
                            match result {
                                Ok(response) if correlated_response(&envelope, &response) => {
                                    let _ = tx.try_send(Input::Peer(response, None));
                                }
                                _ => {
                                    let _ = tx
                                        .try_send(Input::RpcTimeout(envelope.to, envelope.rpc_id));
                                }
                            }
                        });
                    } else {
                        self.rpc_deadlines.insert(
                            (envelope.to, envelope.rpc_id),
                            Instant::now() + PEER_TIMEOUT,
                        );
                    }
                }
                Effect::Apply(entry) => {
                    if matches!(entry.command, Command::Client(_)) {
                        self.failpoint("after_quorum_before_apply");
                    }
                    let response = self.kv.apply(&entry);
                    self.trace(
                        "apply",
                        json!({"index":entry.index,"entry_term":entry.term}),
                    );
                    if let Command::Client(request) = &entry.command {
                        self.failpoint("after_apply_before_reply");
                        if self.core.role == Role::Leader {
                            self.reply_request(request, response);
                        }
                    }
                }
                Effect::ResetElection { generation } => self.reset_election(generation),
                Effect::ResetHeartbeat { generation } => {
                    self.heartbeat = Some((Instant::now() + Duration::from_millis(100), generation))
                }
                Effect::ReadReady { id, read_index } => {
                    if let Some(read) = self.reads.remove(&id) {
                        assert!(
                            self.kv.applied_index() >= read_index,
                            "ReadIndex step 4: the state machine reached readIndex"
                        );
                        self.reads_served += 1;
                        self.trace(
                            "read_served",
                            json!({"read_id":id,"read_index":read_index,
                            "applied_index":self.kv.applied_index()}),
                        );
                        let _ = read.reply.try_send(ClientResponse {
                            status: "OK".into(),
                            result: self.kv.get(&read.key).map(str::to_owned),
                            leader_hint: Some(self.core.id),
                            committed_index: Some(self.kv.applied_index()),
                        });
                    }
                }
                Effect::ReadFailed { id, status } => {
                    if let Some(read) = self.reads.remove(&id) {
                        self.reads_failed += 1;
                        self.trace("read_failed", json!({"read_id":id,"status":status}));
                        let _ = read
                            .reply
                            .try_send(ClientResponse::error(status, self.core.leader_hint));
                    }
                }
                Effect::Reject(request, status) => self.reply_request(
                    &request,
                    ClientResponse::error(status, self.core.leader_hint),
                ),
                Effect::MembershipDone { id, voters, index } => {
                    self.trace(
                        "membership_done",
                        json!({"change_id":id,"voters":voters.to_vec(),"index":index}),
                    );
                    if let Some(waiting) = self.changes.remove(&id) {
                        let _ = waiting.reply.try_send(ClientResponse {
                            status: "OK".into(),
                            result: Some(voters.to_string()),
                            leader_hint: Some(self.core.id),
                            committed_index: Some(index),
                        });
                    }
                }
                Effect::MembershipFailed { id, status } => {
                    self.trace("membership_failed", json!({"change_id":id,"status":status}));
                    if let Some(waiting) = self.changes.remove(&id) {
                        let _ = waiting
                            .reply
                            .try_send(ClientResponse::error(status, self.core.leader_hint));
                    }
                }
            }
        }
        Ok(())
    }
    fn failpoint(&self, name: &str) {
        let control = std::env::var_os("RAFT_FAILPOINT_FILE")
            .and_then(|path| std::fs::read_to_string(path).ok());
        if std::env::var("RAFT_FAILPOINT").ok().as_deref() == Some(name)
            || control.as_deref().is_some_and(|value| value.trim() == name)
        {
            self.trace("failpoint", json!({"name":name}));
            std::process::exit(91);
        }
    }
    fn step(&mut self, event: Event) -> io::Result<()> {
        let role = self.core.role;
        let details = match &event {
            Event::Message(envelope) => {
                json!({"event":"message","from":envelope.from,"rpc_id":envelope.rpc_id,"rpc_term":envelope.body.term()})
            }
            Event::Propose(request) => {
                json!({"event":"propose","client_id":request.client_id.to_string(),"sequence":request.sequence})
            }
            Event::ElectionTimeout { generation } => {
                json!({"event":"election_timeout","generation":generation})
            }
            Event::Heartbeat { generation } => json!({"event":"heartbeat","generation":generation}),
            Event::RpcTimeout { peer, rpc_id } => {
                json!({"event":"rpc_timeout","peer":peer,"rpc_id":rpc_id})
            }
            Event::Compact { index, data } => {
                json!({"event":"compact","index":index,"bytes":data.len()})
            }
            Event::Read { id } => json!({"event":"read","read_id":id}),
            Event::CancelRead { id } => json!({"event":"cancel_read","read_id":id}),
            Event::ChangeMembership { id, change } => {
                json!({"event":"change_membership","change_id":id,"change":format!("{change:?}")})
            }
            Event::CancelMembership { id } => json!({"event":"cancel_membership","change_id":id}),
            Event::LeaderContactExpired { generation } => {
                json!({"event":"leader_contact_expired","generation":generation})
            }
        };
        self.trace("event", details);
        let effects = self.core.step(event);
        self.execute(effects)?;
        self.maybe_compact()?;
        if role == Role::Leader && self.core.role != Role::Leader {
            self.reject_pending("TIMEOUT_UNKNOWN");
        }
        if self.core.role == Role::Leader {
            self.election = None;
        } else {
            self.heartbeat = None;
            // A leader has no election timer. Higher-term replies may demote it
            // without a vote/AppendEntries timer reset; start the missing timer.
            if self.election.is_none() {
                self.reset_election(self.core.election_generation);
            }
        }
        Ok(())
    }
    /// docs/SNAPSHOT_COMPACTION.md §8. Runs between events, so no durability
    /// barrier is pending and the state machine is exactly at `last_applied`.
    fn maybe_compact(&mut self) -> io::Result<()> {
        let applied = self.core.last_applied;
        let base = self.core.state.base().index;
        if applied <= base || self.core.failed() {
            return Ok(());
        }
        let due = match self.snapshot_every {
            Some(every) => applied - base >= every,
            None => {
                let previous = self.core.snapshot().map_or(0, |s| s.data.len() as u64);
                self.wal.wal_bytes()? >= SNAPSHOT_MIN_WAL_BYTES.max(SNAPSHOT_EXPANSION * previous)
            }
        };
        if !due || applied < self.oversize_retry_at {
            return Ok(());
        }
        assert_eq!(
            self.kv.applied_index(),
            applied,
            "state machine and Raft disagree"
        );
        let started = Instant::now();
        self.snapshot_attempts += 1;
        let data = self.kv.snapshot();
        if data.len() > self.snapshot_limit {
            // N1: this node stops compacting and its log grows again. Finding
            // that out costs a full serialization (50 to 100 ms at the limit),
            // and "due" stays true from here on, so without a pause it would
            // be paid after every single event (lesson 13). Try again only
            // when the log since the last snapshot has doubled; deletes may
            // have made the state fit by then.
            self.oversize_retry_at = applied + (applied - base);
            self.trace(
                "snapshot_too_large",
                json!({"bytes":data.len(),"limit":self.snapshot_limit,"index":applied,
                "retry_at":self.oversize_retry_at,
                "serialize_us":started.elapsed().as_micros() as u64}),
            );
            return Ok(());
        }
        self.oversize_retry_at = 0;
        self.trace(
            "snapshot_serialized",
            json!({"index":applied,"bytes":data.len(),
            "serialize_us":started.elapsed().as_micros() as u64}),
        );
        let effects = self.core.step(Event::Compact {
            index: applied,
            data,
        });
        self.execute(effects)
    }
    fn run(&mut self) -> io::Result<()> {
        self.reset_election(self.core.election_generation);
        self.trace(
            "started",
            json!({"routed":!self.config.routes.is_empty(),"recovery_apply":0,
            "voters":self.core.configuration().map(|(_, voters)| voters.to_vec())}),
        );
        loop {
            let now = Instant::now();
            if let Some(key) = self
                .rpc_deadlines
                .iter()
                .find(|(_, deadline)| **deadline <= now)
                .map(|(key, _)| *key)
            {
                self.rpc_deadlines.remove(&key);
                self.step(Event::RpcTimeout {
                    peer: key.0,
                    rpc_id: key.1,
                })?;
                continue;
            }
            if self.election.is_some_and(|(deadline, _)| deadline <= now) {
                let (_, generation) = self.election.take().expect("checked timer");
                self.step(Event::ElectionTimeout { generation })?;
                continue;
            }
            if self.heartbeat.is_some_and(|(deadline, _)| deadline <= now) {
                let (_, generation) = self.heartbeat.take().expect("checked timer");
                self.step(Event::Heartbeat { generation })?;
                continue;
            }
            if self.contact.is_some_and(|(deadline, _)| deadline <= now) {
                let (_, generation) = self.contact.take().expect("checked timer");
                self.step(Event::LeaderContactExpired { generation })?;
                continue;
            }
            // A membership request whose deadline passed: the core forgets it
            // and the administrator hears TIMEOUT_UNKNOWN (§7 step 10).
            if let Some(id) = self
                .changes
                .iter()
                .find(|(_, change)| change.deadline <= now)
                .map(|(id, _)| *id)
            {
                let waiting = self.changes.remove(&id).expect("found");
                let _ = waiting.reply.try_send(ClientResponse::error(
                    "TIMEOUT_UNKNOWN",
                    self.core.leader_hint,
                ));
                self.step(Event::CancelMembership { id })?;
                continue;
            }
            // A read whose client has given up is forgotten, so an isolated
            // leader's waiting list cannot grow without bound.
            if let Some(id) = self
                .reads
                .iter()
                .find(|(_, read)| read.deadline <= now)
                .map(|(id, _)| *id)
            {
                self.reads.remove(&id);
                self.step(Event::CancelRead { id })?;
                continue;
            }
            let wait = self
                .election
                .into_iter()
                .chain(self.heartbeat)
                .chain(self.contact)
                .map(|(deadline, _)| deadline.saturating_duration_since(now))
                .chain(
                    self.rpc_deadlines
                        .values()
                        .chain(self.reads.values().map(|read| &read.deadline))
                        .chain(self.changes.values().map(|change| &change.deadline))
                        .map(|deadline| deadline.saturating_duration_since(now)),
                )
                .min()
                .unwrap_or(Duration::from_millis(100));
            match self.rx.recv_timeout(wait) {
                #[cfg(test)]
                Ok(Input::Stop) => return Ok(()),
                Ok(Input::Status(reply)) => {
                    let _ = reply.try_send(self.status());
                }
                Ok(Input::Peer(envelope, reply)) => {
                    if let Some(reply) = reply {
                        if self.incoming.len() >= QUEUE {
                            continue;
                        }
                        self.incoming
                            .entry((envelope.from, envelope.rpc_id))
                            .or_default()
                            .push(reply);
                    } else {
                        self.rpc_deadlines.remove(&(envelope.from, envelope.rpc_id));
                    }
                    self.step(Event::Message(envelope))?;
                }
                Ok(Input::RpcTimeout(peer, rpc_id)) => {
                    // An immediate connect/refusal/EOF is not permission to spin.
                    // Keep the original total deadline; the timer retires and
                    // retries this correlation with the same bounded cadence.
                    if self.rpc_deadlines.contains_key(&(peer, rpc_id)) {
                        self.trace(
                            "transport_failure",
                            json!({"peer":peer,"rpc_id":rpc_id,"retry":"at_original_deadline"}),
                        );
                    }
                }
                Ok(Input::Membership(change, reply)) => {
                    // The leader must be able to reach a node it adds (§10).
                    if let Change::Add(node) = change
                        && !self.config.has(node)
                    {
                        let _ = reply.try_send(ClientResponse::error(
                            "INVALID_ARGUMENT",
                            self.core.leader_hint,
                        ));
                        continue;
                    }
                    self.next_change += 1;
                    let id = self.next_change;
                    self.changes.insert(
                        id,
                        WaitingChange {
                            reply,
                            deadline: Instant::now() + MEMBERSHIP_TIMEOUT,
                        },
                    );
                    self.step(Event::ChangeMembership { id, change })?;
                }
                Ok(Input::Client(request, reply)) => {
                    if self.pending.values().map(Vec::len).sum::<usize>()
                        + self.reads.len()
                        + self.changes.len()
                        >= QUEUE
                    {
                        let _ = reply.try_send(ClientResponse::error(
                            "TIMEOUT_UNKNOWN",
                            self.core.leader_hint,
                        ));
                        continue;
                    }
                    if !request.operation.valid() {
                        let _ = reply.try_send(ClientResponse::error(
                            "INVALID_ARGUMENT",
                            self.core.leader_hint,
                        ));
                        continue;
                    }
                    // S3: a GET never enters the log (docs/READ_INDEX.md).
                    if let Operation::Get { key } = request.operation {
                        self.next_read += 1;
                        let id = self.next_read;
                        self.reads.insert(
                            id,
                            WaitingRead {
                                key,
                                reply,
                                deadline: Instant::now() + CLIENT_TIMEOUT,
                            },
                        );
                        self.step(Event::Read { id })?;
                        continue;
                    }
                    self.pending
                        .entry((request.client_id, request.sequence))
                        .or_default()
                        .push((request.clone(), reply));
                    self.step(Event::Propose(request))?;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            }
        }
    }
}

fn main() -> io::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!(
            "Usage: raft-node --id N --config FILE --data-dir PATH [--bootstrap] [--snapshot-every N]\n--id N: 1..=7, listed in the config's address book.\n--bootstrap: on an empty data directory only, start a new cluster whose\n  configuration is this node alone (docs/MEMBERSHIP.md §6). Bootstrap exactly\n  one node, once; add the others with tools/kv_admin.py.\n--snapshot-every N: test mode, compact after every N applied entries.\nDebug status: framed JSON {{\"status\":true}}; no local client GET."
        );
        return Ok(());
    }
    let mut id = None;
    let mut config_path = None;
    let mut data_dir = None;
    let mut snapshot_every = None;
    let mut bootstrap = false;
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        if flag == "--bootstrap" {
            bootstrap = true;
            continue;
        }
        let value = args
            .next()
            .ok_or_else(|| io::Error::other("missing CLI argument value; see --help"))?;
        match flag.as_str() {
            "--id" => id = Some(value.parse::<u8>().map_err(io::Error::other)?),
            "--config" => config_path = Some(PathBuf::from(value)),
            "--data-dir" => data_dir = Some(PathBuf::from(value)),
            "--snapshot-every" => {
                let every = value.parse::<u64>().map_err(io::Error::other)?;
                if every == 0 {
                    return Err(io::Error::other("--snapshot-every must be at least 1"));
                }
                snapshot_every = Some(every);
            }
            _ => return Err(io::Error::other("unknown CLI argument; see --help")),
        }
    }
    let id = id
        .filter(|id| valid_node(*id))
        .ok_or_else(|| io::Error::other("--id must be 1..=7"))?;
    let config_path = config_path.ok_or_else(|| io::Error::other("--config is required"))?;
    let data_dir = data_dir.ok_or_else(|| io::Error::other("--data-dir is required"))?;
    let config: Config =
        toml::from_str(&std::fs::read_to_string(config_path)?).map_err(io::Error::other)?;
    config.validate()?;
    let own = config
        .nodes
        .iter()
        .find(|node| node.id == id)
        .ok_or_else(|| io::Error::other("--id is not in the config's address book"))?;
    let peer = TcpListener::bind(own.peer_addr)?;
    let client = TcpListener::bind(own.client_addr)?;
    let mut wal = Wal::open(data_dir, id)?;
    // docs/MEMBERSHIP.md §6: only a never-used directory is bootstrapped, so the
    // flag is harmless on restarts. The entry is durable before we serve.
    let bootstrap = if !bootstrap {
        "not_requested"
    } else if *wal.state() == DurableState::default() && wal.snapshot().is_none() {
        wal.save(&bootstrap_state(id))?;
        "bootstrapped"
    } else {
        "ignored_existing_data"
    };
    println!(
        "{}",
        json!({"kind":"bootstrap","node_id":id,"outcome":bootstrap})
    );
    wal.inject_sync_failure(std::env::var("RAFT_SYNC_FAILURE").ok().as_deref() == Some("1"));
    // Entries up to the snapshot are committed; everything after it is relearned.
    let kv = match wal.snapshot() {
        Some(snapshot) => StateMachine::restore(&snapshot.data, snapshot.base.index)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
        None => StateMachine::default(),
    };
    let core = Core::restore(id, wal.state().clone(), wal.snapshot().cloned())
        .map_err(io::Error::other)?;
    let (tx, rx) = mpsc::sync_channel(QUEUE);
    let known = config.nodes.iter().map(|node| node.id).collect();
    peer_listener(peer, tx.clone(), id, known);
    client_listener(client, tx.clone());
    let random = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
        ^ u64::from(id).wrapping_mul(77777);
    Actor {
        core,
        wal,
        kv,
        config,
        tx,
        rx,
        incoming: BTreeMap::new(),
        pending: BTreeMap::new(),
        outbound_active: Arc::new(AtomicUsize::new(0)),
        rpc_deadlines: BTreeMap::new(),
        election: None,
        contact: None,
        heartbeat: None,
        random,
        snapshot_every,
        snapshots_taken: 0,
        snapshots_installed: 0,
        snapshot_limit: MAX_SNAPSHOT,
        snapshot_attempts: 0,
        oversize_retry_at: 0,
        reads: BTreeMap::new(),
        next_read: 0,
        reads_served: 0,
        reads_failed: 0,
        changes: BTreeMap::new(),
        next_change: 0,
    }
    .run()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saturated_outbound_pool_does_not_starve_status() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/node-pool-tests")
            .join(format!(
                "{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        let wal = Wal::open(&dir, 1).unwrap();
        // Index 1 configures nodes 1, 2 and 3: since S4 a leader replicates only
        // to the voters of its configuration, and without one it would send
        // nothing and the pool would never be exercised (lesson 07 again).
        let mut core = Core::new(
            1,
            DurableState::new(
                1,
                Some(1),
                vec![LogEntry {
                    index: 1,
                    term: 1,
                    command: Command::Config {
                        voters: vec![1, 2, 3],
                    },
                }],
            ),
        )
        .unwrap();
        core.role = Role::Leader;
        let config: Config = toml::from_str(include_str!("../../../config/cluster.toml")).unwrap();
        let (tx, rx) = mpsc::sync_channel(QUEUE);
        let mut actor = Actor {
            core,
            wal,
            kv: StateMachine::default(),
            config,
            tx: tx.clone(),
            rx,
            incoming: BTreeMap::new(),
            pending: BTreeMap::new(),
            outbound_active: Arc::new(AtomicUsize::new(CONNECTIONS)),
            rpc_deadlines: BTreeMap::new(),
            election: None,
            contact: None,
            heartbeat: None,
            random: 1,
            snapshot_every: None,
            snapshots_taken: 0,
            snapshots_installed: 0,
            snapshot_limit: MAX_SNAPSHOT,
            snapshot_attempts: 0,
            oversize_retry_at: 0,
            reads: BTreeMap::new(),
            next_read: 0,
            reads_served: 0,
            reads_failed: 0,
            changes: BTreeMap::new(),
            next_change: 0,
        };
        let (client_tx, _client_rx) = mpsc::sync_channel(1);
        // A write, so that the leader must replicate into the saturated pool.
        // Until S3 this was a GET, which also replicated; since S3 a GET waits
        // for ReadIndex and sends nothing, so the test stopped exercising the
        // pool (docs/lessons/07-test-silently-stopped-testing/).
        tx.send(Input::Client(
            ClientRequest {
                client_id: 1,
                sequence: 1,
                operation: Operation::Put {
                    key: "x".into(),
                    value: "v".into(),
                },
            },
            client_tx,
        ))
        .unwrap();
        let (status_tx, status_rx) = mpsc::sync_channel(1);
        tx.send(Input::Status(status_tx)).unwrap();
        let worker = thread::spawn(move || actor.run());
        let status = status_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("pool saturation must not starve queued status");
        assert_eq!(status["role"], "Leader");
        tx.send(Input::Stop).unwrap();
        worker.join().unwrap().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// docs/SNAPSHOT_COMPACTION.md N1 (lesson 13): once the state machine no
    /// longer fits a snapshot, the node must not serialize it after every
    /// event just to find that out again. The limit is 64 bytes here so that
    /// ten small entries exceed it.
    #[test]
    fn an_oversize_state_is_not_serialized_again_until_the_log_doubles() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/node-pool-tests")
            .join(format!(
                "oversize-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        let put = |index: u64| LogEntry {
            index,
            term: 1,
            command: Command::Client(ClientRequest {
                client_id: 7,
                sequence: index,
                operation: Operation::Put {
                    key: format!("key-{index}"),
                    value: "v".repeat(16),
                },
            }),
        };
        let mut core = Core::new(
            1,
            DurableState::new(
                1,
                Some(1),
                vec![LogEntry {
                    index: 1,
                    term: 1,
                    command: Command::Config {
                        voters: vec![1, 2, 3],
                    },
                }],
            ),
        )
        .unwrap();
        let mut kv = StateMachine::default();
        kv.apply(core.state.entry(1).unwrap());
        core.commit_index = 1;
        core.last_applied = 1;
        let config: Config = toml::from_str(include_str!("../../../config/cluster.toml")).unwrap();
        let (tx, rx) = mpsc::sync_channel(QUEUE);
        let mut actor = Actor {
            core,
            wal: Wal::open(&dir, 1).unwrap(),
            kv,
            config,
            tx,
            rx,
            incoming: BTreeMap::new(),
            pending: BTreeMap::new(),
            outbound_active: Arc::new(AtomicUsize::new(0)),
            rpc_deadlines: BTreeMap::new(),
            election: None,
            contact: None,
            heartbeat: None,
            random: 1,
            snapshot_every: Some(1),
            snapshots_taken: 0,
            snapshots_installed: 0,
            snapshot_limit: 64,
            snapshot_attempts: 0,
            oversize_retry_at: 0,
            reads: BTreeMap::new(),
            next_read: 0,
            reads_served: 0,
            reads_failed: 0,
            changes: BTreeMap::new(),
            next_change: 0,
        };
        // What the actor does between events: apply what is committed, then
        // look whether a snapshot is due.
        let apply_through = |actor: &mut Actor, last: u64| {
            for index in actor.core.last_applied + 1..=last {
                actor.core.state.push(put(index));
                actor.kv.apply(&put(index));
                actor.core.commit_index = index;
                actor.core.last_applied = index;
                actor.maybe_compact().unwrap();
            }
        };
        // The assertions below do not depend on which index first exceeds the
        // limit, only on what happens after one did.
        apply_through(&mut actor, 10);
        assert!(
            actor.kv.snapshot().len() > 64,
            "ten entries exceed the limit"
        );
        let first_failure = actor.oversize_retry_at;
        assert!(first_failure > 0, "an oversize image was met");
        let attempts = actor.snapshot_attempts;
        let base = actor.core.state.base().index;
        // Events that apply nothing new: no further serialization at all.
        for _ in 0..50 {
            actor.maybe_compact().unwrap();
        }
        assert_eq!(
            actor.snapshot_attempts, attempts,
            "an oversize state is not serialized again after every event"
        );
        // Not before the retry index either ...
        let retry_at = actor.oversize_retry_at;
        apply_through(&mut actor, retry_at - 1);
        assert_eq!(actor.snapshot_attempts, attempts);
        // ... and exactly once when it is reached; the gap then doubles.
        apply_through(&mut actor, retry_at);
        assert_eq!(actor.snapshot_attempts, attempts + 1);
        assert_eq!(actor.oversize_retry_at, retry_at + (retry_at - base));
        assert_eq!(actor.core.state.base().index, base, "nothing was compacted");
        // When the image fits again, compaction resumes at the next attempt.
        actor.snapshot_limit = MAX_SNAPSHOT;
        let retry_at = actor.oversize_retry_at;
        apply_through(&mut actor, retry_at);
        assert_eq!(actor.core.state.base().index, retry_at);
        assert_eq!(actor.oversize_retry_at, 0);
        assert!(actor.snapshots_taken >= 1);
        drop(actor);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// docs/MEMBERSHIP.md §6: the address book lists any distinct IDs 1..=7; if
    /// routed, every direction between them needs its own proxy endpoint.
    #[test]
    fn address_book_accepts_any_nodes_one_to_seven_with_complete_routes() {
        let book = |ids: &[u8]| Config {
            cluster_id: CLUSTER.into(),
            nodes: ids
                .iter()
                .map(|id| NodeConfig {
                    id: *id,
                    peer_addr: format!("127.0.0.1:{}", 7100 + u16::from(*id))
                        .parse()
                        .unwrap(),
                    client_addr: format!("127.0.0.1:{}", 7200 + u16::from(*id))
                        .parse()
                        .unwrap(),
                })
                .collect(),
            routes: vec![],
        };
        for ids in [&[1][..], &[2, 5], &[1, 2, 3, 4, 5], &[1, 2, 3, 4, 5, 6, 7]] {
            assert!(book(ids).validate().is_ok(), "{ids:?}");
        }
        assert!(book(&[]).validate().is_err(), "empty");
        assert!(book(&[1, 8]).validate().is_err(), "node 8");
        assert!(book(&[0, 1]).validate().is_err(), "node 0");
        let mut duplicate = book(&[1, 2]);
        duplicate.nodes[1].id = 1;
        duplicate.nodes[1].peer_addr = "127.0.0.1:7999".parse().unwrap();
        assert!(duplicate.validate().is_err(), "duplicate ID");
        let mut routed = book(&[1, 2, 3, 4, 5]);
        let mut port = 7300;
        for from in 1..=5 {
            for to in 1..=5 {
                if from != to {
                    port += 1;
                    routed.routes.push(Route {
                        from,
                        to,
                        address: format!("127.0.0.1:{port}").parse().unwrap(),
                    });
                }
            }
        }
        assert_eq!(routed.routes.len(), 20);
        assert!(routed.validate().is_ok());
        assert_eq!(
            routed.outbound(4, 5),
            // Routes are numbered in (from, to) order: (4, 5) is the 16th.
            Some("127.0.0.1:7316".parse().unwrap())
        );
        assert_eq!(routed.outbound(4, 6), None, "not in the address book");
        routed.routes.pop();
        assert!(routed.validate().is_err(), "one direction missing");
        assert!(book(&[1, 2]).has(2) && !book(&[1, 2]).has(3));
    }

    #[test]
    fn config_rejects_partial_routes_and_wrong_cluster() {
        let mut config = Config {
            cluster_id: CLUSTER.into(),
            nodes: [1, 2, 3]
                .into_iter()
                .map(|id| NodeConfig {
                    id,
                    peer_addr: format!("127.0.0.1:{}", 7100 + u16::from(id))
                        .parse()
                        .unwrap(),
                    client_addr: format!("127.0.0.1:{}", 7200 + u16::from(id))
                        .parse()
                        .unwrap(),
                })
                .collect(),
            routes: vec![],
        };
        assert!(config.validate().is_ok());
        config.cluster_id = "other".into();
        assert!(config.validate().is_err());
        config.cluster_id = CLUSTER.into();
        config.routes.push(Route {
            from: 1,
            to: 2,
            address: "127.0.0.1:7301".parse().unwrap(),
        });
        assert!(config.validate().is_err());
        config.routes.clear();
        config.nodes[1].client_addr = config.nodes[0].peer_addr;
        assert!(config.validate().is_err());
    }
}
