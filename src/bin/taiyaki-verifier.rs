//! Independently provisioned verifier: no SQLite access and no arbitrary MCP commands.
use std::{env, io, path::PathBuf, time::Duration};
fn required(name: &str) -> io::Result<PathBuf> {
    env::var_os(name)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("missing {name}")))
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let once = std::env::args().skip(1).collect::<Vec<_>>();
    if !once.is_empty() && once.as_slice() != ["--once"] {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "only --once is supported").into());
    }
    let socket = required("TAIYAKI_SOCKET_PATH")?;
    let secret = required("TAIYAKI_SECRET_FILE")?;
    let config = required("TAIYAKI_TEST_CATALOG")?;
    let client = taiyaki::authority::Client::open(socket, &secret)?;
    let catalog = taiyaki::verifier::read_catalog(&config)?;
    loop {
        match taiyaki::verifier::verify_pending(&client, &catalog) {
            Ok(count) => {
                if count > 0 {
                    eprintln!("Taiyaki verifier completed {count} work item(s)")
                }
            }
            Err(error) => {
                eprintln!("Taiyaki verifier: {error}");
                if !once.is_empty() {
                    return Err(error.into());
                }
            }
        }
        if !once.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    Ok(())
}
