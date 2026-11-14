import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { REVIEW_MEDIA_TYPE, fromB64 } from "../js/protocol.js";
import { blobReader, blobSlot, failedLine, loadingLine, planHref } from "./blobs.js";

const vectors = JSON.parse(readFileSync(new URL("../vectors/blobs.json", import.meta.url), "utf8"));
const [opens, tampered] = vectors.cases;

function hub(answers) {
  const asked = [];
  return {
    asked,
    fetchBytes: async (id) => {
      asked.push(id);
      const answer = answers[id];
      if (answer instanceof Error) throw answer;
      return fromB64(answer);
    },
  };
}

test("a plan is fetched once, decrypted with its ref's key and kept in memory", async () => {
  const fake = hub({ [opens.blob.id]: opens.ciphertext });
  const reader = blobReader(fake.fetchBytes);
  assert.equal(reader.peek(opens.blob.id), null);
  const [first, second] = await Promise.all([reader.load(opens.blob), reader.load(opens.blob)]);
  assert.equal(first.text, vectors.plaintext);
  assert.equal(second, first);
  assert.equal((await reader.load(opens.blob)).text, vectors.plaintext);
  assert.deepEqual(fake.asked, [opens.blob.id]);
  assert.equal(reader.peek(opens.blob.id).text, vectors.plaintext);
});

test("a blob that does not match its ref, does not arrive or is not text fails, and is tried again after a refresh", async () => {
  const fake = hub({ [tampered.blob.id]: tampered.ciphertext });
  const reader = blobReader(fake.fetchBytes);
  assert.deepEqual(await reader.load(tampered.blob), { failed: true });
  const missing = { ...opens.blob, id: `blb_${"0".repeat(64)}` };
  const gone = blobReader(async () => {
    throw new Error("404 not-found");
  });
  assert.deepEqual(await gone.load(missing), { failed: true });
  assert.deepEqual(await blobReader(hub({ [opens.blob.id]: opens.ciphertext }).fetchBytes).load({ ...opens.blob, media_type: "image/png" }), { failed: true });
  await reader.load(tampered.blob);
  assert.equal(fake.asked.length, 1);
  reader.retryFailed();
  assert.equal(reader.peek(tampered.blob.id), null);
  await reader.load(tampered.blob);
  assert.equal(fake.asked.length, 2);
});

test("locking forgets every decrypted text, including one still on its way", async () => {
  const fake = hub({ [opens.blob.id]: opens.ciphertext });
  const reader = blobReader(fake.fetchBytes);
  await reader.load(opens.blob);
  reader.forget();
  assert.equal(reader.peek(opens.blob.id), null);
  const pending = reader.load(opens.blob);
  reader.forget();
  assert.equal(await pending, null);
  assert.equal(reader.peek(opens.blob.id), null);
});

test("a slot says the plan is loading with its size, and a failure says it is still on the machine", () => {
  const html = blobSlot(opens.blob, { kind: "plan", look: "md md-peek" });
  assert.match(html, new RegExp(`^<div class="plan" data-blob="${opens.blob.id}" data-kind="plan" data-look="md md-peek" data-wait="plan"><div>Loading the plan \\(1 kB\\)…</div></div>$`));
  assert.equal(loadingLine("card", { bytes: 6 * 1024 }), "Loading the card (6 kB)…");
  assert.equal(failedLine("plan"), "The plan could not be opened here; it is still on the machine.");
  assert.match(blobSlot(opens.blob, { kind: "plan", tag: "article", look: "task md md-read", wait: "task plan" }), /^<article class="task plan" .*<\/article>$/);
  assert.equal(planHref("mch_a", "refunds@shop"), "#/plan/mch_a/refunds%40shop");
});

test("a backlog item's body opens as checked JSON, and a body that is not one fails", async () => {
  const backlog = JSON.parse(readFileSync(new URL("../vectors/backlog.json", import.meta.url), "utf8"));
  const reader = blobReader(hub({ [backlog.blob.id]: backlog.ciphertext }).fetchBytes);
  assert.deepEqual(await reader.load(backlog.blob), { body: backlog.body });
  const plan = blobReader(hub({ [opens.blob.id]: opens.ciphertext }).fetchBytes);
  assert.deepEqual(await plan.load({ ...opens.blob, media_type: backlog.media_type }), { failed: true });
});

test("a review opens as a checked review, and a review that fails the check does not", async () => {
  const review = JSON.parse(readFileSync(new URL("../vectors/review.json", import.meta.url), "utf8"));
  const reader = blobReader(hub({ [review.blob.id]: review.ciphertext }).fetchBytes);
  assert.deepEqual(await reader.load(review.blob), { review: review.review });
  const cards = JSON.parse(readFileSync(new URL("../vectors/cards.json", import.meta.url), "utf8"));
  const cardReader = blobReader(hub({ [cards.blob.id]: cards.ciphertext }).fetchBytes);
  assert.deepEqual(await cardReader.load(cards.blob), { card: cards.card });
  const markdown = blobReader(hub({ [cards.blob.id]: cards.ciphertext }).fetchBytes);
  assert.deepEqual(await markdown.load({ ...cards.blob, media_type: REVIEW_MEDIA_TYPE }), { failed: true }, "a card is not opened as a review");
  const backlog = JSON.parse(readFileSync(new URL("../vectors/backlog.json", import.meta.url), "utf8"));
  const wrong = blobReader(hub({ [backlog.blob.id]: backlog.ciphertext }).fetchBytes);
  assert.deepEqual(await wrong.load({ ...backlog.blob, media_type: review.media_type }), { failed: true });
});

test("a review of a step from a machine not yet updated opens as a subtask of one commit", async () => {
  const review = JSON.parse(readFileSync(new URL("../vectors/review.json", import.meta.url), "utf8"));
  const { body, opens_as } = review.reviews_from_a_machine_not_yet_updated[0];
  const plain = new TextEncoder().encode(JSON.stringify(body));
  const reader = blobReader(hub({ [review.blob.id]: review.ciphertext }).fetchBytes, async () => plain);
  const opened = await reader.load(review.blob);
  assert.deepEqual(opened, { review: opens_as });
  assert.equal(opened.review.subtask.commits.length, 1);
});
