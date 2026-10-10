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
