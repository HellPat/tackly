//! A Playwright-style API over the app's web view: `Page`, `Locator`, and
//! `expect`. Semantics follow Playwright: locators are lazy and strict (an
//! action on a locator that matches several elements is an error), actions
//! auto-wait for the element to be visible, enabled, stable and receiving
//! pointer events, and `expect` assertions retry until they hold or time out.
//!
//! The browser protocol itself is not Playwright's: the app's embedded web
//! view has no CDP, so the engine in `driver.js` runs inside the page and is
//! reached through the app's `ui-test` bridge.

#![allow(dead_code)] // a general API; each scenario file uses part of it
#![allow(clippy::wrong_self_convention)] // `to_be_visible` etc. are named after Playwright

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    sync::Mutex,
};

use super::{Failure, Outcome};

pub const TIMEOUT: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_millis(100);

#[derive(Clone)]
pub struct Page {
    io: Arc<Mutex<BufReader<TcpStream>>>,
    pub name: String,
}

impl Page {
    /// Connects to the app's test port and installs the engine in its page.
    pub(super) async fn connect(name: &str, port: u16) -> Outcome<Self> {
        let deadline = Instant::now() + TIMEOUT;
        let stream = loop {
            match TcpStream::connect(("127.0.0.1", port)).await {
                Ok(stream) => break stream,
                Err(_) if Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                Err(error) => {
                    return Err(Failure(format!(
                        "{name}'s app never opened its test port: {error}"
                    )));
                }
            }
        };
        let page = Self {
            io: Arc::new(Mutex::new(BufReader::new(stream))),
            name: name.to_owned(),
        };
        // The page may still be starting; retry until the engine is installed.
        let deadline = Instant::now() + TIMEOUT;
        loop {
            match page.raw(include_str!("driver.js")).await {
                Ok(_) => return Ok(page),
                Err(_) if Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
                Err(error) => {
                    return Err(Failure(format!("could not drive {name}'s window: {error}")));
                }
            }
        }
    }

    /// Runs JavaScript in the page and returns its result.
    async fn raw(&self, js: &str) -> Outcome<Value> {
        let mut io = self.io.lock().await;
        let request = format!("{}\n", json!({ "js": js }));
        io.get_mut().write_all(request.as_bytes()).await?;
        let mut line = String::new();
        io.read_line(&mut line).await?;
        let reply: Value = serde_json::from_str(&line)
            .map_err(|error| Failure(format!("bad reply {line:?}: {error}")))?;
        match reply.get("err") {
            Some(error) => Err(Failure(error.as_str().unwrap_or("error").to_owned())),
            None => Ok(reply["ok"].clone()),
        }
    }

    pub async fn text(&self) -> String {
        match self.raw("return pw.pageText()").await {
            Ok(Value::String(text)) => text,
            _ => String::new(),
        }
    }

    /// A failure that also shows what is on the screen.
    async fn fail(&self, message: String) -> Failure {
        Failure(format!(
            "{}: {message}\n  page text: {}",
            self.name,
            self.text().await
        ))
    }

    pub fn locator(&self, css: &str) -> Locator {
        Locator::root(self.clone()).locator(css)
    }

    pub fn get_by_role(&self, role: &str, name: &str) -> Locator {
        Locator::root(self.clone()).get_by_role(role, name)
    }

    pub fn get_by_role_exact(&self, role: &str, name: &str) -> Locator {
        Locator::root(self.clone()).get_by_role_exact(role, name)
    }

    pub fn get_by_label(&self, text: &str) -> Locator {
        Locator::root(self.clone()).get_by_label(text)
    }

    pub fn get_by_text(&self, text: &str) -> Locator {
        Locator::root(self.clone()).get_by_text(text)
    }
}

#[derive(Clone)]
pub struct Locator {
    page: Page,
    chain: Vec<Value>,
    desc: String,
}

/// A string as a JavaScript literal.
fn quoted(text: &str) -> String {
    Value::String(text.to_owned()).to_string()
}

impl Locator {
    fn root(page: Page) -> Self {
        Self {
            page,
            chain: vec![],
            desc: "page".into(),
        }
    }

    fn then(&self, step: Value, desc: String) -> Self {
        let mut chain = self.chain.clone();
        chain.push(step);
        let desc = if self.chain.is_empty() {
            desc
        } else {
            format!("{}.{desc}", self.desc)
        };
        Self {
            page: self.page.clone(),
            chain,
            desc,
        }
    }

    pub fn locator(&self, css: &str) -> Self {
        self.then(
            json!({"k": "css", "v": css}),
            format!("locator({})", quoted(css)),
        )
    }

    pub fn get_by_role(&self, role: &str, name: &str) -> Self {
        self.then(
            json!({"k": "role", "role": role, "name": name, "exact": false}),
            format!("getByRole({}, name={})", quoted(role), quoted(name)),
        )
    }

