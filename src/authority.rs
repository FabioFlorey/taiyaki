//! Unix-socket authority protocol: kernel UID check AND fresh HMAC challenge.
//! The socket does not grant raw SQL or arbitrary tool execution.

use crate::{
    api::{make_edit, EditInput, NewPlan, ReviewAction, ReviewInput},
    db::{Database, DbError},
    planning::{Plan, PlanChange, PlanError},
    workflow::{GateObservation, TestGate},
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{
    fs,
    io::{self, BufRead, BufReader, Write},
    os::unix::{
        fs::{MetadataExt, PermissionsExt},
        net::UnixStream as StdUnixStream,
    },
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
};
type HmacSha256 = Hmac<Sha256>;
const FRAME_MAX: usize = 1_100_000;
const TIMEOUT: Duration = Duration::from_secs(8);
const DOMAIN: &[u8] = b"taiyaki-authority-v1";

#[derive(Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Get,
    Create {
        args: NewPlan,
    },
    Edit {
        args: Box<EditInput>,
    },
    Review {
        args: ReviewInput,
    },
    WorkStatus,
    WorkNext {
        expected_revision: u64,
        agent: String,
    },
    WorkReport {
        expected_revision: u64,
        item_id: uuid::Uuid,
        lease: uuid::Uuid,
        notes: String,
    },
    TestDefine {
        expected_revision: u64,
        gate: TestGate,
    },
    VerifyFetch,
    VerifyRefresh {
        expected_revision: u64,
        source_tree_sha256: Option<String>,
    },
    VerifyCommit {
        expected_revision: u64,
        item_id: uuid::Uuid,
        source_tree_sha256: String,
        results: Vec<GateObservation>,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Response {
    Ok { plan: Plan },
    Data { data: serde_json::Value },
    Error { code: String, message: String },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Challenge {
    version: u8,
    nonce: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Proof {
    mac: String,
}

fn err(code: &str, message: impl Into<String>) -> Response {
    Response::Error {
        code: code.into(),
        message: message.into(),
    }
}
fn db_error(error: DbError) -> Response {
    let code = match &error {
        DbError::NotFound => "not_found",
        DbError::AlreadyExists => "already_exists",
        DbError::Plan(PlanError::RevisionConflict { .. }) => "revision_conflict",
        DbError::Plan(PlanError::ReviewLocked) => "review_locked",
        DbError::Plan(PlanError::NotReady(_)) => "not_ready",
        DbError::Plan(_) => "invalid_plan",
        DbError::Sql(_) => "storage_error",
        DbError::Invalid(_) => "unsafe_storage",
        DbError::Workflow(_) => "workflow_rejected",
    };
    err(code, error.to_string())
}

fn execute(db: &Database, request: Request, verifier_role: bool) -> Response {
    use serde_json::json;
    if verifier_role {
        match request {
            Request::VerifyFetch => {
                return match (db.get(), db.workflow()) {
                    (Ok(plan), Ok(mut flow)) => {
                        for work in flow.work.values_mut() {
                            work.lease = None;
                        }
                        Response::Data {
                            data: json!({"plan":plan,"workflow":flow}),
                        }
                    }
                    (Err(e), _) | (_, Err(e)) => db_error(e),
                }
            }
            Request::VerifyRefresh {
                expected_revision,
                source_tree_sha256,
            } => {
                return match db.refresh_verifier_source(expected_revision, source_tree_sha256) {
                    Ok(mut flow) => {
                        for rec in flow.work.values_mut() {
                            rec.lease = None;
                        }
                        Response::Data { data: json!(flow) }
                    }
                    Err(e) => db_error(e),
                }
            }
            Request::VerifyCommit {
                expected_revision,
                item_id,
                source_tree_sha256,
                results,
            } => {
                return match db.commit_verification(
                    expected_revision,
                    item_id,
                    source_tree_sha256,
                    results,
                ) {
                    Ok(mut flow) => {
                        for work in flow.work.values_mut() {
                            work.lease = None;
                        }
                        Response::Data { data: json!(flow) }
                    }
                    Err(e) => db_error(e),
                }
            }
            _ => {
                return err(
                    "forbidden",
                    "verifier identity can only report verification",
                )
            }
        }
    }
    match request {
        Request::VerifyFetch | Request::VerifyRefresh { .. } | Request::VerifyCommit { .. } => err(
            "forbidden",
            "only verifier identity can submit test evidence",
        ),
        Request::Get => match db.get() {
            Ok(plan) => Response::Ok { plan },
            Err(e) => db_error(e),
        },
        Request::Create { args } => match db.create(args.title, args.description) {
            Ok(plan) => Response::Ok { plan },
            Err(e) => db_error(e),
        },
        Request::Edit { args } => {
            match db.apply(args.expected_revision, |plan| make_edit(plan, args.edit)) {
                Ok(plan) => Response::Ok { plan },
                Err(e) => db_error(e),
            }
        }
        Request::Review { args } => {
            let change = match args.action {
                ReviewAction::Submit => PlanChange::SubmitForReview,
                ReviewAction::ReturnToDraft => PlanChange::ReturnToDraft,
            };
            match db.apply(args.expected_revision, |_| Ok(change)) {
                Ok(plan) => Response::Ok { plan },
                Err(e) => db_error(e),
            }
        }
        Request::WorkStatus => match db.workflow() {
            Ok(mut flow) => {
                for record in flow.work.values_mut() {
                    record.lease = None;
                }
                Response::Data { data: json!(flow) }
            }
            Err(e) => db_error(e),
        },
        Request::WorkNext {
            expected_revision,
            agent,
        } => match db.claim_next(expected_revision, &agent) {
            Ok((mut flow, assignment)) => {
                for record in flow.work.values_mut() {
                    record.lease = None;
                }
                Response::Data {
                    data: json!({"workflow":flow,"assignment":assignment}),
                }
            }
            Err(e) => db_error(e),
        },
        Request::WorkReport {
            expected_revision,
            item_id,
            lease,
            notes,
        } => match db.submit_work(expected_revision, item_id, lease, notes) {
            Ok(mut flow) => {
                for record in flow.work.values_mut() {
                    record.lease = None;
                }
                Response::Data { data: json!(flow) }
            }
            Err(e) => db_error(e),
        },
        Request::TestDefine {
            expected_revision,
            gate,
        } => match db.define_gate(expected_revision, gate) {
            Ok(mut flow) => {
                for record in flow.work.values_mut() {
                    record.lease = None;
                }
                Response::Data { data: json!(flow) }
            }
            Err(e) => db_error(e),
        },
    }
}

fn key_file(path: &Path) -> io::Result<[u8; 32]> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "key path must be absolute",
        ));
    }
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file()
        || meta.nlink() != 1
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "key must be private regular file owned by current process",
        ));
    }
    let input = fs::read_to_string(path)?;
    let bytes = hex::decode(input.trim())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "key must be 64 hex characters"))?;
    <[u8; 32]>::try_from(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "key must be 32 bytes"))
}
fn signature(key: &[u8; 32], nonce: &[u8]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts 256-bit key");
    mac.update(DOMAIN);
    mac.update(nonce);
    mac.finalize().into_bytes().into()
}

