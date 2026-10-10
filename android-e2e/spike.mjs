// Spike: Playwright drives the real Tackly app on an Android emulator.
//
//  - Locators and assertions go through Playwright's Page on the app's WebView
//    (CDP, trusted events), exactly like the desktop-free "page" API.
//  - Real operating-system input comes from AndroidInput over adb:
//    device.input.tap / type / press.
//
// Needs: an emulator or device on adb, the debug APK installed, and the relay
// binary built (`cargo build -p tackly-sync`).
import { spawn } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { expect } from "@playwright/test";
import { _android as android } from "playwright-core";

const PACKAGE = "dev.tackly.tackly";
const ACTIVITY = `${PACKAGE}/dev.dioxus.main.MainActivity`;
const PORT = 3100;
const root = resolve(import.meta.dirname, "..");

function startRelay() {
  const dir = mkdtempSync(join(tmpdir(), "tackly-android-"));
  const relay = spawn(join(root, "target/debug/tackly-sync"), [], {
    env: { ...process.env, DATABASE_URL: `sqlite://${dir}/relay.db`, TACKLY_BIND: `0.0.0.0:${PORT}` },
    stdio: "inherit",
  });
  return relay;
}

const step = async (name, action) => {
  const started = Date.now();
  await action();
  console.log(`  ok  ${name} (${Date.now() - started} ms)`);
};

const relay = startRelay();
let device;
let failed = false;
try {
  const devices = await android.devices();
  if (devices.length === 0) throw new Error("no Android device on adb");
  device = devices[0];
  console.log(`device: ${device.model()} (${device.serial()})`);

  await device.shell(`pm clear ${PACKAGE}`);
  await device.shell(`am start -W -n ${ACTIVITY}`);
  // Attaching while the app is still starting made the debug build panic
  // ("capacity overflow"), so give the first render a moment.
  await new Promise((resolve) => setTimeout(resolve, 4000));
  const webview = await device.webView({ pkg: PACKAGE });
  const page = await webview.page();
  console.log(`webview page: ${await page.title()} ${page.url()}`);

  // Where the WebView is on the screen, for real taps.
  // (uiautomator ships with Android; Playwright's own driver APK is not needed.)
  const hierarchy = await device.shell("uiautomator dump /dev/stdout");
  const bounds = /class="android\.webkit\.WebView"[^>]*bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"/.exec(
    hierarchy.toString(),
  );
  if (!bounds) throw new Error("could not find the WebView on screen");
  const origin = { x: Number(bounds[1]), y: Number(bounds[2]) };
  const scale = await page.evaluate(() => window.devicePixelRatio);
  const tap = async (locator) => {
    const box = await locator.boundingBox();
    if (!box) throw new Error("nothing to tap");
    await device.input.tap({
      x: Math.round(origin.x + (box.x + box.width / 2) * scale),
      y: Math.round(origin.y + (box.y + box.height / 2) * scale),
    });
  };

  await step("create a family (real taps, real keyboard)", async () => {
    await tap(page.getByRole("button", { name: "Create a family", exact: true }));
    await page.getByLabel("Your name").focus();
    await device.input.type("patrick");
    await page.getByLabel("Family name").focus();
    // AndroidInput.type drops capital letters (no Shift), so type lowercase.
    await device.input.type("thesmiths");
    await page.getByLabel("Server").fill(`http://10.0.2.2:${PORT}`);
    await tap(page.getByRole("button", { name: "Create", exact: true }));
    await expect(page.locator(".sub")).toContainText("thesmiths");
    await expect(page.locator(".sync.on")).toBeVisible();
  });

  await step("add a task with the keyboard and Enter", async () => {
    await page.getByLabel("Add a task").focus();
    await device.input.type("waterplants");
    await device.input.press("Enter");
    await expect(page.locator(".card").filter({ hasText: "waterplants" })).toBeVisible();
  });

  await step("start and finish it (Playwright locators)", async () => {
    const card = page.locator(".card").filter({ hasText: "waterplants" });
    await card.getByRole("button", { name: "Start", exact: true }).click();
    await expect(card).toContainText("You are on it");
    await card.getByRole("button", { name: "Finish", exact: true }).click();
    await page.getByLabel("Note (optional)").pressSequentially("done");
    await page.getByRole("button", { name: "Done", exact: true }).click();
    await expect(card).toContainText("Done by You");
    await expect(card).toContainText("💬 done");
  });

  const shot = join(tmpdir(), "tackly-android-final.png");
  await device.screenshot({ path: shot });
  console.log(`screenshot: ${shot}`);
  console.log("SPIKE PASSED");
} catch (error) {
  failed = true;
  console.error(error);
} finally {
  relay.kill();
  if (device) {
    // Closing can hang once the app is idle; do not wait for it forever.
    await Promise.race([device.close(), new Promise((done) => setTimeout(done, 5000))]);
  }
}
process.exit(failed ? 1 : 0);
