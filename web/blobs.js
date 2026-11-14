import { BACKLOG_BODY_MEDIA_TYPE, CARD_MEDIA_TYPE, LEAD_THREAD_MEDIA_TYPE, REVIEW_MEDIA_TYPE, checkBacklogBody, openBlob, readCard, readLeadThread, readReview } from "../js/protocol.js";

export const kb = (bytes) => `${Math.max(1, Math.round(bytes / 1024))} kB`;

export const loadingLine = (kind, ref) => `Loading the ${kind} (${kb(ref.bytes)})…`;

export const failedLine = (kind) => `The ${kind} could not be opened here; it is still on the machine.`;

const esc = (value) => String(value ?? "").replace(/[&<>"']/g, (char) => `&#${char.charCodeAt(0)};`);

export const blobSlot = (ref, { kind, tag = "div", look, wait = "plan" }) =>
  `<${tag} class="${esc(wait)}" data-blob="${esc(ref.id)}" data-kind="${esc(kind)}" data-look="${esc(look)}" data-wait="${esc(wait)}"><div>${esc(loadingLine(kind, ref))}</div></${tag}>`;

export const planHref = (machine, address) => `#/plan/${encodeURIComponent(machine)}/${encodeURIComponent(address)}`;

const hex = async (bytes) => [...new Uint8Array(await crypto.subtle.digest("SHA-256", bytes))].map((byte) => byte.toString(16).padStart(2, "0")).join("");

export function keptBytes(fetchBytes, kept, now = () => Math.floor(Date.now() / 1000)) {
  return async (id) => {
    const held = await kept.get(id).catch(() => null);
    if (held && `blb_${await hex(held)}` === id) return held;
    const bytes = await fetchBytes(id);
    await kept.put(id, bytes, now()).catch(() => undefined);
    return bytes;
  };
}

export function blobReader(fetchBytes, open = openBlob, { pause = 30_000, wait = (ms) => new Promise((done) => setTimeout(done, ms)) } = {}) {
  const held = new Map();
  const loading = new Map();
  let generation = 0;
  async function arrival(id, started) {
    for (;;) {
      try {
        return await fetchBytes(id);
      } catch (error) {
        if (error?.status !== 429 || started !== generation) throw error;
        await wait(pause);
      }
    }
  }
  return {
    peek: (id) => held.get(id) ?? null,
    load(ref) {
      if (held.has(ref.id)) return Promise.resolve(held.get(ref.id));
      if (!loading.has(ref.id)) {
        const started = generation;
        const done = (async () => {
          try {
            const body = ref.media_type === BACKLOG_BODY_MEDIA_TYPE;
            const review = ref.media_type === REVIEW_MEDIA_TYPE;
            const card = ref.media_type === CARD_MEDIA_TYPE;
            const lead = ref.media_type === LEAD_THREAD_MEDIA_TYPE;
            const data = body || review || card || lead;
            if (!data && !/^text\//.test(ref.media_type)) throw new Error("not text");
            const text = new TextDecoder("utf-8", { fatal: data }).decode(await open(ref, await arrival(ref.id, started)));
            if (!data) return { text };
            const parsed = JSON.parse(text);
            if (review) {
              return { review: readReview(parsed) };
            }
            if (card) return { card: readCard(parsed) };
            if (lead) return { lead: readLeadThread(parsed) };
            checkBacklogBody(parsed);
            return { body: parsed };
          } catch {
            return { failed: true };
          }
        })().then((entry) => {
          if (started !== generation) return null;
          loading.delete(ref.id);
          held.set(ref.id, entry);
          return entry;
        });
        loading.set(ref.id, done);
      }
      return loading.get(ref.id);
    },
    retryFailed() {
      for (const [id, entry] of held) if (entry.failed) held.delete(id);
    },
    forget() {
      generation += 1;
      held.clear();
      loading.clear();
    },
  };
}
