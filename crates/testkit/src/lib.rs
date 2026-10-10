//! A real relay (the server's router, a SQLite file, real HTTP) that tests can
//! stop and start again on the same address, to play a server outage.

use std::{net::SocketAddr, path::Path};

use anyhow::{Context, Result};
use tokio::runtime::Runtime;

pub struct Relay {
    addr: SocketAddr,
    database_url: String,
    runtime: Option<Runtime>,
}

impl Relay {
    /// Picks an address and a database file in `dir`, without serving yet: a
    /// relay that is "down" until [`Self::start`].
    pub fn reserve(dir: &Path) -> Result<Self> {
        let addr = std::net::TcpListener::bind("127.0.0.1:0")
            .context("reserve a port")?
            .local_addr()?;
        Ok(Self {
            addr,
            database_url: format!("sqlite://{}", dir.join("relay.db").display()),
            runtime: None,
        })
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn is_running(&self) -> bool {
        self.runtime.is_some()
    }

    /// Starts serving. Does nothing when it already runs.
    pub async fn start(&mut self) -> Result<()> {
        if self.is_running() {
            return Ok(());
        }
        // Its own runtime, so that stopping also drops the open SSE connections.
        let runtime = Runtime::new().context("start the relay's runtime")?;
        let (database_url, addr) = (self.database_url.clone(), self.addr);
        let (ready, listening) = tokio::sync::oneshot::channel();
        runtime.spawn(async move {
            let result = serve(&database_url, addr, ready).await;
            if let Err(error) = result {
                eprintln!("test relay stopped: {error:#}");
            }
        });
        listening
            .await
            .context("the relay failed to start (see its log line above)")?;
        self.runtime = Some(runtime);
        Ok(())
    }

    /// Stops serving and drops every connection. Does nothing when stopped.
    pub fn stop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.stop();
    }
}

async fn serve(
    database_url: &str,
    addr: SocketAddr,
    ready: tokio::sync::oneshot::Sender<()>,
) -> Result<()> {
    let pool = tackly_sync::connect(database_url).await?;
    tackly_sync::migrate(&pool).await?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    // The receiver is gone only if the test gave up waiting.
    let _ = ready.send(());
    axum::serve(listener, tackly_sync::router(pool)).await?;
    Ok(())
}
