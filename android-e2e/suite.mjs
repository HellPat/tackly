// Playwright drives the real Tackly app on an Android emulator.
//
//  - Locators and assertions go through Playwright's Page on the app's WebView
//    (CDP, trusted events).
//  - Real operating-system input comes from adb: taps, the keyboard, the
//    location permission dialog, the emulator's GPS.
//  - The other phone is `tackly-probe`: a family head without a screen on the
//    same relay. It invites the emulator, lets it in, and reports what it
//    receives, so the test checks what really synced.
//
// Needs: an emulator or device on adb, the debug APK installed, and the relay
// and probe built (`cargo build -p tackly-sync -p tackly-testkit`).
import { spawn } from "node:child_process";
import { mkdtempSync, readFileSync } from "node:fs";
import { createInterface } from "node:readline";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { expect } from "@playwright/test";
import { _android as android } from "playwright-core";

const PACKAGE = "dev.tackly.tackly";
const ACTIVITY = `${PACKAGE}/dev.dioxus.main.MainActivity`;
const PORT = 3100;
// Winnenden. `geo fix` takes longitude first.
const HERE = { latitude: 48.8752, longitude: 9.3775 };
const root = resolve(import.meta.dirname, "..");
const dir = mkdtempSync(join(tmpdir(), "tackly-android-"));

function startRelay() {
  return spawn(join(root, "target/debug/tackly-sync"), [], {
    env: { ...process.env, DATABASE_URL: `sqlite://${dir}/relay.db`, TACKLY_BIND: `127.0.0.1:${PORT}` },
    stdio: "inherit",
  });
}

/** The probe phone: one JSON command per line in, one JSON answer per line out. */
function startProbe() {
  const probe = spawn(join(root, "target/debug/tackly-probe"), [join(dir, "probe"), `http://127.0.0.1:${PORT}`], {
    stdio: ["pipe", "pipe", "inherit"],
  });
  const answers = createInterface({ input: probe.stdout })[Symbol.asyncIterator]();
  const ask = async (cmd) => {
    probe.stdin.write(`${JSON.stringify({ cmd })}\n`);
    const { value, done } = await answers.next();
    if (done) throw new Error(`the probe quit while answering ${cmd}`);
    return JSON.parse(value);
  };
  return { probe, ask };
}

/** Waits until the probe's view of the family passes `check`. */
async function probeSees(ask, what, check) {
  for (let i = 0; i < 60; i++) {
    if (check(await ask("state"))) return;
    await new Promise((done) => setTimeout(done, 500));
  }
  throw new Error(`the probe never saw ${what}: ${JSON.stringify(await ask("state"))}`);
}

const step = async (name, action) => {
  const started = Date.now();
  await action();
  console.log(`  ok  ${name} (${Date.now() - started} ms)`);
};

