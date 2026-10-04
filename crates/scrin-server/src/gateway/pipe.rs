//! Byte pipes between stream halves of different QUIC stacks (wtransport
//! for the browser, noq via iroh for the host), propagating FIN, `RESET_STREAM`
//! and `STOP_SENDING` with their codes.

use std::future::Future;
use std::sync::Arc;

use iroh::endpoint::{ReadError, RecvStream as HostRecv, SendStream as HostSend, WriteError};

use super::code;
use super::iroh_varint;
use super::quota::Quota;

pub const PIPE_BUF: usize = 16 * 1024;

/// A receive half. `Err(Some(code))` = reset by the peer with `code`.
pub trait Rx: Send + 'static {
    fn rx_read(
        &mut self,
        buf: &mut [u8],
    ) -> impl Future<Output = Result<Option<usize>, Option<u64>>> + Send;
    fn rx_stop(self, code: u64);
}

/// A send half. `Err(Some(code))` = the peer sent `STOP_SENDING` with `code`.
pub trait Tx: Send + 'static {
    fn tx_write(&mut self, buf: &[u8]) -> impl Future<Output = Result<(), Option<u64>>> + Send;
    fn tx_finish(self) -> impl Future<Output = ()> + Send;
    fn tx_reset(self, code: u64);
}

impl Rx for HostRecv {
    async fn rx_read(&mut self, buf: &mut [u8]) -> Result<Option<usize>, Option<u64>> {
        self.read(buf).await.map_err(|e| match e {
            ReadError::Reset(c) => Some(c.into_inner()),
            _ => None,
        })
    }

    fn rx_stop(mut self, code: u64) {
        let _ = self.stop(iroh_varint(code));
    }
}

impl Tx for HostSend {
    async fn tx_write(&mut self, buf: &[u8]) -> Result<(), Option<u64>> {
        self.write_all(buf).await.map_err(|e| match e {
            WriteError::Stopped(c) => Some(c.into_inner()),
            _ => None,
        })
    }

    async fn tx_finish(mut self) {
        let _ = self.finish();
    }

    fn tx_reset(mut self, code: u64) {
        let _ = self.reset(iroh_varint(code));
    }
}

fn wt_varint(v: u64) -> wtransport::VarInt {
    wtransport::VarInt::try_from_u64(v).unwrap_or(wtransport::VarInt::from_u32(0))
}

impl Rx for wtransport::RecvStream {
    async fn rx_read(&mut self, buf: &mut [u8]) -> Result<Option<usize>, Option<u64>> {
        self.read(buf).await.map_err(|e| match e {
            wtransport::error::StreamReadError::Reset(c) => Some(c.into_inner()),
            _ => None,
        })
    }

    fn rx_stop(self, code: u64) {
        self.stop(wt_varint(code));
    }
}

impl Tx for wtransport::SendStream {
    async fn tx_write(&mut self, buf: &[u8]) -> Result<(), Option<u64>> {
        self.write_all(buf).await.map_err(|e| match e {
            wtransport::error::StreamWriteError::Stopped(c) => Some(c.into_inner()),
            _ => None,
        })
    }

    async fn tx_finish(mut self) {
        let _ = self.finish().await;
    }

    fn tx_reset(mut self, code: u64) {
        let _ = self.reset(wt_varint(code));
    }
}

/// Copies `r` into `w` until FIN, reset or stop, charging `quota` and calling
/// `count` with every forwarded chunk size.
pub async fn pipe<R: Rx, W: Tx>(mut r: R, mut w: W, quota: Arc<Quota>, count: impl Fn(usize)) {
    let mut buf = vec![0u8; PIPE_BUF];
    loop {
        match r.rx_read(&mut buf).await {
            Ok(Some(n)) => {
                if quota.spend(n).await.is_err() {
                    w.tx_reset(u64::from(code::QUOTA));
                    r.rx_stop(u64::from(code::QUOTA));
                    return;
                }
                count(n);
                if let Err(c) = w.tx_write(&buf[..n]).await {
                    r.rx_stop(c.unwrap_or(0));
                    return;
                }
            }
            Ok(None) => {
                w.tx_finish().await;
                return;
            }
            Err(c) => {
                w.tx_reset(c.unwrap_or(0));
                return;
            }
        }
    }
}