async fn read_frame(socket: &mut UnixStream) -> io::Result<Vec<u8>> {
    // Byte-budgeted before allocation: a sender cannot force unbounded buffering.
    let mut message = Vec::new();
    for _ in 0..FRAME_MAX {
        let b = socket.read_u8().await?;
        if b == b'\n' {
            return Ok(message);
        }
        message.push(b);
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "frame too large",
    ))
}
async fn send_frame(socket: &mut UnixStream, value: &impl Serialize) -> io::Result<()> {
    let mut message = serde_json::to_vec(value).map_err(io::Error::other)?;
    message.push(b'\n');
    socket.write_all(&message).await
}
async fn serve_connection(
    mut socket: UnixStream,
    secret: [u8; 32],
    db: Database,
    verifier_role: bool,
) -> io::Result<()> {
    let nonce: [u8; 32] = rand::random();
    send_frame(
        &mut socket,
        &Challenge {
            version: 1,
            nonce: hex::encode(nonce),
        },
    )
    .await?;
    let auth = read_frame(&mut socket).await?;
    let proof: Proof = match serde_json::from_slice(&auth) {
        Ok(p) => p,
        Err(_) => {
            send_frame(&mut socket, &err("unauthorized", "invalid proof")).await?;
            return Ok(());
        }
    };
    let received = hex::decode(proof.mac).unwrap_or_default();
    let mut verifier = HmacSha256::new_from_slice(&secret).expect("HMAC key");
    verifier.update(DOMAIN);
    verifier.update(&nonce);
    if verifier.verify_slice(&received).is_err() {
        send_frame(
            &mut socket,
            &err("unauthorized", "challenge verification failed"),
        )
        .await?;
        return Ok(());
    }
    // Handshake completes before any request is read or any state touched.
    let frame = read_frame(&mut socket).await?;
    let request: Request = match serde_json::from_slice(&frame) {
        Ok(r) => r,
        Err(_) => {
            send_frame(
                &mut socket,
                &err("invalid_request", "unrecognized or malformed operation"),
            )
            .await?;
            return Ok(());
        }
    };
    let response = tokio::task::spawn_blocking(move || execute(&db, request, verifier_role))
        .await
        .map_err(io::Error::other)?;
    send_frame(&mut socket, &response).await
}

