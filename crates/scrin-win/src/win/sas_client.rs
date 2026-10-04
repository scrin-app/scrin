//! Asks the scrin service for the secure attention sequence over its pipe
//! (`crates/scrin-service/src/ipc.rs`). Only succeeds for the agent process
//! the service launched itself; anywhere else it reports `Unavailable`.

use std::fs::OpenOptions;
use std::io::{Read, Write};

const PIPE_NAME: &str = r"\\.\pipe\scrin-service";
const TAG_HELLO: u8 = 1;
const TAG_SAS: u8 = 2;
const TAG_ACK: u8 = 4;

#[derive(Debug, thiserror::Error)]
pub enum SasError {
    /// No service, or this process was not launched by it.
    #[error("the scrin service is not available to this process")]
    Unavailable,
    #[error("the scrin service refused")]
    Denied,
}

/// Sends `Hello{pid}` then `SendSas`; one short-lived connection.
pub fn request_sas() -> Result<(), SasError> {
    let mut pipe = OpenOptions::new()
        .read(true)
        .write(true)
        .open(PIPE_NAME)
        .map_err(|_| SasError::Unavailable)?;
    let mut hello = vec![5, TAG_HELLO];
    hello.extend_from_slice(&std::process::id().to_le_bytes());
    exchange(&mut pipe, &hello)?;
    exchange(&mut pipe, &[1, TAG_SAS])
}

fn exchange(pipe: &mut std::fs::File, frame: &[u8]) -> Result<(), SasError> {
    pipe.write_all(frame).map_err(|_| SasError::Unavailable)?;
    let mut reply = [0u8; 2];
    pipe.read_exact(&mut reply)
        .map_err(|_| SasError::Unavailable)?;
    if reply == [1, TAG_ACK] {
        Ok(())
    } else {
        Err(SasError::Denied)
    }
}
