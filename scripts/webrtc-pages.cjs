#!/usr/bin/env node
// Two pages of the game meet for real in headless Chromium (issue #49): the WebRTC connection
// between two browsers, which the CI cannot check (it has neither signaling server nor browser).
//
// Needs `matchbox_server` (port 3536) and `trunk serve` running, Node, and Playwright with its
// Chromium (`npm install -g playwright && npx playwright install chromium`; PLAYWRIGHT=path
// names another install of it).
//
//   node scripts/webrtc-pages.cjs [GAME_URL] [--room CODE] [--query "stun=none&..."] [--media]
//                                 [--seconds 60] [--screenshot PREFIX]
//
// GAME_URL: http://127.0.0.1:8080/ by default (http://192.168.1.20:8080/ for the local network).
// Each page is its own browser context (like two windows of two profiles), with its own pseudo.
// The game's network lines of both consoles are printed; the script ends with 0 once both pages
// logged « connected », with 1 after 60 s (--seconds).
//
// --media grants the camera and the microphone (without using them): Chrome then gives its real
// local addresses instead of `….local` names. Without it, in some sandboxes, Chrome never ends
// gathering its ICE candidates (IPv6 disabled): the page's shim cuts the wait after 3 s.
// --screenshot PREFIX saves each page at the end (PREFIX-Ana.png, PREFIX-Bob.png): the lobby.
// CHROMIUM_ARGS adds switches to Chromium, e.g. a WebRTC policy that hides every address:
//   CHROMIUM_ARGS=--force-webrtc-ip-handling-policy=disable_non_proxied_udp

const playwright = require(process.env.PLAYWRIGHT || "playwright");

const args = process.argv.slice(2);
const option = (name) => {
  const i = args.indexOf(name);
  if (i < 0) return undefined;
  const [, value] = args.splice(i, 2);
  return value;
};
const media = args.includes("--media");
if (media) args.splice(args.indexOf("--media"), 1);
const room = option("--room") || `PAGES${Math.floor(Math.random() * 1e4)}`;
const extra = option("--query");
const limit = Number(option("--seconds") || 60) * 1000;
const screenshot = option("--screenshot");
const base = args[0] || "http://127.0.0.1:8080/";

const interesting = /network:|\[fluffy-net\]|matchbox_socket|panicked|ERROR/;

(async () => {
  const browser = await playwright.chromium.launch({
    headless: true,
    args: (process.env.CHROMIUM_ARGS || "").split(" ").filter(Boolean),
  });
  const start = Date.now();
  const seconds = () => ((Date.now() - start) / 1000).toFixed(2).padStart(6);
  const connected = new Set();
  const pages = [];
  for (const name of ["Ana", "Bob"]) {
    const context = await browser.newContext(
      media ? { permissions: ["camera", "microphone"] } : {},
    );
    const page = await context.newPage();
    pages.push([name, page]);
    page.on("console", (message) => {
      const text = message.text().replace(/%c/g, "").replace(/\s*color: \S+;?/g, "");
      if (!interesting.test(text)) return;
      // The SDPs of matchbox_socket's DEBUG lines are long: their first line is enough.
      console.log(`${seconds()} ${name}: ${text.split("\\r\\n")[0].slice(0, 300)}`);
      if (/network: #\w+ connected/.test(text)) connected.add(name);
    });
    page.on("pageerror", (error) => console.log(`${seconds()} ${name}: page error ${error}`));
    const url = `${base}?room=${room}&pseudo=${name}${extra ? `&${extra}` : ""}`;
    console.log(`${seconds()} ${name}: ${url}`);
    await page.goto(url);
  }
  while (connected.size < 2 && Date.now() - start < limit) {
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  const ok = connected.size === 2;
  if (screenshot) {
    // A moment for the lobby to show the ping.
    await new Promise((resolve) => setTimeout(resolve, 1500));
    for (const [name, page] of pages) {
      await page.screenshot({ path: `${screenshot}-${name}.png` });
    }
  }
  console.log(ok ? `both connected after ${seconds()} s` : `not connected: ${[...connected]}`);
  await browser.close();
  process.exit(ok ? 0 : 1);
})();
