//! A real relay (the Cloudflare Worker under `wrangler dev`: the Worker, its
//! Durable Object and a local D1, real HTTP and WebSockets) that tests can
//! stop and start again on the same address and data, to play an outage.
//!
//! With `TACKLY_RELAY_URL` set, the tests talk to that relay instead (a
//! deployed one); then the relay cannot be stopped.

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::OnceLock,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use tackly_phone::infrastructure::server_url::ServerUrl;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    runtime::Runtime,
};

pub struct Relay {
    target: Target,
}

enum Target {
    Local {
        addr: SocketAddr,
        /// The runtime's devtools port: its own, so relays run side by side.
        inspector: u16,
        state: PathBuf,
        process: Option<Child>,
    },
    Deployed(ServerUrl),
}

/// The Worker's folder: in the repository, or `TACKLY_RELAY_DIR` (mutation
/// testing runs from a copy of the sources that has no `node_modules`).
fn worker_dir() -> PathBuf {
    std::env::var_os("TACKLY_RELAY_DIR").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/relay"),
        PathBuf::from,
    )
}

/// Builds the Worker once per test run; every `wrangler dev` uses that build.
fn build_worker() -> Result<()> {
    static BUILT: OnceLock<Result<(), String>> = OnceLock::new();
    BUILT
        .get_or_init(|| {
            // Its own environment: not the test run's (coverage flags, the
            // target folder), which belong to the host build, not the Worker.
            let mut command = Command::new("worker-build");
            command
                .arg("--release")
                .current_dir(worker_dir())
                .env_clear();
            for name in ["PATH", "HOME"] {
                if let Some(value) = std::env::var_os(name) {
                    command.env(name, value);
                }
            }
            let status = command
                .status()
                .map_err(|error| format!("worker-build did not run (Nix shell?): {error}"))?;
            if status.success() {
                Ok(())
            } else {
                Err(format!("worker-build failed: {status}"))
            }
        })
        .clone()
        .map_err(anyhow::Error::msg)
}

impl Relay {
    /// Picks an address and a data folder in `dir`, without serving yet: a
    /// relay that is "down" until [`Self::start`].
    pub fn reserve(dir: &Path) -> Result<Self> {
        if let Ok(url) = std::env::var("TACKLY_RELAY_URL") {
            return Ok(Self {
                target: Target::Deployed(url.parse().context("TACKLY_RELAY_URL")?),
            });
        }
        let free_port = || -> Result<SocketAddr> {
            Ok(std::net::TcpListener::bind("127.0.0.1:0")
                .context("reserve a port")?
                .local_addr()?)
        };
        let (addr, inspector) = (free_port()?, free_port()?.port());
        Ok(Self {
            target: Target::Local {
                addr,
                inspector,
                state: dir.join("relay-state"),
                process: None,
            },
        })
    }

    pub fn url(&self) -> ServerUrl {
        match &self.target {
            Target::Local { addr, .. } => ServerUrl::http(*addr),
            Target::Deployed(url) => url.clone(),
        }
    }

    /// Whether the tests talk to a deployed relay, which they cannot stop.
    pub fn is_deployed(&self) -> bool {
        matches!(self.target, Target::Deployed(_))
    }

    pub fn is_running(&self) -> bool {
        match &self.target {
            Target::Local { process, .. } => process.is_some(),
            Target::Deployed(_) => true,
        }
    }

