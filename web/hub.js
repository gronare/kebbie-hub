export class HubError extends Error {
  constructor(status, code, url = null) {
    super(`${status} ${code}`);
    this.status = status;
    this.code = code;
    this.url = url;
  }
}

async function call(method, path, { body } = {}) {
  const headers = {};
  if (body !== undefined) headers["Content-Type"] = "application/json";
  const response = await fetch(path, { method, headers, body: body === undefined ? undefined : JSON.stringify(body), credentials: "same-origin" });
  if (!response.ok) {
    const refusal = await response.json().catch(() => ({}));
    throw new HubError(response.status, refusal.error ?? "unknown", refusal.url ?? null);
  }
  return response.status === 204 ? null : response.json();
}

const query = (value) => encodeURIComponent(value ?? "");

export const account = {
  hub: () => call("GET", "/v1/hub"),
  me: () => call("GET", "/v1/a/account"),
  registerBegin: (ticket) => call("POST", "/v1/a/register/begin", { body: ticket }),
  registerFinish: (ceremony, credential, name) => call("POST", "/v1/a/register/finish", { body: { ceremony, credential, name } }),
  loginBegin: (request = {}) => call("POST", "/v1/a/login/begin", { body: request }),
  loginFinish: (ceremony, credential) => call("POST", "/v1/a/login/finish", { body: { ceremony, credential } }),
  passkeyBegin: () => call("POST", "/v1/a/passkeys/begin", { body: {} }),
  passkeyFinish: (ceremony, credential, name) => call("POST", "/v1/a/passkeys/finish", { body: { ceremony, credential, name } }),
  setKey: (setup) => call("POST", "/v1/a/key", { body: setup }),
  putWrapper: (passkey, wrapper) => call("PUT", `/v1/a/passkeys/${query(passkey)}/wrapper`, { body: wrapper }),
  boards: () => call("GET", "/v1/a/boards"),
};

export const connectCalls = {
  peek: (code) => call("GET", `/v1/connect?code=${query(code)}`),
  view: (code) => call("GET", `/v1/a/connect?code=${query(code)}`),
  approve: (approval) => call("POST", "/v1/a/connect/approve", { body: approval }),
  deny: (code) => call("POST", "/v1/a/connect/deny", { body: { code } }),
  watch: (pairing, seen, wait) => call("GET", `/v1/a/pairings/${query(pairing)}?seen=${seen}&wait=${wait}`),
  reveal: (pairing, key, nonce) => call("POST", `/v1/a/pairings/${query(pairing)}/reveal`, { body: { key, nonce } }),
  boards: () => call("GET", "/v1/a/boards"),
  putMachines: (signed) => call("PUT", "/v1/a/machines", { body: signed }),
  removeMachine: (machine) => call("DELETE", `/v1/a/machines/${query(machine)}`),
};

export const admin = {
  overview: () => call("GET", "/v1/a/admin/overview"),
  accounts: () => call("GET", "/v1/a/admin/accounts"),
  machines: () => call("GET", "/v1/a/admin/machines"),
  reports: (quarantine) => call("GET", `/v1/a/admin/reports${quarantine ? "?quarantine=true" : ""}`),
  signals: () => call("GET", "/v1/a/admin/signals"),
  invite: () => call("POST", "/v1/a/admin/invites", { body: {} }),
  setActive: (id, active) => call("POST", `/v1/a/admin/accounts/${query(id)}/${active ? "activate" : "deactivate"}`, { body: {} }),
  removeMachine: (id) => call("DELETE", `/v1/a/admin/machines/${query(id)}`),
  decideKey: (key, decision) => call("POST", `/v1/a/admin/reports/keys/${query(key)}/${decision}`, { body: {} }),
  ack: (id) => call("POST", `/v1/a/admin/reports/${query(id)}/ack`, { body: {} }),
};

export function parseEvents(buffer) {
  const events = [];
  const blocks = buffer.split(/\r?\n\r?\n/);
  const rest = blocks.pop();
  for (const block of blocks) {
    const event = { name: "message", data: "", id: null };
    for (const line of block.split(/\r?\n/)) {
      if (line.startsWith(":")) continue;
      const colon = line.indexOf(":");
      const field = colon < 0 ? line : line.slice(0, colon);
      const value = colon < 0 ? "" : line.slice(colon + 1).replace(/^ /, "");
      if (field === "event") event.name = value;
      else if (field === "data") event.data += (event.data ? "\n" : "") + value;
      else if (field === "id") event.id = value;
    }
    if (event.data) events.push(event);
  }
  return { events, rest };
}

export async function follow(onEvent, signal) {
  const response = await fetch("/v1/a/events", { headers: { Accept: "text/event-stream" }, credentials: "same-origin", signal });
  if (!response.ok) throw new HubError(response.status, (await response.json().catch(() => ({}))).error ?? "unknown");
  const reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
  let buffer = "";
  for (;;) {
    const { value, done } = await reader.read();
    if (done) return;
    const parsed = parseEvents(buffer + value);
    buffer = parsed.rest;
    for (const event of parsed.events) onEvent(event.name, JSON.parse(event.data));
  }
}