fn check_socket_directory(path: &Path) -> io::Result<()> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "socket path must be absolute",
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "socket missing parent"))?;
    let meta = fs::metadata(parent)?;
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o022 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "socket directory must be owned by authority and not group/world writable",
        ));
    }
    if fs::symlink_metadata(path).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "socket path already exists (refusing to unlink)",
        ));
    }
    Ok(())
}
pub async fn run(
    socket_path: PathBuf,
    db: Database,
    allowed_uid: u32,
    secret: [u8; 32],
) -> io::Result<()> {
    run_with_verifier(socket_path, db, allowed_uid, secret, None).await
}

pub async fn run_with_verifier(
    socket_path: PathBuf,
    db: Database,
    allowed_uid: u32,
    secret: [u8; 32],
    independent_verifier: Option<(u32, [u8; 32])>,
) -> io::Result<()> {
    if independent_verifier
        .as_ref()
        .is_some_and(|(uid, _)| *uid == allowed_uid || *uid == unsafe { libc::geteuid() })
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "verifier, client and authority UIDs must be different",
        ));
    }
    if allowed_uid == unsafe { libc::geteuid() } {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "authority UID and client UID must be different",
        ));
    }
    check_socket_directory(&socket_path)?;
    let listener = UnixListener::bind(&socket_path)?;
    // World-connectable socket; SO_PEERCRED + HMAC are mandatory. Directory is
    // not writable by untrusted users, so the socket cannot be replaced.
    fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o666))?;
    loop {
        let (mut conn, _) = listener.accept().await?;
        let Ok(credentials) = conn.peer_cred() else {
            continue;
        };
        let identity = if credentials.uid() == allowed_uid {
            Some((secret, false))
        } else if independent_verifier
            .as_ref()
            .is_some_and(|(uid, _)| *uid == credentials.uid())
        {
            independent_verifier.map(|(_, key)| (key, true))
        } else {
            None
        };
        let Some((key, verifier_role)) = identity else {
            let _ = send_frame(&mut conn, &err("unauthorized", "peer identity denied")).await;
            continue;
        };
        let db = db.clone();
        tokio::spawn(async move {
            let _ =
                tokio::time::timeout(TIMEOUT, serve_connection(conn, key, db, verifier_role)).await;
        });
    }
}

