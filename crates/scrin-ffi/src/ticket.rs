//! Connect tickets: the host's device id plus how to reach it, as one copyable string.
//!
//! `scrin1` + lowercase base32 of
//! `ver u8 = 1 || id [32] || n u8 || n × (fam u8 (4|6) || ip || port u16 BE) || relay_len u16 BE || relay utf8`.
//! A bare 64-hex device id is accepted too (dialled through address lookup / relays).
//! Tickets are not secret: they carry a public key and addresses, never the code.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use data_encoding::{BASE32_NOPAD, HEXLOWER_PERMISSIVE};
use iroh::{EndpointAddr, EndpointId, RelayUrl, TransportAddr};

use crate::types::ScrinError;

const PREFIX: &str = "scrin1";
const VERSION: u8 = 1;
const MAX_ADDRS: usize = 16;

/// Encodes `addr` plus any extra bound sockets (unspecified ones are skipped).
pub(crate) fn encode(addr: &EndpointAddr, extra: &[SocketAddr]) -> String {
    let mut ips: Vec<SocketAddr> = addr
        .ip_addrs()
        .copied()
        .chain(extra.iter().copied())
        .filter(|s| !s.ip().is_unspecified())
        .collect();
    ips.sort_unstable();
    ips.dedup();
    ips.truncate(MAX_ADDRS);

    let mut b = Vec::with_capacity(64 + ips.len() * 19);
    b.push(VERSION);
    b.extend_from_slice(addr.id.as_bytes());
    // `ips.len() <= MAX_ADDRS` (16).
    b.push(u8::try_from(ips.len()).unwrap_or(u8::MAX));
    for s in &ips {
        match s.ip() {
            IpAddr::V4(v) => {
                b.push(4);
                b.extend_from_slice(&v.octets());
            }
            IpAddr::V6(v) => {
                b.push(6);
                b.extend_from_slice(&v.octets());
            }
        }
        b.extend_from_slice(&s.port().to_be_bytes());
    }
    let relay = addr
        .relay_urls()
        .next()
        .map(ToString::to_string)
        .unwrap_or_default();
    let relay = if relay.len() > usize::from(u16::MAX) {
        String::new()
    } else {
        relay
    };
    b.extend_from_slice(&u16::try_from(relay.len()).unwrap_or(0).to_be_bytes());
    b.extend_from_slice(relay.as_bytes());
    format!("{PREFIX}{}", BASE32_NOPAD.encode(&b).to_lowercase())
}

/// Parses a ticket or a bare hex device id.
pub(crate) fn decode(input: &str) -> Result<EndpointAddr, ScrinError> {
    let s: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    if s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit()) {
        let bytes = HEXLOWER_PERMISSIVE
            .decode(s.as_bytes())
            .map_err(|_| ScrinError::input("device id"))?;
        return Ok(EndpointAddr::new(endpoint_id(&bytes)?));
    }
    let body = s
        .strip_prefix(PREFIX)
        .ok_or_else(|| ScrinError::input("not a scrin ticket"))?;
    let raw = BASE32_NOPAD
        .decode(body.to_ascii_uppercase().as_bytes())
        .map_err(|_| ScrinError::input("ticket encoding"))?;
    let mut r = Reader(&raw);
    if r.u8()? != VERSION {
        return Err(ScrinError::input("unsupported ticket version"));
    }
    let id = endpoint_id(r.take(32)?)?;
    let n = usize::from(r.u8()?);
    if n > MAX_ADDRS {
        return Err(ScrinError::input("too many addresses"));
    }
    let mut addrs = Vec::with_capacity(n + 1);
    for _ in 0..n {
        let ip = match r.u8()? {
            4 => {
                let o: [u8; 4] = r.array()?;
                IpAddr::V4(Ipv4Addr::from(o))
            }
            6 => {
                let o: [u8; 16] = r.array()?;
                IpAddr::V6(Ipv6Addr::from(o))
            }
            _ => return Err(ScrinError::input("address family")),
        };
        let port = u16::from_be_bytes(r.array()?);
        addrs.push(TransportAddr::Ip(SocketAddr::new(ip, port)));
    }
    let relay_len = usize::from(u16::from_be_bytes(r.array()?));
    if relay_len > 0 {
        let url =
            std::str::from_utf8(r.take(relay_len)?).map_err(|_| ScrinError::input("relay url"))?;
        let url: RelayUrl = url.parse().map_err(|_| ScrinError::input("relay url"))?;
        addrs.push(TransportAddr::Relay(url));
    }
    if !r.0.is_empty() {
        return Err(ScrinError::input("trailing ticket bytes"));
    }
    Ok(EndpointAddr::from_parts(id, addrs))
}

fn endpoint_id(bytes: &[u8]) -> Result<EndpointId, ScrinError> {
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| ScrinError::input("device id length"))?;
    EndpointId::from_bytes(&arr).map_err(|_| ScrinError::input("device id is not a valid key"))
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], ScrinError> {
        if self.0.len() < n {
            return Err(ScrinError::input("ticket truncated"));
        }
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        Ok(head)
    }

    fn u8(&mut self) -> Result<u8, ScrinError> {
        Ok(self.take(1)?[0])
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], ScrinError> {
        self.take(N)?
            .try_into()
            .map_err(|_| ScrinError::input("ticket truncated"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scrin_crypto::identity::Identity;

    fn id() -> EndpointId {
        EndpointId::from_bytes(&Identity::from_seed([7; 32]).device_id().0).expect("valid key")
    }

    #[test]
    fn ticket_round_trips_ips_and_relay() {
        let relay: RelayUrl = "https://relay.example.org/".parse().expect("url");
        let addr = EndpointAddr::from_parts(
            id(),
            [
                TransportAddr::Ip("127.0.0.1:4242".parse().expect("sock")),
                TransportAddr::Ip("[::1]:5353".parse().expect("sock")),
                TransportAddr::Relay(relay.clone()),
            ],
        );
        let t = encode(&addr, &["0.0.0.0:9".parse().expect("sock")]);
        assert!(t.starts_with("scrin1"));
        let back = decode(&t).expect("decode");
        assert_eq!(back.id, addr.id);
        assert_eq!(back.ip_addrs().count(), 2);
        assert_eq!(back.relay_urls().next(), Some(&relay));
    }

    #[test]
    fn bare_hex_id_is_accepted_and_garbage_is_not() {
        let hex = Identity::from_seed([7; 32]).device_id().to_hex();
        assert_eq!(decode(&hex.to_uppercase()).expect("hex").id, id());
        assert!(decode("scrin1zzzz").is_err());
        assert!(decode("hello").is_err());
        let mut t = encode(&EndpointAddr::new(id()), &[]);
        t.push('a');
        assert!(decode(&t).is_err());
    }
}