    /// Like Playwright's `{ exact: true }`: the whole name, case-sensitively.
    pub fn get_by_role_exact(&self, role: &str, name: &str) -> Self {
        self.then(
            json!({"k": "role", "role": role, "name": name, "exact": true}),
            format!("getByRole({}, name={}, exact)", quoted(role), quoted(name)),
        )
    }

    pub fn get_by_label(&self, text: &str) -> Self {
        self.then(
            json!({"k": "label", "v": text, "exact": true}),
            format!("getByLabel({})", quoted(text)),
        )
    }

    pub fn get_by_text(&self, text: &str) -> Self {
        self.then(
            json!({"k": "text", "v": text, "exact": false}),
            format!("getByText({})", quoted(text)),
        )
    }

    pub fn filter_has_text(&self, text: &str) -> Self {
        self.then(
            json!({"k": "filter", "hasText": text}),
            format!("filter(hasText={})", quoted(text)),
        )
    }

    pub fn filter_has_not_text(&self, text: &str) -> Self {
        self.then(
            json!({"k": "filter", "hasNotText": text}),
            format!("filter(hasNotText={})", quoted(text)),
        )
    }

    pub fn first(&self) -> Self {
        self.then(json!({"k": "nth", "n": 0}), "first()".into())
    }

    pub fn last(&self) -> Self {
        self.then(json!({"k": "nth", "n": -1}), "last()".into())
    }

    pub fn nth(&self, n: usize) -> Self {
        self.then(json!({"k": "nth", "n": n}), format!("nth({n})"))
    }

    // ---- actions (auto-waiting) --------------------------------------------------

    /// Tries `op` until the element is ready for it, or the timeout passes.
    async fn act(&self, op: &str, arg: Value) -> Outcome<Value> {
        let script = format!(
            "return await pw.act({}, {}, {}, {})",
            Value::Array(self.chain.clone()),
            quoted(op),
            arg,
            quoted(&self.desc)
        );
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let last = match self.page.raw(&script).await {
                Ok(reply) if reply["status"] == "done" => return Ok(reply["value"].clone()),
                Ok(reply) => reply["reason"].as_str().unwrap_or("not ready").to_owned(),
                // Several elements match: waiting will not help.
                Err(Failure(error)) if error.starts_with("strict mode violation") => {
                    return Err(self.page.fail(error).await);
                }
                Err(Failure(error)) => error,
            };
            if Instant::now() >= deadline {
                return Err(self
                    .page
                    .fail(format!(
                        "locator.{op}: timeout {}ms exceeded\n  locator: {}\n  last: {last}",
                        TIMEOUT.as_millis(),
                        self.desc
                    ))
                    .await);
            }
            tokio::time::sleep(POLL).await;
        }
    }

    pub async fn click(&self) -> Outcome<()> {
        self.act("click", Value::Null).await.map(drop)
    }

    pub async fn check(&self) -> Outcome<()> {
        self.act("check", Value::Null).await.map(drop)
    }

    /// Sets the whole value at once, like a paste.
    /// Puts a file from disk into a file field, like Playwright's `setInputFiles`.
    pub async fn set_input_files(&self, path: &std::path::Path, mime: &str) -> Outcome<()> {
        let bytes = std::fs::read(path)?;
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.act(
            "set_input_files",
            json!({ "name": name, "mime": mime, "base64": STANDARD.encode(bytes) }),
        )
        .await
        .map(drop)
    }

    pub async fn fill(&self, text: &str) -> Outcome<()> {
        self.act("fill", json!(text)).await.map(drop)
    }

    /// Types one key at a time: keydown, keypress, beforeinput, input, keyup,
    /// with `delay` between keys. Each key is its own call, so the app has
    /// handled the previous one before the next arrives.
    pub async fn press_sequentially(&self, text: &str, delay: Duration) -> Outcome<()> {
        for ch in text.chars() {
            self.act("type_key", json!(ch.to_string())).await?;
            tokio::time::sleep(delay).await;
        }
        Ok(())
    }

    pub async fn press(&self, key: &str) -> Outcome<()> {
        self.act("press", json!(key)).await.map(drop)
    }

    pub async fn input_value(&self) -> Outcome<String> {
        let value = self.act("input_value", Value::Null).await?;
        Ok(value.as_str().unwrap_or_default().to_owned())
    }

    pub async fn text_content(&self) -> Outcome<String> {
        let value = self.act("text_content", Value::Null).await?;
        Ok(value.as_str().unwrap_or_default().to_owned())
    }

    /// Reads the QR code drawn inside this element, like a phone's camera.
    pub async fn decode_qr(&self) -> Outcome<String> {
        let reply = self.act("qr_pixels", Value::Null).await?;
        let size = reply["size"]
            .as_u64()
            .ok_or("the page sent no picture size")? as usize;
        let grey = STANDARD.decode(reply["grey"].as_str().ok_or("the page sent no picture")?)?;
        if grey.len() != size * size {
            return Err("the picture has the wrong size".into());
        }
        let mut image =
            rqrr::PreparedImage::prepare_from_greyscale(size, size, |x, y| grey[y * size + x]);
        let grids = image.detect_grids();
        let Some(grid) = grids.first() else {
            // Keep what was drawn, to see why nothing was detected.
            let path = std::env::temp_dir().join(format!("{}-qr.pgm", self.page.name));
            let mut pgm = format!("P5\n{size} {size}\n255\n").into_bytes();
            pgm.extend_from_slice(&grey);
            let _ = std::fs::write(&path, pgm);
            return Err(Failure(format!(
                "{}: no QR code found in {} (picture saved to {})",
                self.page.name,
                self.desc,
                path.display()
            )));
        };
        let (_, content) = grid
            .decode()
            .map_err(|error| Failure(format!("{}: unreadable QR code: {error}", self.page.name)))?;
        Ok(content)
    }

    async fn query(&self) -> Value {
        let script = format!("return pw.query({})", Value::Array(self.chain.clone()));
        self.page.raw(&script).await.unwrap_or(Value::Null)
    }
}