    /// Starts serving and waits until it answers. Does nothing when it
    /// already runs.
    pub async fn start(&mut self) -> Result<()> {
        let Target::Local {
            addr,
            inspector,
            state,
            process,
        } = &mut self.target
        else {
            return Ok(());
        };
        if process.is_some() {
            return Ok(());
        }
        build_worker()?;
        let wrangler = worker_dir().join("node_modules/.bin/wrangler");
        ensure!(wrangler.exists(), "no wrangler: run `npm ci` in apps/relay");
        let mut command = Command::new(wrangler);
        command
            .args(["dev", "--ip", "127.0.0.1", "--port"])
            .arg(addr.port().to_string())
            .arg("--inspector-port")
            .arg(inspector.to_string())
            .arg("--persist-to")
            .arg(&*state)
            .args([
                "--log-level",
                "error",
                "--show-interactive-dev-session=false",
            ])
            .env("TACKLY_RELAY_PREBUILT", "1")
            .current_dir(worker_dir())
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let mut child = command.spawn().context("start wrangler dev")?;
        let deadline = Instant::now() + Duration::from_secs(60);
        while !healthy(*addr).await {
            if let Some(status) = child.try_wait()? {
                bail!("wrangler dev exited: {status}");
            }
            if Instant::now() > deadline {
                stop_process(&mut child);
                bail!("the relay did not answer within a minute");
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        *process = Some(child);
        Ok(())
    }

    /// The local D1 database file, for tests that play a hostile relay.
    pub fn database_file(&self) -> Result<PathBuf> {
        let Target::Local { state, .. } = &self.target else {
            bail!("a deployed relay's database is out of reach");
        };
        let dir = state.join("v3/d1/miniflare-D1DatabaseObject");
        for entry in std::fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
            let path = entry?.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "sqlite")
                && !path
                    .file_name()
                    .is_some_and(|name| name == "metadata.sqlite")
            {
                return Ok(path);
            }
        }
        bail!("no D1 database in {}", dir.display())
    }

    /// Stops serving and drops every connection. Does nothing when stopped.
    pub fn stop(&mut self) {
        if let Target::Local { process, .. } = &mut self.target
            && let Some(mut child) = process.take()
        {
            stop_process(&mut child);
        }
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Stops `wrangler dev` and the runtime it started (its process group).
fn stop_process(child: &mut Child) {
    #[cfg(unix)]
    if let Ok(group) = i32::try_from(child.id()) {
        // SAFETY: sends a signal; no memory is involved.
        unsafe { libc::killpg(group, libc::SIGKILL) };
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Whether `GET /health` answers 204.
async fn healthy(addr: SocketAddr) -> bool {
    let Ok(mut stream) = tokio::net::TcpStream::connect(addr).await else {
        return false;
    };
    let request = format!("GET /health HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).await.is_err() {
        return false;
    }
    let mut answer = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut answer)).await;
    answer.starts_with(b"HTTP/1.1 204")
}

/// A stand-in for the address search (Photon) so tests need no internet. It
/// knows two Lidl branches and nothing else.
pub struct FakeGeocoder {
    addr: SocketAddr,
    runtime: Option<Runtime>,
}

impl FakeGeocoder {
    pub fn start() -> Result<Self> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").context("reserve a port")?;
        listener.set_nonblocking(true)?;
        let addr = listener.local_addr()?;
        let runtime = Runtime::new().context("start the geocoder's runtime")?;
        runtime.spawn(async move {
            let app = axum::Router::new().route("/api/", axum::routing::get(photon_answer));
            let result = async {
                axum::serve(tokio::net::TcpListener::from_std(listener)?, app).await?;
                anyhow::Ok(())
            }
            .await;
            if let Err(error) = result {
                eprintln!("fake geocoder stopped: {error:#}");
            }
        });
        Ok(Self {
            addr,
            runtime: Some(runtime),
        })
    }

    /// The address to give the app as `TACKLY_GEOCODER`.
    pub fn url(&self) -> String {
        format!("http://{}/api/", self.addr)
    }
}

async fn photon_answer(
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> axum::Json<serde_json::Value> {
    let asked = query
        .get("q")
        .map(|text| text.to_lowercase())
        .unwrap_or_default();
    let branch = |longitude: f64, latitude: f64, street: &str, postcode: &str, city: &str| {
        serde_json::json!({
            "geometry": {"type": "Point", "coordinates": [longitude, latitude]},
            "properties": {"name": "Lidl", "street": street, "housenumber": "1", "postcode": postcode, "city": city},
        })
    };
    let features = if asked.contains("lidl") {
        vec![
            branch(9.3775, 48.8752, "Marbacher Straße", "71364", "Winnenden"),
            branch(9.4330, 48.9435, "Stuttgarter Straße", "71522", "Backnang"),
        ]
    } else {
        Vec::new()
    };
    axum::Json(serde_json::json!({"type": "FeatureCollection", "features": features}))
}

impl Drop for FakeGeocoder {
    // A runtime must not be dropped inside async code, where the tests drop this.
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}
