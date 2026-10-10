//! The real sync server, in this process, on a fixed port so it can be
//! stopped and started again while the apps keep their configured address.

use std::{net::SocketAddr, path::Path};

pub struct Server {
    addr: SocketAddr,
    db: String,
    runtime: Option<tokio::runtime::Runtime>,
}

impl Server {
    pub fn reserve(dir: &Path) -> Self {
        let addr = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        Self {
            addr,
            db: format!("sqlite://{}", dir.join("relay.db").display()),
            runtime: None,
        }
    }

    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn running(&self) -> bool {
        self.runtime.is_some()
    }

    pub async fn start(&mut self) {
        if self.running() {
            return;
        }
        // Its own runtime, so stopping also drops open SSE connections.
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (db, addr) = (self.db.clone(), self.addr);
        let (ready, listening) = tokio::sync::oneshot::channel();
        runtime.spawn(async move {
            let pool = tackly_sync::connect(&db).await.unwrap();
            tackly_sync::migrate(&pool).await.unwrap();
            let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
            ready.send(()).unwrap();
            axum::serve(listener, tackly_sync::router(pool))
                .await
                .unwrap();
        });
        listening.await.expect("server failed to start");
        self.runtime = Some(runtime);
    }

    pub fn stop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}
