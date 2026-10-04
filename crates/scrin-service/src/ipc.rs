//! Agent ↔ service messages on the local named pipe.
//!
//! Frame: `len u8` (1..=[`MAX_FRAME`]) then `tag u8` and payload. The pipe's
//! ACL admits only `LocalSystem`; on top of that the service acts on a
//! request only from the agent process it launched itself ([`authorize`]).

/// Pipe the service listens on.
pub const PIPE_NAME: &str = r"\\.\pipe\scrin-service";

/// Largest frame body (tag + payload). Every message is far smaller.
pub const MAX_FRAME: usize = 32;

const TAG_HELLO: u8 = 1;
const TAG_SAS: u8 = 2;
const TAG_SHUTDOWN: u8 = 3;
const TAG_ACK: u8 = 4;
const TAG_DENIED: u8 = 5;

/// Agent → service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// First message of a connection: the agent's own process id.
    Hello { pid: u32 },
    /// Send the secure attention sequence (Ctrl+Alt+Del) on behalf of a
    /// remote technician the device owner admitted.
    SendSas,
    /// The agent is exiting on purpose; do not restart it right away.
    Shutdown,
}

/// Service → agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    Ack,
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IpcError {
    #[error("empty or oversized frame")]
    FrameSize,
    #[error("unknown message")]
    Unknown,
    #[error("malformed message")]
    Malformed,
}

impl Request {
    #[must_use]
    pub fn encode(self) -> Vec<u8> {
        let body: Vec<u8> = match self {
            Self::Hello { pid } => {
                let mut b = vec![TAG_HELLO];
                b.extend_from_slice(&pid.to_le_bytes());
                b
            }
            Self::SendSas => vec![TAG_SAS],
            Self::Shutdown => vec![TAG_SHUTDOWN],
        };
        frame(&body)
    }

    pub fn decode(body: &[u8]) -> Result<Self, IpcError> {
        match body {
            [TAG_HELLO, a, b, c, d] => Ok(Self::Hello {
                pid: u32::from_le_bytes([*a, *b, *c, *d]),
            }),
            [TAG_SAS] => Ok(Self::SendSas),
            [TAG_SHUTDOWN] => Ok(Self::Shutdown),
            [TAG_HELLO | TAG_SAS | TAG_SHUTDOWN, ..] => Err(IpcError::Malformed),
            _ => Err(IpcError::Unknown),
        }
    }
}

impl Reply {
    #[must_use]
    pub fn encode(self) -> Vec<u8> {
        frame(&[match self {
            Self::Ack => TAG_ACK,
            Self::Denied => TAG_DENIED,
        }])
    }

    pub fn decode(body: &[u8]) -> Result<Self, IpcError> {
        match body {
            [TAG_ACK] => Ok(Self::Ack),
            [TAG_DENIED] => Ok(Self::Denied),
            _ => Err(IpcError::Unknown),
        }
    }
}

fn frame(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 1);
    // Bodies are at most 5 bytes.
    out.push(u8::try_from(body.len()).unwrap_or(u8::MAX));
    out.extend_from_slice(body);
    out
}

/// Validates a frame's length byte.
pub fn body_len(len_byte: u8) -> Result<usize, IpcError> {
    let n = usize::from(len_byte);
    if n == 0 || n > MAX_FRAME {
        return Err(IpcError::FrameSize);
    }
    Ok(n)
}

/// What the service does with a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Remember the connection as the agent's.
    Greet,
    SendSas,
    Shutdown,
    Deny,
}

/// `client_pid` is what the OS reports for the pipe client
/// (`GetNamedPipeClientProcessId`), never what the message says;
/// `greeted` is whether this connection already sent a matching `Hello`;
/// `agent_pid` is the process the service launched, if running.
#[must_use]
pub fn authorize(req: Request, client_pid: u32, greeted: bool, agent_pid: Option<u32>) -> Decision {
    let from_agent = agent_pid == Some(client_pid);
    match req {
        Request::Hello { pid } if from_agent && pid == client_pid => Decision::Greet,
        Request::SendSas if from_agent && greeted => Decision::SendSas,
        Request::Shutdown if from_agent && greeted => Decision::Shutdown,
        _ => Decision::Deny,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_and_replies_round_trip() {
        for r in [
            Request::Hello { pid: 0xDEAD_BEEF },
            Request::SendSas,
            Request::Shutdown,
        ] {
            let f = r.encode();
            let n = body_len(f[0]).expect("len");
            assert_eq!(Request::decode(&f[1..=n]), Ok(r));
        }
        for r in [Reply::Ack, Reply::Denied] {
            let f = r.encode();
            assert_eq!(Reply::decode(&f[1..]), Ok(r));
        }
    }

    #[test]
    fn rejects_bad_frames() {
        assert_eq!(body_len(0), Err(IpcError::FrameSize));
        assert_eq!(body_len(33), Err(IpcError::FrameSize));
        assert_eq!(Request::decode(&[]), Err(IpcError::Unknown));
        assert_eq!(Request::decode(&[99]), Err(IpcError::Unknown));
        assert_eq!(
            Request::decode(&[TAG_HELLO, 1, 2]),
            Err(IpcError::Malformed)
        );
        assert_eq!(Request::decode(&[TAG_SAS, 0]), Err(IpcError::Malformed));
    }

    #[test]
    fn only_the_launched_agent_after_hello_may_ask() {
        let agent = Some(42);
        assert_eq!(
            authorize(Request::Hello { pid: 42 }, 42, false, agent),
            Decision::Greet
        );
        // A process lying about its pid.
        assert_eq!(
            authorize(Request::Hello { pid: 42 }, 7, false, agent),
            Decision::Deny
        );
        assert_eq!(
            authorize(Request::Hello { pid: 7 }, 42, false, agent),
            Decision::Deny
        );
        // No hello yet.
        assert_eq!(
            authorize(Request::SendSas, 42, false, agent),
            Decision::Deny
        );
        assert_eq!(
            authorize(Request::SendSas, 42, true, agent),
            Decision::SendSas
        );
        assert_eq!(
            authorize(Request::Shutdown, 42, true, agent),
            Decision::Shutdown
        );
        // Another process, or no agent running.
        assert_eq!(authorize(Request::SendSas, 7, true, agent), Decision::Deny);
        assert_eq!(authorize(Request::SendSas, 42, true, None), Decision::Deny);
    }
}
