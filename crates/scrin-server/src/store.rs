//! Persistent rendezvous state behind the [`Store`] trait.
//!
//! [`MemoryStore`] is for tests and throwaway dev instances; [`SqliteStore`]
//! (`--db path`) persists registrations, presence and the abuse blocklist.
//! Methods are synchronous: every call is a single indexed lookup or insert on
//! a small table, well under a millisecond.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Mutex, PoisonError};

use rusqlite::{Connection, OptionalExtension, params};
use scrin_crypto::identity::DeviceId;

use crate::ids;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("random source unavailable")]
    Random,
    #[error("could not allocate a free id")]
    IdSpaceExhausted,
    #[error("corrupt row: {0}")]
    Corrupt(&'static str),
}

pub type StoreResult<T> = Result<T, StoreError>;

/// Result of [`Store::register`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Registration {
    pub id: u64,
    /// `false` when the key was already registered (idempotent re-register).
    pub created: bool,
}

/// An online device's last advertised address hint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Presence {
    pub key: DeviceId,
    /// Opaque JSON string signed by the device (see [`crate::api::AddrHint`]).
    pub addr_hint: String,
    pub expires_at: u64,
}

pub trait Store: Send + Sync + std::fmt::Debug {
    /// Binds a fresh random ID to `key`, or returns the existing one.
    fn register(&self, key: &DeviceId, now: u64) -> StoreResult<Registration>;
    fn id_for_key(&self, key: &DeviceId) -> StoreResult<Option<u64>>;
    fn key_for_id(&self, id: u64) -> StoreResult<Option<DeviceId>>;
    fn set_presence(&self, id: u64, addr_hint: &str, expires_at: u64) -> StoreResult<()>;
    /// The presence of `id` if it has not expired at `now`.
    fn presence(&self, id: u64, now: u64) -> StoreResult<Option<Presence>>;
    /// Records an abuse report; returns the number of distinct reporters of `subject`.
    fn add_abuse_report(
        &self,
        reporter: &DeviceId,
        subject: &DeviceId,
        reason: &str,
        now: u64,
    ) -> StoreResult<u64>;
    fn block(&self, key: &DeviceId, reason: &str, now: u64) -> StoreResult<()>;
    fn is_blocked(&self, key: &DeviceId) -> StoreResult<bool>;
    fn device_count(&self) -> StoreResult<u64>;
    /// Cheap liveness probe for `/ready`.
    fn ping(&self) -> StoreResult<()>;
}

/// Allocation retries before giving up. With 9e8 IDs this only fails when the
/// space is nearly full, which is the right time to stop and page someone.
const ALLOC_ATTEMPTS: usize = 32;

fn fresh_id(taken: impl Fn(u64) -> StoreResult<bool>) -> StoreResult<u64> {
    for _ in 0..ALLOC_ATTEMPTS {
        let id = ids::random_id().map_err(|_| StoreError::Random)?;
        if !taken(id)? {
            return Ok(id);
        }
    }
    Err(StoreError::IdSpaceExhausted)
}

// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct Mem {
    by_key: HashMap<DeviceId, u64>,
    by_id: HashMap<u64, DeviceId>,
    presence: HashMap<u64, (String, u64)>,
    reports: HashMap<DeviceId, HashSet<DeviceId>>,
    blocked: HashSet<DeviceId>,
}

#[derive(Debug, Default)]
pub struct MemoryStore(Mutex<Mem>);

impl MemoryStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Mem> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Store for MemoryStore {
    fn register(&self, key: &DeviceId, _now: u64) -> StoreResult<Registration> {
        let mut m = self.lock();
        if let Some(id) = m.by_key.get(key) {
            return Ok(Registration {
                id: *id,
                created: false,
            });
        }
        let id = fresh_id(|id| Ok(m.by_id.contains_key(&id)))?;
        m.by_key.insert(*key, id);
        m.by_id.insert(id, *key);
        Ok(Registration { id, created: true })
    }

    fn id_for_key(&self, key: &DeviceId) -> StoreResult<Option<u64>> {
        Ok(self.lock().by_key.get(key).copied())
    }

    fn key_for_id(&self, id: u64) -> StoreResult<Option<DeviceId>> {
        Ok(self.lock().by_id.get(&id).copied())
    }

    fn set_presence(&self, id: u64, addr_hint: &str, expires_at: u64) -> StoreResult<()> {
        self.lock()
            .presence
            .insert(id, (addr_hint.to_owned(), expires_at));
        Ok(())
    }

    fn presence(&self, id: u64, now: u64) -> StoreResult<Option<Presence>> {
        let m = self.lock();
        let Some((hint, exp)) = m.presence.get(&id) else {
            return Ok(None);
        };
        if *exp <= now {
            return Ok(None);
        }
        Ok(m.by_id.get(&id).map(|key| Presence {
            key: *key,
            addr_hint: hint.clone(),
            expires_at: *exp,
        }))
    }

    fn add_abuse_report(
        &self,
        reporter: &DeviceId,
        subject: &DeviceId,
        _reason: &str,
        _now: u64,
    ) -> StoreResult<u64> {
        let mut m = self.lock();
        let set = m.reports.entry(*subject).or_default();
        set.insert(*reporter);
        Ok(set.len() as u64)
    }

