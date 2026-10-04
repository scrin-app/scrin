//! Prints a signed `POST /v1/register` body for a fresh identity, for manual
//! testing with curl:
//!
//! ```text
//! cargo run -p scrin-server --example sign_register > body.json
//! curl.exe -k -H "content-type: application/json" --data @body.json https://127.0.0.1:4433/v1/register
//! ```

use scrin_crypto::identity::Identity;
use scrin_server::api::AddrHint;
use scrin_server::auth;

fn main() -> anyhow::Result<()> {
    let id = Identity::generate()?;
    let hint = AddrHint {
        relay_url: None,
        direct_addrs: vec!["127.0.0.1:41641".into()],
    };
    let ts = auth::now_secs();
    let sig = id.sign(&auth::canonical(
        auth::LABEL_REGISTER,
        &id.device_id(),
        ts,
        &hint.canonical(),
    ));
    let body = serde_json::json!({
        "device_pub": id.device_id().to_hex(),
        "addr_hint": hint,
        "timestamp": ts,
        "signature": data_encoding::HEXLOWER.encode(&sig),
    });
    println!("{body}");
    Ok(())
}
