//! SQLite authority: start manually under a distinct, dedicated Unix UID.
use std::{env, io, path::PathBuf};
fn required(name: &str) -> io::Result<PathBuf> {
    env::var_os(name)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("missing {name}")))
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    unsafe {
        libc::umask(0o077);
    }
    let socket = required("TAIYAKI_SOCKET_PATH")?;
    let database = required("TAIYAKI_DB_FILE")?;
    let key = required("TAIYAKI_SECRET_FILE")?;
    let client_uid = env::var("TAIYAKI_CLIENT_UID")
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "missing TAIYAKI_CLIENT_UID"))?
        .parse::<u32>()?;
    if client_uid == unsafe { libc::geteuid() } {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "authority/client must have distinct UIDs",
        )
        .into());
    }
    let secret = taiyaki::authority::load_secret(&key)?;
    let db = taiyaki::db::Database::open(&database)?;
    taiyaki::authority::run(socket, db, client_uid, secret).await?;
    Ok(())
}
