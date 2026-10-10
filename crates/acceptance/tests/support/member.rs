//! One family member = one real Tackly app window, driven through its web view.

use std::{
    path::Path,
    process::{Child, Command, Stdio},
};

use super::page::Page;

pub struct Member {
    pub page: Page,
    child: Child,
}

impl Drop for Member {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Member {
    pub async fn open(
        name: &str,
        binary: &Path,
        dir: &Path,
        server_url: &str,
        location: &str,
    ) -> Self {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let log = std::fs::File::create(dir.join(format!("{name}.log"))).unwrap();
        let child = Command::new(binary)
            .env("TACKLY_PROFILE", name)
            .env("TACKLY_DATA_DIR", dir.join(name))
            .env("TACKLY_SERVER_URL", server_url)
            .env("TACKLY_LOCATION", location)
            .env("TACKLY_UI_TEST_PORT", port.to_string())
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .spawn()
            .expect("start the Tackly app");
        Self {
            page: Page::connect(name, port).await,
            child,
        }
    }
}
