import { HubError } from "./hub.js";
import { authScreen, disabled, esc, githubHref, heading } from "./screens.js";

export { authScreen, isAuthScreen } from "./screens.js";

const field = (label, input) => `<div class="note-field"><label for="${input.id}">${esc(label)}</label><input ${Object.entries(input).map(([name, value]) => `${name}="${esc(value)}"`).join(" ")}></div>`;

export const emailInput = (value = "") => field("Email", { id: "email", name: "email", type: "email", autocomplete: "email", inputmode: "email", maxlength: "254", placeholder: "you@example.com", value });

export const codeInput = (value = "") => field("6-digit code", { id: "code", name: "code", class: "code-entry", inputmode: "numeric", autocomplete: "one-time-code", pattern: "[0-9]*", maxlength: "6", placeholder: "······", value }) + `<div class="meta">The code works for 10 minutes.</div>`;

const or = `<div class="or"><span>or</span></div>`;
const signedLine = (text) => `<div class="center ok">✓ ${esc(text)}</div>`;
const leadWith = (title, lead) => `<h1 class="title">${esc(title)}</h1><p class="lead">${lead}</p>`;
const primary = (action, label, busy) => `<button type="button" class="btn primary" data-action="${action}"${disabled(busy)}>${esc(label)}</button>`;
const githubLink = (href) => `<a class="btn" href="${esc(href)}">Continue with GitHub</a>`;
const codeLinks = (busy) => `<p class="foot">Nothing arrived? <button type="button" class="link inline" data-action="resend-code"${disabled(busy)}>Send a new code</button></p><div class="links"><button type="button" class="link" data-action="other-email"${disabled(busy)}>Use another email</button></div>`;
const newHere = `<p class="foot">New to kebbie? Start in your terminal: <code>kebbie setup</code></p>`;

export function emailSignInScreen({ github = false, email = "", next = null, notice = null, busy = false }) {
  return authScreen({
    intro: heading("See your boards", `Your machines' boards, end-to-end encrypted. Sign in with your email${github ? " or GitHub" : ""}.`),
    fields: emailInput(email),
    actions: primary("send-code", "Send code", busy),
    alt: (github ? or + githubLink(githubHref({ next })) : "") + newHere,
    notice,
  });
}

export function passkeySignInScreen({ notice = null, busy = false }) {
  return authScreen({
    intro: heading("See your boards", "Your machines' boards, end-to-end encrypted."),
    actions: primary("sign-in", "Sign in with your passkey", busy) + `<p class="foot">Can't use your passkey? <button type="button" class="link inline" data-action="email-instead">Sign in with email instead</button></p>`,
    notice,
  });
}

const CODE_TEXT = {
  "sign-in": { title: "Check your email", button: "Sign in" },
  connect: { title: "Check your email", button: "Continue" },
  confirm: { title: "Confirm your email", button: "Confirm email" },
};

export function codeScreen({ purpose = "sign-in", email, code = "", name = "this machine", notice = null, busy = false }) {
  const text = CODE_TEXT[purpose] ?? CODE_TEXT["sign-in"];
  const waits = purpose === "connect" ? `<div class="center">${esc(name)} waits in its terminal. Next you create a passkey, then connect it.</div>` : "";
  return authScreen({
    intro: leadWith(text.title, `Enter the 6-digit code sent to <b>${esc(email)}</b>.`),
    fields: codeInput(code) + waits,
    actions: primary(purpose === "confirm" ? "confirm-email" : "enter-code", text.button, busy) + codeLinks(busy),
    notice,
  });
}

export function createPasskeyScreen({ who = null, notice = null, busy = false }) {
  return authScreen({
    intro: (who ? signedLine(`Signed in as ${who}`) : "") + heading("Create a passkey to unlock your boards", "Your boards are end-to-end encrypted, and only a passkey opens them. Create one on this device. It unlocks your boards wherever the passkey is saved."),
    actions: primary("add-passkey", "Create passkey", busy),
    notice,
  });
}

export function addEmailScreen({ email = "", notice = null, busy = false }) {
  return authScreen({
    intro: signedLine("Signed in with your passkey") + heading("Add your email", "Every kebbie account has an email. With it you can reach your account page when your passkey isn't at hand."),
    fields: emailInput(email),
    actions: primary("add-email", "Send code", busy),
    notice,
  });
}

