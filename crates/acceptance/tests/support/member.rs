//! One family member = one real Tackly app window, driven through its web view.

use std::{
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};

const TIMEOUT: Duration = Duration::from_secs(30);

pub struct Member {
    pub name: String,
    child: Child,
    io: BufReader<TcpStream>,
}

impl Drop for Member {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn quote(text: &str) -> String {
    serde_json::to_string(text).unwrap()
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
        let deadline = Instant::now() + TIMEOUT;
        let stream = loop {
            match TcpStream::connect(("127.0.0.1", port)).await {
                Ok(stream) => break stream,
                Err(_) if Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(200)).await
                }
                Err(error) => panic!("{name}'s app never opened its test port: {error}"),
            }
        };
        let mut member = Self {
            name: name.to_owned(),
            child,
            io: BufReader::new(stream),
        };
        member.install_driver().await;
        member
    }

    async fn install_driver(&mut self) {
        // The page may still be starting; retry until the script runs.
        let deadline = Instant::now() + TIMEOUT;
        loop {
            match self.raw(include_str!("driver.js")).await {
                Ok(_) => return,
                Err(_) if Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(250)).await
                }
                Err(error) => panic!("could not drive {}'s window: {error}", self.name),
            }
        }
    }

    async fn raw(&mut self, js: &str) -> Result<Value, String> {
        let request = format!("{}\n", json!({ "js": js }));
        self.io
            .get_mut()
            .write_all(request.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        let mut line = String::new();
        self.io
            .read_line(&mut line)
            .await
            .map_err(|e| e.to_string())?;
        let reply: Value =
            serde_json::from_str(&line).map_err(|e| format!("bad reply {line:?}: {e}"))?;
        match reply.get("err") {
            Some(error) => Err(error.as_str().unwrap_or("error").to_owned()),
            None => Ok(reply["ok"].clone()),
        }
    }

    /// Everything currently on screen, for failure messages.
    pub async fn screen(&mut self) -> String {
        self.raw("return T.text()")
            .await
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default()
    }

    async fn fail(&mut self, what: &str, error: &str) -> ! {
        let screen = self.screen().await;
        panic!("{}: {what}\n  {error}\n  on screen: {screen}", self.name);
    }

    /// Retries until the element can be clicked: it may still be appearing.
    pub async fn click(&mut self, spec: &str) {
        self.act(
            &format!("click {spec}"),
            &format!("return T.click({})", quote(spec)),
        )
        .await;
    }

    async fn act(&mut self, what: &str, js: &str) {
        let deadline = Instant::now() + TIMEOUT;
        let mut first_error = None;
        loop {
            match self.raw(js).await {
                Ok(_) => return,
                Err(error) => {
                    first_error.get_or_insert(error.clone());
                    if Instant::now() >= deadline {
                        let first = first_error.unwrap_or_default();
                        self.fail(
                            &format!("could not {what}"),
                            &format!("{error} (first error: {first})"),
                        )
                        .await;
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    }

    pub async fn fill(&mut self, spec: &str, text: &str) {
        self.act(
            &format!("type into {spec}"),
            &format!("return T.type({}, {})", quote(spec), quote(text)),
        )
        .await;
    }

    /// Waits until the JavaScript expression is truthy and returns its value.
    pub async fn wait_for(&mut self, what: &str, expression: &str) -> Value {
        let deadline = Instant::now() + TIMEOUT;
        let mut last = String::new();
        loop {
            match self.raw(&format!("return ({expression})")).await {
                Ok(Value::Null | Value::Bool(false)) => {}
                Ok(Value::String(text)) if text.is_empty() => {}
                Ok(value) => return value,
                Err(error) => last = error,
            }
            if Instant::now() >= deadline {
                self.fail(&format!("timed out waiting for {what}"), &last)
                    .await;
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    }

    pub async fn wait_for_text(&mut self, text: &str) {
        self.wait_for(
            &format!("the text {text:?}"),
            &format!("T.text().includes({})", quote(text)),
        )
        .await;
    }

    pub async fn read(&mut self, what: &str, spec_js: &str) -> String {
        self.wait_for(what, spec_js)
            .await
            .as_str()
            .unwrap_or_default()
            .to_owned()
    }

    /// The text of a task card once it contains all of `parts`.
    pub async fn wait_for_card(&mut self, title: &str, parts: &[String]) {
        let checks = parts
            .iter()
            .map(|part| format!("c.includes({})", quote(part)))
            .collect::<Vec<_>>()
            .join(" && ");
        let expression = format!(
            "(() => {{ const c = T.cardText({}); return c !== null && {} }})()",
            quote(title),
            if checks.is_empty() {
                "true".into()
            } else {
                checks
            }
        );
        self.wait_for(
            &format!("the card {title:?} to show {parts:?}"),
            &expression,
        )
        .await;
    }

    pub async fn has(&mut self, spec: &str) -> bool {
        self.raw(&format!("return T.has({})", quote(spec)))
            .await
            .map(|v| v == Value::Bool(true))
            .unwrap_or(false)
    }
}