#[derive(Clone)]
pub struct Client {
    socket: PathBuf,
    key: [u8; 32],
}
impl Client {
    pub fn open(socket: PathBuf, secret_file: &Path) -> io::Result<Self> {
        if !socket.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "socket path must be absolute",
            ));
        }
        Ok(Self {
            socket,
            key: key_file(secret_file)?,
        })
    }
    pub fn request(&self, request: Request) -> Result<serde_json::Value, (String, String)> {
        let mut stream = StdUnixStream::connect(&self.socket)
            .map_err(|e| ("authority_unavailable".into(), e.to_string()))?;
        stream
            .set_read_timeout(Some(TIMEOUT))
            .map_err(|e| ("transport".into(), e.to_string()))?;
        stream
            .set_write_timeout(Some(TIMEOUT))
            .map_err(|e| ("transport".into(), e.to_string()))?;
        let mut reader = BufReader::new(
            stream
                .try_clone()
                .map_err(|e| ("transport".into(), e.to_string()))?,
        );
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| ("transport".into(), e.to_string()))?;
        let challenge: Challenge = serde_json::from_str(&line).map_err(|_| {
            (
                "unauthorized".into(),
                "authority refused connection or challenge".into(),
            )
        })?;
        if challenge.version != 1 {
            return Err(("protocol".into(), "unsupported challenge version".into()));
        }
        let nonce = hex::decode(&challenge.nonce)
            .map_err(|_| ("protocol".into(), "bad challenge".into()))?;
        if nonce.len() != 32 {
            return Err(("protocol".into(), "bad challenge length".into()));
        }
        let mac = hex::encode(signature(&self.key, &nonce));
        writeln!(stream, "{}", serde_json::to_string(&Proof { mac }).unwrap())
            .map_err(|e| ("transport".into(), e.to_string()))?;
        writeln!(stream, "{}", serde_json::to_string(&request).unwrap())
            .map_err(|e| ("transport".into(), e.to_string()))?;
        let mut answer = String::new();
        reader
            .read_line(&mut answer)
            .map_err(|e| ("transport".into(), e.to_string()))?;
        let result: Response = serde_json::from_str(&answer).map_err(|_| {
            (
                "transport".into(),
                "missing or malformed authority response".into(),
            )
        })?;
        match result {
            Response::Ok { plan } => {
                serde_json::to_value(plan).map_err(|e| ("protocol".into(), e.to_string()))
            }
            Response::Data { data } => Ok(data),
            Response::Error { code, message } => Err((code, message)),
        }
    }
    pub fn call(&self, request: Request) -> Result<Plan, (String, String)> {
        let value = self.request(request)?;
        serde_json::from_value(value).map_err(|e| ("protocol".into(), e.to_string()))
    }
}
pub fn load_secret(path: &Path) -> io::Result<[u8; 32]> {
    key_file(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn secret_file_must_be_owned_private_regular_and_32_bytes() {
        let root = tempdir().unwrap();
        let key = root.path().join("secret.hex");
        fs::write(&key, format!("{}\n", "11".repeat(32))).unwrap();
        fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(load_secret(&key).unwrap(), [0x11u8; 32]);
        fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load_secret(&key).is_err());
        fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&key, "0a").unwrap();
        assert!(load_secret(&key).is_err());
        let alias = root.path().join("alias");
        std::os::unix::fs::symlink(&key, &alias).unwrap();
        assert!(load_secret(&alias).is_err());
    }

    #[test]
    fn challenge_mac_is_nonce_specific() {
        let key = [0x42; 32];
        assert_ne!(signature(&key, &[1u8; 32]), signature(&key, &[2u8; 32]));
        assert_ne!(
            signature(&key, &[1u8; 32]),
            signature(&[0x43; 32], &[1u8; 32])
        );
    }
}