// ---- expect --------------------------------------------------------------------

pub fn expect(locator: &Locator) -> Expect {
    Expect {
        locator: locator.clone(),
        negate: false,
    }
}

pub struct Expect {
    locator: Locator,
    negate: bool,
}

impl Expect {
    pub fn not(mut self) -> Self {
        self.negate = true;
        self
    }

    /// Waits until `holds` is true of the locator's state (or false, after
    /// `not`), or fails with what it last saw.
    async fn retry(&self, expectation: String, holds: impl Fn(&Value) -> bool) -> Outcome<()> {
        let deadline = Instant::now() + TIMEOUT;
        let received = loop {
            let received = self.locator.query().await;
            if received["count"].as_u64().unwrap_or(0) > 1 {
                return Err(self
                    .locator
                    .page
                    .fail(format!(
                        "strict mode violation: {} resolved to {} elements: {}",
                        self.locator.desc, received["count"], received["items"]
                    ))
                    .await);
            }
            if holds(&received) != self.negate {
                return Ok(());
            }
            if Instant::now() >= deadline {
                break received;
            }
            tokio::time::sleep(POLL).await;
        };
        let not = if self.negate { ".not" } else { "" };
        Err(self
            .locator
            .page
            .fail(format!(
                "expect(locator){not}.{expectation} timed out after {}s\n  locator: {}\n  received: {received}",
                TIMEOUT.as_secs(),
                self.locator.desc
            ))
            .await)
    }

    pub async fn to_be_visible(self) -> Outcome<()> {
        self.retry("toBeVisible()".into(), |q| {
            q["count"] == 1 && q["items"][0]["visible"] == true
        })
        .await
    }

    pub async fn to_be_hidden(self) -> Outcome<()> {
        self.retry("toBeHidden()".into(), |q| {
            q["count"] == 0 || q["items"][0]["visible"] == false
        })
        .await
    }

    pub async fn to_be_enabled(self) -> Outcome<()> {
        self.retry("toBeEnabled()".into(), |q| {
            q["count"] == 1 && q["items"][0]["enabled"] == true
        })
        .await
    }

    pub async fn to_be_disabled(self) -> Outcome<()> {
        self.retry("toBeDisabled()".into(), |q| {
            q["count"] == 1 && q["items"][0]["enabled"] == false
        })
        .await
    }

    pub async fn to_be_checked(self) -> Outcome<()> {
        self.retry("toBeChecked()".into(), |q| {
            q["count"] == 1 && q["items"][0]["checked"] == true
        })
        .await
    }

    pub async fn to_have_count(self, count: usize) -> Outcome<()> {
        self.retry(format!("toHaveCount({count})"), move |q| {
            q["count"] == count
        })
        .await
    }

    pub async fn to_have_text(self, text: &str) -> Outcome<()> {
        let wanted = text.to_owned();
        self.retry(format!("toHaveText({})", quoted(text)), move |q| {
            q["count"] == 1 && q["items"][0]["text"] == wanted.as_str()
        })
        .await
    }

    pub async fn to_contain_text(self, text: &str) -> Outcome<()> {
        let wanted = text.to_owned();
        self.retry(format!("toContainText({})", quoted(text)), move |q| {
            q["count"] == 1
                && q["items"][0]["text"]
                    .as_str()
                    .is_some_and(|actual| actual.contains(&wanted))
        })
        .await
    }

    pub async fn to_have_value(self, value: &str) -> Outcome<()> {
        let wanted = value.to_owned();
        self.retry(format!("toHaveValue({})", quoted(value)), move |q| {
            q["count"] == 1 && q["items"][0]["value"] == wanted.as_str()
        })
        .await
    }
}
