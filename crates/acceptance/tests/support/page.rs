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

pub const TIMEOUT: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_millis(100);

#[derive(Clone)]
pub struct Page {
    io: Arc<Mutex<BufReader<TcpStream>>>,
    pub name: String,
}

impl Page {
    pub(super) async fn connect(name: &str, port: u16) -> Self {
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
        let page = Self {
            io: Arc::new(Mutex::new(BufReader::new(stream))),
            name: name.to_owned(),
        };
        // The page may still be starting; retry until the engine is installed.
        let deadline = Instant::now() + TIMEOUT;
        loop {
            match page.raw(include_str!("driver.js")).await {
                Ok(_) => return page,
                Err(_) if Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(250)).await
                }
                Err(error) => panic!("could not drive {name}'s window: {error}"),
            }
        }
    }

    async fn raw(&self, js: &str) -> Result<Value, String> {
        let mut io = self.io.lock().await;
        let request = format!("{}\n", json!({ "js": js }));
        io.get_mut()
            .write_all(request.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        let mut line = String::new();
        io.read_line(&mut line).await.map_err(|e| e.to_string())?;
        let reply: Value =
            serde_json::from_str(&line).map_err(|e| format!("bad reply {line:?}: {e}"))?;
        match reply.get("err") {
            Some(error) => Err(error.as_str().unwrap_or("error").to_owned()),
            None => Ok(reply["ok"].clone()),
        }
    }

    pub async fn text(&self) -> String {
        self.raw("return pw.pageText()")
            .await
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default()
    }

    async fn fail(&self, message: String) -> ! {
        panic!(
            "{}: {message}\n  page text: {}",
            self.name,
            self.text().await
        );
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

fn q(text: &str) -> String {
    serde_json::to_string(text).unwrap()
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
            format!("locator({})", q(css)),
        )
    }

    pub fn get_by_role(&self, role: &str, name: &str) -> Self {
        self.then(
            json!({"k": "role", "role": role, "name": name, "exact": false}),
            format!("getByRole({}, name={})", q(role), q(name)),
        )
    }

    /// Like Playwright's `{ exact: true }`: the whole name, case-sensitively.
    pub fn get_by_role_exact(&self, role: &str, name: &str) -> Self {
        self.then(
            json!({"k": "role", "role": role, "name": name, "exact": true}),
            format!("getByRole({}, name={}, exact)", q(role), q(name)),
        )
    }

    pub fn get_by_label(&self, text: &str) -> Self {
        self.then(
            json!({"k": "label", "v": text, "exact": true}),
            format!("getByLabel({})", q(text)),
        )
    }

    pub fn get_by_text(&self, text: &str) -> Self {
        self.then(
            json!({"k": "text", "v": text, "exact": false}),
            format!("getByText({})", q(text)),
        )
    }

    pub fn filter_has_text(&self, text: &str) -> Self {
        self.then(
            json!({"k": "filter", "hasText": text}),
            format!("filter(hasText={})", q(text)),
        )
    }

    pub fn filter_has_not_text(&self, text: &str) -> Self {
        self.then(
            json!({"k": "filter", "hasNotText": text}),
            format!("filter(hasNotText={})", q(text)),
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

    // ---- actions (auto-waiting) ------------------------------------------------

    async fn act(&self, op: &str, arg: Value) -> Value {
        let script = format!(
            "return await pw.act({}, {}, {}, {})",
            Value::Array(self.chain.clone()),
            q(op),
            arg,
            q(&self.desc)
        );
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let last = match self.page.raw(&script).await {
                Ok(reply) if reply["status"] == "done" => return reply["value"].clone(),
                Ok(reply) => reply["reason"].as_str().unwrap_or("not ready").to_owned(),
                Err(error) if error.starts_with("strict mode violation") => {
                    self.page.fail(error).await
                }
                Err(error) => error,
            };
            if Instant::now() >= deadline {
                self.page
                    .fail(format!(
                        "locator.{op}: timeout {}ms exceeded\n  locator: {}\n  last: {last}",
                        TIMEOUT.as_millis(),
                        self.desc
                    ))
                    .await;
            }
            tokio::time::sleep(POLL).await;
        }
    }

    pub async fn click(&self) {
        self.act("click", Value::Null).await;
    }

    pub async fn check(&self) {
        self.act("check", Value::Null).await;
    }

    /// Sets the whole value at once, like a paste.
    pub async fn fill(&self, text: &str) {
        self.act("fill", json!(text)).await;
    }

    /// Types one key at a time: keydown, keypress, beforeinput, input, keyup,
    /// with `delay` between keys. Each key is its own call, so the app has
    /// handled the previous one before the next arrives.
    pub async fn press_sequentially(&self, text: &str, delay: Duration) {
        for ch in text.chars() {
            self.act("type_key", json!(ch.to_string())).await;
            tokio::time::sleep(delay).await;
        }
    }

    pub async fn press(&self, key: &str) {
        self.act("press", json!(key)).await;
    }

    pub async fn input_value(&self) -> String {
        self.act("input_value", Value::Null)
            .await
            .as_str()
            .unwrap_or_default()
            .to_owned()
    }

    pub async fn text_content(&self) -> String {
        self.act("text_content", Value::Null)
            .await
            .as_str()
            .unwrap_or_default()
            .to_owned()
    }

    /// Reads the QR code drawn inside this element, like a phone's camera.
    pub async fn decode_qr(&self) -> String {
        let reply = self.act("qr_pixels", Value::Null).await;
        let size = reply["size"].as_u64().unwrap() as usize;
        let grey = STANDARD.decode(reply["grey"].as_str().unwrap()).unwrap();
        // atob() gives one char per byte, and the JS encoded chars 0..=255
        // through btoa, so the bytes are the grey values.
        let mut image =
            rqrr::PreparedImage::prepare_from_greyscale(size, size, |x, y| grey[y * size + x]);
        let grids = image.detect_grids();
        let Some(grid) = grids.first() else {
            // Keep what was drawn, to see why nothing was detected.
            let path = std::env::temp_dir().join(format!("{}-qr.pgm", self.page.name));
            let mut pgm = format!("P5\n{size} {size}\n255\n").into_bytes();
            pgm.extend_from_slice(&grey);
            let _ = std::fs::write(&path, pgm);
            panic!(
                "{}: no QR code found in {} (picture saved to {})",
                self.page.name,
                self.desc,
                path.display()
            );
        };
        let (_, content) = grid
            .decode()
            .unwrap_or_else(|e| panic!("{}: unreadable QR code: {e}", self.page.name));
        content
    }

    async fn query(&self) -> Value {
        let script = format!("return pw.query({})", Value::Array(self.chain.clone()));
        self.page.raw(&script).await.unwrap_or(Value::Null)
    }
}

// ---- expect ------------------------------------------------------------------

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

    async fn retry(&self, expectation: String, holds: impl Fn(&Value) -> bool) {
        let deadline = Instant::now() + TIMEOUT;
        let received = loop {
            let received = self.locator.query().await;
            if received["count"].as_u64().unwrap_or(0) > 1 {
                self.locator
                    .page
                    .fail(format!(
                        "strict mode violation: {} resolved to {} elements: {}",
                        self.locator.desc, received["count"], received["items"]
                    ))
                    .await;
            }
            if holds(&received) != self.negate || Instant::now() >= deadline {
                break received;
            }
            tokio::time::sleep(POLL).await;
        };
        if holds(&received) != self.negate {
            return;
        }
        let not = if self.negate { ".not" } else { "" };
        self.locator
            .page
            .fail(format!(
                "expect(locator){not}.{expectation} timed out after {}s\n  locator: {}\n  received: {received}",
                TIMEOUT.as_secs(),
                self.locator.desc
            ))
            .await;
    }

    pub async fn to_be_visible(self) {
        self.retry("toBeVisible()".into(), |q| {
            q["count"] == 1 && q["items"][0]["visible"] == true
        })
        .await;
    }

    pub async fn to_be_hidden(self) {
        self.retry("toBeHidden()".into(), |q| {
            q["count"] == 0 || q["items"][0]["visible"] == false
        })
        .await;
    }

    pub async fn to_be_enabled(self) {
        self.retry("toBeEnabled()".into(), |q| {
            q["count"] == 1 && q["items"][0]["enabled"] == true
        })
        .await;
    }

    pub async fn to_be_disabled(self) {
        self.retry("toBeDisabled()".into(), |q| {
            q["count"] == 1 && q["items"][0]["enabled"] == false
        })
        .await;
    }

    pub async fn to_be_checked(self) {
        self.retry("toBeChecked()".into(), |q| {
            q["count"] == 1 && q["items"][0]["checked"] == true
        })
        .await;
    }

    pub async fn to_have_count(self, count: usize) {
        self.retry(format!("toHaveCount({count})"), move |q| {
            q["count"] == count
        })
        .await;
    }

    pub async fn to_have_text(self, text: &str) {
        let wanted = text.to_owned();
        self.retry(format!("toHaveText({})", q(text)), move |q| {
            q["count"] == 1 && q["items"][0]["text"] == wanted.as_str()
        })
        .await;
    }

    pub async fn to_contain_text(self, text: &str) {
        let wanted = text.to_owned();
        self.retry(format!("toContainText({})", q(text)), move |q| {
            q["count"] == 1
                && q["items"][0]["text"]
                    .as_str()
                    .is_some_and(|t| t.contains(&wanted))
        })
        .await;
    }

    pub async fn to_have_value(self, value: &str) {
        let wanted = value.to_owned();
        self.retry(format!("toHaveValue({})", q(value)), move |q| {
            q["count"] == 1 && q["items"][0]["value"] == wanted.as_str()
        })
        .await;
    }
}