export function connectEmailScreen({ name, code = "", github = false, email = "", notice = null, busy = false }) {
  const back = `/connect?code=${encodeURIComponent(code)}`;
  return authScreen({
    intro: heading(`Connect ${name} to kebbie hub`, `The hub shows ${name}'s board on your phone and in any browser, end-to-end encrypted. Start with your email${github ? " or GitHub" : ""}.`),
    fields: emailInput(email),
    actions: primary("send-code", "Send code", busy),
    alt: github ? or + githubLink(githubHref({ connect: code, next: back })) : "",
    notice,
  });
}

export function keylessUnlockScreen({ who = null, notice = null, busy = false }) {
  return authScreen({
    intro:
      (who ? signedLine(`Signed in as ${who}`) : "") +
      heading("Unlock your boards", "Email and GitHub open your account, not your boards. Your boards and answers open only with a passkey that holds your account key.") +
      `<section class="task"><div class="meta">Use the passkey saved on this device, or one on your phone: your browser shows a QR code to scan. There is no other way in, and nobody can reset it for you.</div></section>`,
    actions: primary("use-passkey", "Use a passkey", busy) + `<div class="links"><a class="link" href="/account">Go to your account</a></div>`,
    notice,
  });
}

// iOS shrinks only the visual viewport for the keyboard, so the form is lifted to sit above it while an auth screen shows.
export function keyboardLift(view = globalThis.visualViewport, page = globalThis.document?.documentElement) {
  let on = false;
  const sync = () => {
    const keyboard = innerHeight - view.height - view.offsetTop;
    page.style.setProperty("--vvh", `${view.height}px`);
    page.classList.toggle("kb-open", keyboard > 80);
  };
  return (shown) => {
    if (!view || !page || shown === on) return;
    on = shown;
    if (shown) {
      view.addEventListener("resize", sync);
      view.addEventListener("scroll", sync);
      sync();
    } else {
      view.removeEventListener("resize", sync);
      view.removeEventListener("scroll", sync);
      page.style.removeProperty("--vvh");
      page.classList.remove("kb-open");
    }
  };
}

export const signedInAs = (me, address) => address?.email ?? me?.github ?? null;

export const emailMissing = (address) => Boolean(address) && !address.verified;

const EMAIL_ERRORS = {
  "invalid-email": "That doesn't look like an email address.",
  "wrong-code": "That code doesn't match, or it has expired. Check the newest mail, or send a new code.",
  "email-taken": "Another kebbie account already uses that email.",
  "rate-limited": "Too many tries. Wait a while, then try again.",
  "signup-closed": "This hub makes new accounts by invite only.",
  "unknown-code": "This code has expired or was already used. Run kebbie hub connect again.",
  deactivated: "This account is deactivated.",
};

export function emailText(error) {
  if (error instanceof HubError) return EMAIL_ERRORS[error.code] ?? `The hub answered ${error.status} (${error.code}).`;
  return error?.message ?? String(error);
}

// Mirrors the hub's own check, so the code screen can open in the same tap; the hub still decides.
export function address(text) {
  const email = String(text ?? "").trim();
  const at = email.lastIndexOf("@");
  const domain = email.slice(at + 1);
  const valid = at > 0 && email.length <= 254 && domain.includes(".") && !domain.startsWith(".") && !domain.endsWith(".") && !/[\s<>,;"]/.test(email);
  return valid ? email.toLowerCase() : null;
}

export const typedCode = (value) => String(value ?? "").replace(/\D/g, "").slice(0, 6);

// Which sign-in screen a signed-out visitor sees; a hub that does not advertise email sign-in keeps its passkey screen.
export function signedOutStep({ info, remembered = null, step = "email", chosen = false }) {
  if (!info?.email_signin) return "passkey-only";
  if (step === "code") return "code";
  return remembered && !chosen ? "passkey" : "email";
}

// What a signed-in visitor must finish before the boards, on a hub that signs in by email; null means nothing.
export function signedInStep({ info, me, session = null, address = null, emailStep = null }) {
  if (!info?.email_signin || !me) return null;
  if (me.key_access === false) return me.key ? "unlock" : "create-passkey";
  if (session?.state !== "unlocked") return null;
  if (emailStep) return "confirm-email";
  return emailMissing(address) ? "add-email" : null;
}