    fn block(&self, key: &DeviceId, _reason: &str, _now: u64) -> StoreResult<()> {
        self.lock().blocked.insert(*key);
        Ok(())
    }

    fn is_blocked(&self, key: &DeviceId) -> StoreResult<bool> {
        Ok(self.lock().blocked.contains(key))
    }

    fn device_count(&self) -> StoreResult<u64> {
        Ok(self.lock().by_id.len() as u64)
    }

    fn ping(&self) -> StoreResult<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------

/// Embedded migrations, applied in order; `PRAGMA user_version` = applied count.
const MIGRATIONS: &[&str] = &[
    // 1: initial schema
    "CREATE TABLE devices (
         id          INTEGER PRIMARY KEY,
         device_pub  BLOB NOT NULL UNIQUE CHECK (length(device_pub) = 32),
         created_at  INTEGER NOT NULL
     );
     CREATE TABLE presence (
         id          INTEGER PRIMARY KEY REFERENCES devices(id) ON DELETE CASCADE,
         addr_hint   TEXT NOT NULL,
         expires_at  INTEGER NOT NULL
     );
     CREATE TABLE abuse_reports (
         subject     BLOB NOT NULL,
         reporter    BLOB NOT NULL,
         reason      TEXT NOT NULL,
         created_at  INTEGER NOT NULL,
         PRIMARY KEY (subject, reporter)
     );
     CREATE TABLE blocklist (
         device_pub  BLOB PRIMARY KEY,
         reason      TEXT NOT NULL,
         created_at  INTEGER NOT NULL
     );",
];

#[derive(Debug)]
pub struct SqliteStore(Mutex<Connection>);

