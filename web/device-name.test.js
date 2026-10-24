import assert from "node:assert/strict";
import { test } from "node:test";

import { keepDeviceName, proposedName } from "./device-name.js";

const AGENTS = {
  "Safari on Mac": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.2 Safari/605.1.15",
  "Chrome on Mac": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
  iPhone: "Mozilla/5.0 (iPhone; CPU iPhone OS 18_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.2 Mobile/15E148 Safari/604.1",
  "Chrome on iPhone": "Mozilla/5.0 (iPhone; CPU iPhone OS 18_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/131.0.6778.73 Mobile/15E148 Safari/604.1",
  iPad: "Mozilla/5.0 (iPad; CPU OS 18_2 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.2 Mobile/15E148 Safari/604.1",
  "Chrome on Windows": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
  "Edge on Windows": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0",
  "Firefox on Linux": "Mozilla/5.0 (X11; Linux x86_64; rv:133.0) Gecko/20100101 Firefox/133.0",
  "Chrome on Android": "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36",
};

test("the proposed name says the browser and the system, and only the device on an iPhone's own Safari", () => {
  for (const [name, agent] of Object.entries(AGENTS)) assert.equal(proposedName(agent), name, agent);
  assert.equal(proposedName(""), "Browser");
});

test("a typed name reaches the hub only when it differs, and the device keeps the name the hub stored", async () => {
  const sent = [];
  const saved = [];
  const client = { rename: async (name) => (sent.push(name), { name }) };
  const device = { id: "dev_1", name: "Safari on Mac" };
  assert.equal(await keepDeviceName(client, device, " Safari on Mac ", async (kept) => saved.push(kept)), device);
  assert.equal(await keepDeviceName(client, device, "  ", async (kept) => saved.push(kept)), device);
  const kept = await keepDeviceName(client, device, " Work laptop ", async (value) => saved.push(value));
  assert.deepEqual(sent, ["Work laptop"]);
  assert.deepEqual(kept, { id: "dev_1", name: "Work laptop" });
  assert.deepEqual(saved, [kept]);
});
