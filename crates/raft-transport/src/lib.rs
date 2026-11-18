//! Bounded length-prefixed JSON framing; consensus state stays in raft-core.
use raft_protocol::{Envelope, MAX_FRAME, Rpc};
use serde::{Serialize, de::DeserializeOwned};
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

/// A total I/O deadline, unlike a socket timeout restarted by every partial read.
pub struct DeadlineStream<'a> {
    stream: &'a mut TcpStream,
    deadline: Instant,
}
impl<'a> DeadlineStream<'a> {
    pub fn new(stream: &'a mut TcpStream, deadline: Instant) -> Self {
        Self { stream, deadline }
    }
    fn remaining(&self) -> io::Result<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "total I/O deadline expired"))
    }
}
impl Read for DeadlineStream<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.stream.set_read_timeout(Some(self.remaining()?))?;
        self.stream.read(buffer)
    }
}
impl Write for DeadlineStream<'_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.stream.set_write_timeout(Some(self.remaining()?))?;
        self.stream.write(buffer)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.remaining()?;
        self.stream.flush()
    }
}

pub fn read_frame<R: Read, T: DeserializeOwned>(reader: &mut R) -> io::Result<T> {
    let payload = read_payload(reader)?;
    serde_json::from_slice(&payload)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

pub fn read_payload<R: Read>(reader: &mut R) -> io::Result<Vec<u8>> {
    let mut header = [0; 4];
    reader.read_exact(&mut header)?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame length outside bounds",
        ));
    }
    let mut payload = vec![0; length];
    reader.read_exact(&mut payload)?;
    Ok(payload)
}

pub fn write_frame<W: Write, T: Serialize>(writer: &mut W, value: &T) -> io::Result<()> {
    let payload = serde_json::to_vec(value).map_err(io::Error::other)?;
    if payload.is_empty() || payload.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame length outside bounds",
        ));
    }
    writer.write_all(&(payload.len() as u32).to_be_bytes())?;
    writer.write_all(&payload)?;
    writer.flush()
}

/// A connection belongs to one request. Never deliver an unrelated reply to core.
/// Term/leadership-epoch/in-flight log metadata are then checked by the core.
pub fn correlated_response(request: &Envelope, response: &Envelope) -> bool {
    response.valid_for(request.from)
        && response.from == request.to
        && response.rpc_id == request.rpc_id
        && matches!(
            (&request.body, &response.body),
            (Rpc::RequestVote { .. }, Rpc::VoteResponse { .. })
                | (Rpc::AppendEntries { .. }, Rpc::AppendResponse { .. })
                | (
                    Rpc::InstallSnapshot { .. },
                    Rpc::InstallSnapshotResponse { .. }
                )
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use raft_protocol::CLUSTER;
    use std::io::Cursor;
    #[test]
    fn total_deadline_bounds_slow_fragmented_tcp() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let sender = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let payload = serde_json::to_vec(&"slow response").unwrap();
            let packet = [(payload.len() as u32).to_be_bytes().to_vec(), payload].concat();
            for byte in packet {
                if stream.write_all(&[byte]).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(40));
            }
        });
        let mut stream = TcpStream::connect(address).unwrap();
        let began = Instant::now();
        let mut bounded = DeadlineStream::new(&mut stream, began + Duration::from_millis(100));
        let error = read_frame::<_, String>(&mut bounded).unwrap_err();
        assert!(matches!(
            error.kind(),
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
        ));
        assert!(began.elapsed() < Duration::from_millis(600));
        drop(stream);
        sender.join().unwrap();
    }

    struct Fragmented<T>(T);
    impl<T: Read> Read for Fragmented<T> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let size = buffer.len().min(1);
            self.0.read(&mut buffer[..size])
        }
    }
    impl<T: Write> Write for Fragmented<T> {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.0.write(&buffer[..buffer.len().min(2)])
        }
        fn flush(&mut self) -> io::Result<()> {
            self.0.flush()
        }
    }

    /// A length of 0 is refused by the length check itself. Without it the
    /// empty payload reached the caller, and the node's client listener, which
    /// reads raw payloads, only found out when JSON parsing failed
    /// (2026-11-17 storage review, left open in S10).
    #[test]
    fn a_zero_length_frame_is_refused() {
        let error = read_payload(&mut Cursor::new([0, 0, 0, 0, b'{', b'}'])).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(error.to_string(), "frame length outside bounds");
    }

    #[test]
    fn tcp_framing_handles_partial_io() {
        let mut output = Fragmented(Vec::new());
        write_frame(&mut output, &serde_json::json!({"hello":"台灣"})).unwrap();
        write_frame(&mut output, &serde_json::json!([1, 2, 3])).unwrap();
        let mut input = Fragmented(Cursor::new(output.0));
        let first: serde_json::Value = read_frame(&mut input).unwrap();
        let second: serde_json::Value = read_frame(&mut input).unwrap();
        assert_eq!(first["hello"], "台灣");
        assert_eq!(second, serde_json::json!([1, 2, 3]));
        let oversized = ((MAX_FRAME + 1) as u32).to_be_bytes();
        let error = read_frame::<_, serde_json::Value>(&mut Cursor::new(oversized)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(read_frame::<_, serde_json::Value>(&mut Cursor::new([0, 0, 0, 4, b'{'])).is_err());
        assert!(read_frame::<_, serde_json::Value>(&mut Cursor::new([0, 0, 0, 1, b'x'])).is_err());
        // Do not round-trip requests through serde_json::Value: u128 IDs must
        // retain all bits instead of being coerced into a floating point value.
        let request = raft_protocol::ClientRequest {
            client_id: u128::MAX,
            sequence: 1,
            operation: raft_protocol::Operation::Get { key: "x".into() },
        };
        let mut bytes = vec![];
        write_frame(&mut bytes, &request).unwrap();
        assert_eq!(
            read_frame::<_, raft_protocol::ClientRequest>(&mut Cursor::new(bytes)).unwrap(),
            request
        );
    }

    #[test]
    fn rpc_correlation_ignores_stale_reply() {
        let request = Envelope {
            version: 1,
            cluster_id: CLUSTER.into(),
            from: 1,
            to: 2,
            rpc_id: 77,
            body: Rpc::AppendEntries {
                term: 4,
                leader_id: 1,
                prev_log_index: 8,
                prev_log_term: 4,
                entries: vec![],
                leader_commit: 8,
            },
        };
        let response = Envelope {
            version: 1,
            cluster_id: CLUSTER.into(),
            from: 2,
            to: 1,
            rpc_id: 77,
            body: Rpc::AppendResponse {
                term: 4,
                success: true,
                matched_through: 8,
                last_log_index: 8,
            },
        };
        assert!(correlated_response(&request, &response));
        let mut stale = response.clone();
        stale.rpc_id = 76;
        assert!(!correlated_response(&request, &stale));
        stale = response.clone();
        stale.cluster_id = "other".into();
        assert!(!correlated_response(&request, &stale));
        stale = response.clone();
        stale.from = 3;
        assert!(!correlated_response(&request, &stale));
        stale = response.clone();
        stale.body = Rpc::VoteResponse {
            term: 4,
            granted: true,
        };
        assert!(!correlated_response(&request, &stale));
    }
}
