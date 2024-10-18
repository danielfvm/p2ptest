use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::{path::Path, sync::Arc};

use tokio_rusqlite::{params, Connection, Result};

pub type FileHash = [u8; 32];

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct File {
    pub name: String,
    pub data: Vec<u8>,
}

impl File {
    pub fn hash(&self) -> FileHash {
        let mut hasher = sha2::Sha512_256::new();
        hasher.update(&self.data);
        hasher.update(&self.name);
        hasher.finalize().as_slice().try_into().unwrap()
    }
}

#[derive(Debug)]
pub struct Store {
    conn: Connection,
}

impl Store {
    pub async fn memory() -> Result<Self> {
        let mut conn = Connection::open_in_memory().await?;
        Self::create_tables(&mut conn).await?;

        Ok(Self { conn })
    }

    pub async fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut conn = Connection::open(path).await?;
        Self::create_tables(&mut conn).await?;

        Ok(Self { conn })
    }

    pub async fn insert(&mut self, file: Arc<File>) -> Result<()> {
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO files (hash, name, data) VALUES (?1, ?2, ?3)",
                    params![file.hash(), file.name, file.data],
                )?;

                Ok(())
            })
            .await?;

        Ok(())
    }

    pub async fn get(&mut self, hash: FileHash) -> Result<File> {
        let result = self
            .conn
            .call(move |conn| {
                let mut stmt =
                    conn.prepare("SELECT DISTINCT name, data FROM files WHERE hash = ?1")?;
                let mut files = stmt.query_map(params![hash], |row| {
                    Ok(File {
                        name: row.get(0)?,
                        data: row.get(1)?,
                    })
                })?;

                match files.next() {
                    Some(Ok(file)) => Ok(file),
                    _ => Err(tokio_rusqlite::Error::Rusqlite(
                        rusqlite::Error::QueryReturnedNoRows,
                    )),
                }
            })
            .await?;

        Ok(result)
    }

    async fn create_tables(conn: &mut Connection) -> Result<()> {
        conn.call(|conn| {
            conn.execute(
                "CREATE TABLE IF NOT EXISTS files (
                    hash CHAR(32) NOT NULL PRIMARY KEY,
                    name TEXT NOT NULL,
                    data BLOB NOT NULL,
                    timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
                    UNIQUE(hash)
                )",
                [],
            )?;

            Ok(())
        })
        .await
    }
}
