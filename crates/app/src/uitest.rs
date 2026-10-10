//! Remote control for the acceptance tests (cargo feature `ui-test`).
//!
//! With `TACKLY_UI_TEST_PORT` set, the app listens on 127.0.0.1 for one JSON
//! request per line, `{"js": "<function body>"}`, runs it inside the real web
//! view and answers `{"ok": <value>}` or `{"err": "<message>"}`. The tests use
//! it to click, type and read the page exactly as the DOM sees it.

use dioxus::{document, prelude::*};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};

pub fn install() {
    use_future(|| async {
        let Ok(port) = std::env::var("TACKLY_UI_TEST_PORT") else {
            return;
        };
        let listener = TcpListener::bind(format!("127.0.0.1:{port}"))
            .await
            .expect("bind the UI test port");
        loop {
            let Ok((socket, _)) = listener.accept().await else {
                continue;
            };
            let (read, mut write) = socket.into_split();
            let mut lines = BufReader::new(read).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let js = serde_json::from_str::<Value>(&line)
                    .ok()
                    .and_then(|request| request["js"].as_str().map(str::to_owned))
                    .unwrap_or_default();
                // The answer goes back with `dioxus.send`. The script then waits
                // for our acknowledgement: when it ends, the web view drops the
                // query, and a reply not yet read would be lost.
                let script = format!(
                    "let reply; \
                     try {{ reply = {{ ok: await (async () => {{ {js}\n }})() }}; }} \
                     catch (e) {{ reply = {{ err: String((e && e.message) || e) }}; }} \
                     dioxus.send(reply); \
                     await dioxus.recv();"
                );
                let mut eval = document::eval(&script);
                let reply = match eval.recv::<Value>().await {
                    Ok(value) => value,
                    Err(error) => json!({ "err": error.to_string() }),
                };
                let _ = eval.send(true);
                if write
                    .write_all(format!("{reply}\n").as_bytes())
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }
    });
}