const relay = startRelay();
await new Promise((done) => setTimeout(done, 1000));
const { probe, ask } = startProbe();
let device;
let failed = false;
try {
  const devices = await android.devices();
  if (devices.length === 0) throw new Error("no Android device on adb");
  device = devices[0];
  console.log(`device: ${device.model()} (${device.serial()})`);

  // The app reaches the relay at 127.0.0.1 too, the same address the probe
  // puts into its invitation link.
  await runAdb(["-s", device.serial(), "reverse", `tcp:${PORT}`, `tcp:${PORT}`]);
  await runAdb(["-s", device.serial(), "emu", "geo", "fix", String(HERE.longitude), String(HERE.latitude)]);
  await device.shell("settings put secure location_mode 3");

  await device.shell(`pm clear ${PACKAGE}`);
  await device.shell(`am start -W -n ${ACTIVITY}`);
  await new Promise((done) => setTimeout(done, 4000));
  const webview = await device.webView({ pkg: PACKAGE });
  const page = await webview.page();
  console.log(`webview page: ${await page.title()} ${page.url()}`);

  // Where the WebView is on the screen, for real taps. Playwright's driver on
  // the device sees Android's own views, the WebView among them.
  const { bounds } = await device.info({ clazz: "android.webkit.WebView" });
  const origin = { x: bounds.x, y: bounds.y };
  const scale = await page.evaluate(() => window.devicePixelRatio);
  const tap = async (locator) => {
    // Like a person: close the on-screen keyboard first, it may cover the target.
    await device.shell("input keyevent 111");
    await locator.scrollIntoViewIfNeeded();
    const box = await locator.boundingBox();
    if (!box) throw new Error("nothing to tap");
    await device.input.tap({
      x: Math.round(origin.x + (box.x + box.width / 2) * scale),
      y: Math.round(origin.y + (box.y + box.height / 2) * scale),
    });
  };

  await step("join the probe's family with an invitation", async () => {
    const { link } = await ask("invite");
    await tap(page.getByRole("button", { name: "Join with an invitation", exact: true }));
    await page.getByLabel("Your name").focus();
    // AndroidInput.type drops capital letters (no Shift), so type lowercase.
    await device.input.type("mona");
    await page.getByLabel("Invitation link").fill(link);
    await tap(page.getByRole("button", { name: "Ask to join", exact: true }));
    const { code } = await ask("approve");
    await expect(page.getByLabel("Confirmation code")).toHaveText(code);
    await expect(page.locator("nav[aria-label=Main]")).toBeVisible({ timeout: 15000 });
  });

  await step("allow the location in Android's own dialog", async () => {
    // The app asks once, right after joining. The dialog belongs to Android,
    // not to the WebView, so tap it through Playwright's device selectors.
    const allow = { res: "com.android.permissioncontroller:id/permission_allow_foreground_only_button" };
    await device.wait(allow, { timeout: 20000 });
    await device.tap(allow);
    const granted = (await device.shell(`dumpsys package ${PACKAGE}`)).toString();
    if (!/ACCESS_FINE_LOCATION: granted=true/.test(granted)) throw new Error("the location permission is not granted");
  });

  await step("tick a task off: the probe gets where it was done", async () => {
    await page.getByLabel("Add a task").focus();
    await device.input.type("waterplants");
    await device.input.press("Enter");
    await tap(page.locator("[aria-label=Filter]").getByRole("button", { name: /^All/ }));
    const card = page.locator("main li").filter({ hasText: "waterplants" });
    await expect(card).toBeVisible();
    await tap(card.getByRole("checkbox"));
    await expect(card).toBeHidden();
    await probeSees(ask, "the finished task with a location", (family) =>
      Object.values(family.tasks).some((task) => {
        const where = /^waterplants$/i.test(task.title) && task.done?.location;
        return where && Math.abs(where.latitude - HERE.latitude) < 0.001 && Math.abs(where.longitude - HERE.longitude) < 0.001;
      }),
    );
  });

  await step("choose a photo as my picture: the probe gets it", async () => {
    await tap(page.locator("nav[aria-label=Main]").getByRole("button", { name: "Family", exact: true }));
    await tap(page.locator("main").getByRole("button", { name: /mona/i }).first());
    const photo = readFileSync(join(root, "crates/acceptance/tests/fixtures/photo.jpg"));
    await page.getByLabel("Choose a photo").setInputFiles({ name: "photo.jpg", mimeType: "image/jpeg", buffer: photo });
    await probeSees(ask, "Mona's photo", (family) =>
      Object.values(family.members).some(
        (member) => member.name === "mona" && member.picture?.photo?.startsWith("data:image/jpeg;base64,"),
      ),
    );
  });

  const shot = join(tmpdir(), "tackly-android-final.png");
  await device.screenshot({ path: shot });
  console.log(`screenshot: ${shot}`);
  console.log("ANDROID E2E PASSED");
} catch (error) {
  failed = true;
  console.error(error);
} finally {
  probe.kill();
  relay.kill();
  if (device) {
    // Closing can hang once the app is idle; do not wait for it forever.
    await Promise.race([device.close(), new Promise((done) => setTimeout(done, 5000))]);
  }
}
process.exit(failed ? 1 : 0);

function runAdb(args) {
  return new Promise((done, fail) => {
    const adb = spawn("adb", args, { stdio: "inherit" });
    adb.on("exit", (code) => (code === 0 ? done() : fail(new Error(`adb ${args.join(" ")} failed`))));
  });
}
