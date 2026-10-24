# Capture a browser flow and import it into Plover

You can turn what a web app sends while you click through it into Plover requests:
record the browser's traffic as a HAR file, then import that file. This page shows how
to have Claude do the clicking, and the manual way with browser DevTools.

A HAR file holds every cookie, token and password the browser sent, in plain text.
Record with a **test account**, never with production credentials, and delete the file
after importing it.

## 1. Record the flow

### With Claude (Playwright)

Ask Claude to write and run a short Playwright script that does the flow step by step
and records a HAR. For example: "Open https://app.example.test, log in as the test
user, create a document, sign it, and record a HAR to `flow.har`."

Playwright records HAR files itself through the `recordHar` option of a browser
context; the file is written when the context closes:

```js
// record-flow.mjs: run with `node record-flow.mjs` after `npm i -D playwright`
import { chromium } from "playwright";

const browser = await chromium.launch({ headless: false });
const context = await browser.newContext({ recordHar: { path: "flow.har", content: "embed" } });
const page = await context.newPage();
await page.goto("https://app.example.test/login");
await page.getByLabel("Email").fill("test-user@example.test");
await page.getByLabel("Password").fill(process.env.TEST_PASSWORD);
await page.getByRole("button", { name: "Log in" }).click();
await page.getByRole("button", { name: "New document" }).click();
await context.close(); // flushes flow.har
await browser.close();
```

Keep the password in an environment variable (as above) so it is not written into the
script. Use `content: "omit"` to leave response bodies out of the file; Plover does not
import them anyway, and the file gets much smaller.

### By hand (Chrome DevTools)

1. Open DevTools → **Network**, tick **Preserve log**.
2. Do the flow in the page.
3. Right-click the request list → **Save all as HAR with content** (or the download
   icon in the Network toolbar).

Firefox and Safari have the same action in their network panels.

## 2. Import it

1. In Plover, open the collection and click **Import HAR** (the download icon next to
   **New request** above the sidebar).
2. Pick `flow.har`. Images, scripts, styles, fonts, media, source maps, `OPTIONS`
   preflights and identical repeats are hidden or collapsed; tick what you want.
3. Choose the folder and the environment for the secrets, and check the list of secret
   variables it will create (names only).
4. Click **Import**.

Every `Authorization` header (Bearer and Basic become the request's auth), `Cookie`,
token header (`X-Api-Key`, `X-CSRF-Token`, …), token query parameter (`access_token`,
`api_key`, …), JWT-looking value and credential body field (`password`,
`client_secret`, …) becomes a `{{variable}}` whose value is stored as a secret of the
chosen environment, in the keychain, never in the `.bru` files. XML and plain-text
bodies are imported as they are; rows whose body looks like it holds a token are
flagged so you can untick or edit them.

## 3. Clean up

- Check the new secret variables in the environment (**Edit** next to the environment).
- Delete `flow.har` (and the recording script's output folder, if any).
