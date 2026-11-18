export class HubError extends Error {
  constructor(status, code, url = null) {
    super(`${status} ${code}`);
    this.status = status;
    this.code = code;
    this.url = url;
  }
}

export const ACCOUNT_HEADER = "Kebbie-Account";

const named = (account) => (account ? { [ACCOUNT_HEADER]: account } : {});

async function call(method, path, { body, account = null } = {}) {
  const headers = named(account);
  if (body !== undefined) headers["Content-Type"] = "application/json";
  const response = await fetch(path, { method, headers, body: body === undefined ? undefined : JSON.stringify(body), credentials: "same-origin" });
  if (!response.ok) {
    const refusal = await response.json().catch(() => ({}));
    throw new HubError(response.status, refusal.error ?? "unknown", refusal.url ?? null);
  }
  return response.status === 204 ? null : response.json();
}

const query = (value) => encodeURIComponent(value ?? "");

async function bytes(path, account = null) {
  const response = await fetch(path, { headers: named(account), credentials: "same-origin" });
  if (!response.ok) throw new HubError(response.status, (await response.json().catch(() => ({}))).error ?? "unknown");
  return new Uint8Array(await response.arrayBuffer());
}

export const linkFor = (path, account) => (account ? `${path}${path.includes("?") ? "&" : "?"}account=${query(account)}` : path);

function accountCalls(id) {
  const on = (method, path, body) => call(method, path, { body, account: id });
  return {
    id,
    as: (other) => accountCalls(other),
    hub: () => call("GET", "/v1/hub"),
    me: () => on("GET", "/v1/a/account"),
    registerBegin: (ticket) => call("POST", "/v1/a/register/begin", { body: ticket }),
    registerFinish: (ceremony, credential, name) => call("POST", "/v1/a/register/finish", { body: { ceremony, credential, name } }),
    loginBegin: (request = {}) => call("POST", "/v1/a/login/begin", { body: request }),
    loginFinish: (ceremony, credential) => call("POST", "/v1/a/login/finish", { body: { ceremony, credential } }),
    logout: () => on("POST", "/v1/a/logout"),
    passkeys: () => on("GET", "/v1/a/passkeys"),
    passkeyBegin: () => on("POST", "/v1/a/passkeys/begin", {}),
    passkeyFinish: (ceremony, credential, name) => on("POST", "/v1/a/passkeys/finish", { ceremony, credential, name }),
    setKey: (setup) => on("POST", "/v1/a/key", setup),
    putWrapper: (passkey, wrapper) => on("PUT", `/v1/a/passkeys/${query(passkey)}/wrapper`, wrapper),
    boards: () => on("GET", "/v1/a/boards"),
    blob: (blob) => bytes(`/v1/a/blobs/${query(blob)}`, id),
    removeGitHub: () => on("DELETE", "/v1/a/github"),
    finishGitHub: () => call("POST", "/v1/a/github/finish", { body: {} }),
    email: () => on("GET", "/v1/a/email"),
    setEmail: (email) => on("PUT", "/v1/a/email", { email }),
    removeEmail: () => on("DELETE", "/v1/a/email"),
    askCode: (email, connect = null) => call("POST", "/v1/a/email/code", { body: connect ? { email, connect } : { email } }),
    enterCode: (email, code, connect = null) => call("POST", "/v1/a/email/verify", { body: connect ? { email, code, connect } : { email, code } }),
    confirmEmail: (code) => on("POST", "/v1/a/email/confirm", { code }),
    command: (envelope) => on("POST", "/v1/a/commands", { envelope }),
    results: (after, wait = 0) => on("GET", `/v1/a/results?after=${after}&wait=${wait}`),
    addDevice: (cert) => on("POST", "/v1/a/devices", { cert }),
    devices: () => on("GET", "/v1/a/devices"),
    removeDevice: (device) => on("DELETE", `/v1/a/devices/${query(device)}`),
    subscribePush: (device, subscription) => on("PUT", `/v1/a/devices/${query(device)}/push`, subscription),
    setCategories: (device, categories) => on("PUT", `/v1/a/devices/${query(device)}/categories`, { categories }),
    unsubscribePush: (device) => on("DELETE", `/v1/a/devices/${query(device)}/push`),
    startTrial: () => on("POST", "/v1/a/plan/trial", {}),
    deviceCommands: (device, before = null) => on("GET", `/v1/a/devices/${query(device)}/commands${before === null ? "" : `?before=${before}`}`),
  };
}

function connectCallsOf(id) {
  const on = (method, path, body) => call(method, path, { body, account: id });
  return {
    id,
    peek: (code) => call("GET", `/v1/connect?code=${query(code)}`),
    view: (code) => on("GET", `/v1/a/connect?code=${query(code)}`),
    approve: (approval) => on("POST", "/v1/a/connect/approve", approval),
    deny: (code, pro = false) => on("POST", "/v1/a/connect/deny", pro ? { code, pro } : { code }),
    watch: (pairing, seen, wait) => on("GET", `/v1/a/pairings/${query(pairing)}?seen=${seen}&wait=${wait}`),
    reveal: (pairing, key, nonce) => on("POST", `/v1/a/pairings/${query(pairing)}/reveal`, { key, nonce }),
    boards: () => on("GET", "/v1/a/boards"),
    putMachines: (signed) => on("PUT", "/v1/a/machines", signed),
    removeMachine: (machine) => on("DELETE", `/v1/a/machines/${query(machine)}`),
  };
}

// Every call under /v1/a/ names its account, so a browser holding several never acts on a guess.
export function scoped(id = null) {
  return { account: accountCalls(id), connectCalls: connectCallsOf(id), follow: (onEvent, signal) => follow(onEvent, signal, id) };
}

export const { account, connectCalls } = scoped(null);

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

export async function follow(onEvent, signal, id = null) {
  const headers = { Accept: "text/event-stream", ...named(id) };
  const response = await fetch("/v1/a/events", { headers, credentials: "same-origin", signal });
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