impl SqliteStore {
    pub fn open(path: &Path) -> StoreResult<Self> {
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> StoreResult<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> StoreResult<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        migrate(&mut conn)?;
        Ok(Self(Mutex::new(conn)))
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Applied migration count, for tests and `/ready` diagnostics.
    pub fn schema_version(&self) -> StoreResult<i64> {
        Ok(self
            .conn()
            .query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }
}

fn migrate(conn: &mut Connection) -> StoreResult<()> {
    let applied: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let applied = usize::try_from(applied).map_err(|_| StoreError::Corrupt("user_version"))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(applied) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        let version = i64::try_from(i + 1).map_err(|_| StoreError::Corrupt("user_version"))?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
    Ok(())
}

fn to_i64(id: u64) -> StoreResult<i64> {
    i64::try_from(id).map_err(|_| StoreError::Corrupt("id"))
}

fn key_from_blob(b: &[u8]) -> StoreResult<DeviceId> {
    let a: [u8; 32] = b
        .try_into()
        .map_err(|_| StoreError::Corrupt("device_pub"))?;
    Ok(DeviceId(a))
}

impl Store for SqliteStore {
    fn register(&self, key: &DeviceId, now: u64) -> StoreResult<Registration> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let existing: Option<i64> = tx
            .query_row(
                "SELECT id FROM devices WHERE device_pub = ?1",
                params![&key.0[..]],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = existing {
            let id = u64::try_from(id).map_err(|_| StoreError::Corrupt("id"))?;
            return Ok(Registration { id, created: false });
        }
        let id = fresh_id(|id| {
            Ok(tx
                .query_row(
                    "SELECT 1 FROM devices WHERE id = ?1",
                    params![to_i64(id)?],
                    |_| Ok(()),
                )
                .optional()?
                .is_some())
        })?;
        tx.execute(
            "INSERT INTO devices (id, device_pub, created_at) VALUES (?1, ?2, ?3)",
            params![to_i64(id)?, &key.0[..], to_i64(now)?],
        )?;
        tx.commit()?;
        Ok(Registration { id, created: true })
    }

    fn id_for_key(&self, key: &DeviceId) -> StoreResult<Option<u64>> {
        let id: Option<i64> = self
            .conn()
            .query_row(
                "SELECT id FROM devices WHERE device_pub = ?1",
                params![&key.0[..]],
                |r| r.get(0),
            )
            .optional()?;
        id.map(|i| u64::try_from(i).map_err(|_| StoreError::Corrupt("id")))
            .transpose()
    }

    fn key_for_id(&self, id: u64) -> StoreResult<Option<DeviceId>> {
        let blob: Option<Vec<u8>> = self
            .conn()
            .query_row(
                "SELECT device_pub FROM devices WHERE id = ?1",
                params![to_i64(id)?],
                |r| r.get(0),
            )
            .optional()?;
        blob.map(|b| key_from_blob(&b)).transpose()
    }

    fn set_presence(&self, id: u64, addr_hint: &str, expires_at: u64) -> StoreResult<()> {
        self.conn().execute(
            "INSERT INTO presence (id, addr_hint, expires_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET addr_hint = excluded.addr_hint,
                                           expires_at = excluded.expires_at",
            params![to_i64(id)?, addr_hint, to_i64(expires_at)?],
        )?;
        Ok(())
    }

    fn presence(&self, id: u64, now: u64) -> StoreResult<Option<Presence>> {
        let row: Option<(Vec<u8>, String, i64)> = self
            .conn()
            .query_row(
                "SELECT d.device_pub, p.addr_hint, p.expires_at
                   FROM presence p JOIN devices d ON d.id = p.id
                  WHERE p.id = ?1 AND p.expires_at > ?2",
                params![to_i64(id)?, to_i64(now)?],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        row.map(|(k, addr_hint, exp)| {
            Ok(Presence {
                key: key_from_blob(&k)?,
                addr_hint,
                expires_at: u64::try_from(exp).map_err(|_| StoreError::Corrupt("expires_at"))?,
            })
        })
        .transpose()
    }

    fn add_abuse_report(
        &self,
        reporter: &DeviceId,
        subject: &DeviceId,
        reason: &str,
        now: u64,
    ) -> StoreResult<u64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO abuse_reports (subject, reporter, reason, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(subject, reporter) DO UPDATE SET reason = excluded.reason,
                                                         created_at = excluded.created_at",
            params![&subject.0[..], &reporter.0[..], reason, to_i64(now)?],
        )?;
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM abuse_reports WHERE subject = ?1",
            params![&subject.0[..]],
            |r| r.get(0),
        )?;
        u64::try_from(n).map_err(|_| StoreError::Corrupt("count"))
    }

    fn block(&self, key: &DeviceId, reason: &str, now: u64) -> StoreResult<()> {
        self.conn().execute(
            "INSERT OR IGNORE INTO blocklist (device_pub, reason, created_at) VALUES (?1, ?2, ?3)",
            params![&key.0[..], reason, to_i64(now)?],
        )?;
        Ok(())
    }

    fn is_blocked(&self, key: &DeviceId) -> StoreResult<bool> {
        Ok(self
            .conn()
            .query_row(
                "SELECT 1 FROM blocklist WHERE device_pub = ?1",
                params![&key.0[..]],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    fn device_count(&self) -> StoreResult<u64> {
        let n: i64 = self
            .conn()
            .query_row("SELECT COUNT(*) FROM devices", [], |r| r.get(0))?;
        u64::try_from(n).map_err(|_| StoreError::Corrupt("count"))
    }

    fn ping(&self) -> StoreResult<()> {
        self.conn().query_row("SELECT 1", [], |_| Ok(()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(b: u8) -> DeviceId {
        DeviceId([b; 32])
    }

    fn exercise(store: &dyn Store) {
        let a = store.register(&key(1), 10).expect("register");
        assert!(a.created);
        assert!(ids::is_valid_id(a.id));
        let again = store.register(&key(1), 20).expect("register");
        assert_eq!(
            again,
            Registration {
                id: a.id,
                created: false
            }
        );
        let b = store.register(&key(2), 10).expect("register");
        assert_ne!(a.id, b.id);
        assert_eq!(store.id_for_key(&key(1)).expect("q"), Some(a.id));
        assert_eq!(store.key_for_id(b.id).expect("q"), Some(key(2)));
        assert_eq!(store.key_for_id(1).expect("q"), None);
        assert_eq!(store.device_count().expect("q"), 2);

        store.set_presence(a.id, "{}", 100).expect("presence");
        assert_eq!(
            store.presence(a.id, 99).expect("q").map(|p| p.key),
            Some(key(1))
        );
        assert_eq!(store.presence(a.id, 100).expect("q"), None);
        store
            .set_presence(a.id, "{\"x\":1}", 200)
            .expect("presence");
        assert_eq!(
            store.presence(a.id, 150).expect("q").map(|p| p.addr_hint),
            Some("{\"x\":1}".to_owned())
        );

        assert_eq!(
            store.add_abuse_report(&key(1), &key(9), "r", 1).expect("q"),
            1
        );
        assert_eq!(
            store
                .add_abuse_report(&key(1), &key(9), "r2", 2)
                .expect("q"),
            1
        );
        assert_eq!(
            store.add_abuse_report(&key(2), &key(9), "r", 3).expect("q"),
            2
        );
        assert!(!store.is_blocked(&key(9)).expect("q"));
        store.block(&key(9), "abuse", 4).expect("block");
        store
            .block(&key(9), "abuse", 5)
            .expect("block twice is fine");
        assert!(store.is_blocked(&key(9)).expect("q"));
        store.ping().expect("ping");
    }

    #[test]
    fn memory_store_contract() {
        exercise(&MemoryStore::new());
    }

    #[test]
    fn sqlite_store_contract() {
        exercise(&SqliteStore::open_in_memory().expect("open"));
    }

    #[test]
    fn sqlite_persists_and_migrations_are_idempotent() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("scrin.db");
        let id = {
            let s = SqliteStore::open(&path).expect("open");
            assert_eq!(s.schema_version().expect("v"), 1);
            s.register(&key(5), 1).expect("register").id
        };
        let s = SqliteStore::open(&path).expect("reopen");
        assert_eq!(s.schema_version().expect("v"), 1);
        assert_eq!(s.id_for_key(&key(5)).expect("q"), Some(id));
    }
}
