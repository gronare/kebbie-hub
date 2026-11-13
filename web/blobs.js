import { BACKLOG_BODY_MEDIA_TYPE, REVIEW_MEDIA_TYPE, checkBacklogBody, openBlob, readReview } from "../js/protocol.js";

export const kb = (bytes) => `${Math.max(1, Math.round(bytes / 1024))} kB`;

export const loadingLine = (kind, ref) => `Loading the ${kind} (${kb(ref.bytes)})…`;

export const failedLine = (kind) => `The ${kind} could not be opened here; it is still on the machine.`;

const esc = (value) => String(value ?? "").replace(/[&<>"']/g, (char) => `&#${char.charCodeAt(0)};`);

export const blobSlot = (ref, { kind, tag = "div", look, wait = "plan" }) =>
  `<${tag} class="${esc(wait)}" data-blob="${esc(ref.id)}" data-kind="${esc(kind)}" data-look="${esc(look)}" data-wait="${esc(wait)}"><div>${esc(loadingLine(kind, ref))}</div></${tag}>`;

export const planHref = (machine, address) => `#/plan/${encodeURIComponent(machine)}/${encodeURIComponent(address)}`;

export function blobReader(fetchBytes, open = openBlob) {
  const held = new Map();
  const loading = new Map();
  let generation = 0;
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
            if (!body && !review && !/^text\//.test(ref.media_type)) throw new Error("not text");
            const text = new TextDecoder("utf-8", { fatal: body || review }).decode(await open(ref, await fetchBytes(ref.id)));
            if (!body && !review) return { text };
            const parsed = JSON.parse(text);
            if (review) {
              return { review: readReview(parsed) };
            }
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
