//! One family member = one real Tackly app window, driven through its web view.

use std::{
    path::Path,
    process::{Child, Command, Stdio},
};

use super::{Outcome, page::Page};

/// The running app. Dropping it closes the window, also when starting failed
/// halfway.
struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        // The window may already be gone; nothing more to do then.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub struct Member {
    pub page: Page,
    _process: Process,
}

impl Member {
    /// Starts the app for `name`, with its own data folder and the relay's
    /// address, and connects to it.
    pub async fn open(
        name: &str,
        binary: &Path,
        dir: &Path,
        server_url: &str,
        location: &str,
    ) -> Outcome<Self> {
        let port = std::net::TcpListener::bind("127.0.0.1:0")?
            .local_addr()?
            .port();
        let log = std::fs::File::create(dir.join(format!("{name}.log")))?;
        let process = Process(
            Command::new(binary)
                .env("TACKLY_PROFILE", name)
                .env("TACKLY_DATA_DIR", dir.join(name))
                .env("TACKLY_SERVER_URL", server_url)
                .env("TACKLY_LOCATION", location)
                .env("TACKLY_UI_TEST_PORT", port.to_string())
                .stdout(Stdio::from(log.try_clone()?))
                .stderr(Stdio::from(log))
                .spawn()?,
        );
        let page = Page::connect(name, port).await?;
        Ok(Self {
            page,
            _process: process,
        })
    }
}
