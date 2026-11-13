import assert from "node:assert/strict";
import { test } from "node:test";

import { isAuthScreen } from "./email.js";
import { cardAge, notificationsScreen, notifySection, rulesLines, acceptCount, accountScreen, deviceScreen, historyEntry, connectWelcome, EMAIL_LABEL, agentChip, awayLine, laterGroups, nextList, promptBlock, promptParts, reviewLines, routinesBlock, taskFacts, waited, addPasskeyScreen, cantUnlockScreen, connectDone, connectScreen, githubErrorText, machineAnswerNote, installHint, lastSeenText, machineDots, machineRow, nav, optionCards, signInScreen, signupScreen, setupScreen, sleepingRow, subtaskList, pushOrder, obstacleBlock, stepSummary, taskKey, taskRow, unlockingScreen, unlockScreen } from "./screens.js";

const text = (html) => html.replace(/<span[^>]*aria-hidden="true"[^>]*>[^<]*<\/span>/g, " ").replace(/<[^>]+>/g, " ").replace(/&#(\d+);/g, (_, code) => String.fromCharCode(Number(code))).replace(/\s+/g, " ").trim();
const buttons = (html) => [...html.matchAll(/<(button|a)\b([^>]*)>([^<]*)</g)].map(([, , attributes, label]) => ({ label: label.trim(), disabled: /\sdisabled\b/.test(attributes) }));
const OLD_COPY = /account name|Use an account name|Have an invite|Sign up|invite/i;

const view = { code: "Z78K-XF53", name: "mac", state: "pending" };
const fragment = { code: "secret", fingerprint: "print" };
const me = { account: "acc_1", handle: "k0123456789ab" };

test("the link opened in a browser that is not signed in asks to create a passkey for mac", () => {
  const html = connectScreen({ view, signupOpen: true });
  assert.equal(
    text(html),
    "kebbie hub Connect mac to kebbie hub The hub shows mac's board on your phone and in any browser, end-to-end encrypted. Create a passkey to start: it signs you in and unlocks your boards wherever it's saved. Create passkey Already have a kebbie account? Sign in",
  );
  assert.doesNotMatch(text(html), OLD_COPY);
  assert.doesNotMatch(html, /<input/);
});

test("Continue with GitHub shows only when the hub is configured for GitHub", () => {
  assert.doesNotMatch(text(connectScreen({ view, signupOpen: true, github: false })), /GitHub/);
  assert.match(text(connectScreen({ view, signupOpen: true, github: true })), /Create passkey or Continue with GitHub Already have a kebbie account\? Sign in/);
  assert.doesNotMatch(text(signInScreen({ github: false })), /GitHub/);
  assert.match(text(signInScreen({ github: true })), /Sign in with passkey Sign in with GitHub/);
});

test("an invite-only hub offers sign-in on the link instead of a new account", () => {
  const said = text(connectScreen({ view, signupOpen: false }));
  assert.match(said, /Connect mac to kebbie hub/);
  assert.match(said, /Sign in with passkey/);
  assert.doesNotMatch(said, /Create passkey/);
});

test("a browser signed in by its cookie skips straight to Connect mac?, with Connect pressable", () => {
  const html = connectScreen({ view, me, session: { state: "locked", me }, fragment });
  assert.equal(
    text(html),
    "kebbie hub Connect mac? Check that your terminal shows the same code. If it doesn't, cancel: another machine is asking. mac waiting in its terminal Z78K-XF53 Cancel Connect mac",
  );
  assert.deepEqual(buttons(html), [
    { label: "Cancel", disabled: false },
    { label: "Connect mac", disabled: false },
  ]);
  assert.doesNotMatch(text(html), /Create passkey|Sign in/);
});

test("Connect mac? has no checkbox for showing the board in this browser", () => {
  const html = connectScreen({ view, me, session: { state: "unlocked", me }, fragment });
  assert.doesNotMatch(html, /type="checkbox"|<input/);
  assert.doesNotMatch(text(html), /Also show its board/);
});

test("machines already on the account show in a small card", () => {
  const said = text(connectScreen({ view, me, session: { state: "unlocked", me }, fragment, machines: ["mac", "studio"] }));
  assert.match(said, /Already connected mac · studio Cancel Connect mac/);
  assert.doesNotMatch(text(connectScreen({ view, me, session: { state: "unlocked", me }, fragment, machines: [] })), /Already connected/);
});

test("an unlock that was cancelled on Connect leaves Connect pressable with one line saying so", () => {
  const html = connectScreen({ view, me, session: { state: "locked", me }, fragment, notice: "The passkey was cancelled, so mac isn't connected yet." });
  assert.match(text(html), /Cancel Connect mac The passkey was cancelled, so mac isn't connected yet\.$/);
  assert.deepEqual(buttons(html).at(-1), { label: "Connect mac", disabled: false });
});

test("a session that can't unlock keeps Connect disabled with one line saying why", () => {
  const html = connectScreen({ view, me, session: { state: "no-prf", me }, fragment });
  assert.match(text(html), /This browser's passkey can't unlock your account, so it can't hand mac your key\./);
  assert.deepEqual(buttons(html).at(-1), { label: "Connect mac", disabled: true });
  const unwrapped = connectScreen({ view, me, session: { state: "not-wrapped", me }, fragment });
  assert.match(text(unwrapped), /This passkey can't unlock your account yet, so it can't hand mac your key\./);
  assert.deepEqual(buttons(unwrapped).at(-1), { label: "Connect mac", disabled: true });
});

test("connecting ends with mac is connected and how to reach the board on the phone", () => {
  assert.equal(text(connectDone({ name: "mac", host: "hub.example.com" })), "✓ mac is connected Its board shows here now. On your phone, open hub.example.com and sign in with the same passkey.");
});

test("the hub opened without a link asks to sign in and sends newcomers to the terminal", () => {
  const said = text(signInScreen({ github: false }));
  assert.equal(said, "kebbie hub See your boards Your machines' boards, end-to-end encrypted. Sign in with the passkey you made for kebbie. Sign in with passkey New to kebbie? Start in your terminal: kebbie setup");
  assert.doesNotMatch(said, OLD_COPY);
  assert.equal(text(unlockingScreen()), "kebbie hub Unlocking your boards…");
});

test("a browser that can't unlock says so and what works today, promising no approval from a machine", () => {
  const said = text(cantUnlockScreen({ state: "no-prf" }));
  assert.equal(
    said,
    "kebbie hub This browser can't unlock your boards You're signed in, but this browser can't unlock your boards with its passkey. What works today: sign in on a device whose passkey can unlock them, such as an iPhone or Mac with iCloud Keychain, Google Password Manager or 1Password. Use a passkey instead",
  );
  assert.doesNotMatch(said, /approve|kebbie on|machine|\d{3}/i);
  assert.match(text(cantUnlockScreen({ state: "not-wrapped" })), /this passkey can't unlock your boards yet/);
});

test("the Home Screen hint promises no notifications", () => {
  assert.equal(text(installHint(true)), "Add kebbie to your Home Screen — Share → Add to Home Screen. ×");
  assert.doesNotMatch(text(installHint(true)), /notif/i);
  assert.equal(installHint(false), "");
});

test("a machine whose board expired shows as asleep with when it was last seen", () => {
  const now = new Date(2026, 9, 25, 12, 0).getTime() / 1000;
  const friday = new Date(2026, 9, 23, 17, 40).getTime() / 1000;
  const said = text(sleepingRow({ name: "studio", lastSeen: friday, working: null }, now));
  assert.equal(said, "studio asleep · last seen Fri 17:40");
  assert.doesNotMatch(said, /old|error|expired/i);
  assert.equal(lastSeenText(new Date(2026, 9, 25, 9, 5).getTime() / 1000, now), "09:05");
  assert.equal(lastSeenText(new Date(2026, 9, 10, 9, 5).getTime() / 1000, now), "10 Oct 09:05");
});

test("setup and invite pages ask only for a passkey, never an account name", () => {
  const setup = text(setupScreen({ phase: "start", needsSetup: true, token: "kho_x" }));
  assert.equal(setup, "kebbie hub Set up this hub Create a passkey to become this hub's owner. It signs you in; there is no password. Create passkey");
  assert.equal(text(setupScreen({ phase: "done" })), "kebbie hub Set up this hub You own this hub. Open kebbie");
  const invited = text(signupScreen({ invite: "K7QF-M2XD" }));
  assert.equal(invited, "kebbie hub Sign up You're invited to this hub. Create a passkey to start: it signs you in and unlocks your boards. Create passkey");
  const bare = text(signupScreen({}));
  assert.equal(bare, "kebbie hub Sign up New to kebbie? Start in your terminal: kebbie setup Sign in");
  for (const said of [setup, invited, bare]) assert.doesNotMatch(said, /account name|email/i);
});

test("every hub screen opens with the kebbie mark before kebbie hub", () => {
  const mark = /^(<div class="auth-intro">)?<header class="brand"><svg class="brand-mark" viewBox="0 0 32 32" aria-hidden="true">.*<\/svg><b>kebbie hub<\/b><\/header>/;
  const screens = [
    connectScreen({ view, signupOpen: true }),
    connectScreen({ view, me, session: { state: "locked", me }, fragment }),
    signInScreen({ github: false }),
    unlockingScreen(),
    cantUnlockScreen({ state: "no-prf" }),
    setupScreen({ phase: "start", needsSetup: true, token: "kho_x" }),
    signupScreen({}),
    addPasskeyScreen({ login: "octocat" }),
    accountScreen({ me: { ...me, github: "octocat" }, github: true }),
  ];
  for (const html of screens) assert.match(html, mark);
});

test("every screen before the boards uses the auth layout: intro on top, the art, the form low", () => {
  const github = { ...me, github: "octocat", key: null };
  const screens = {
    unlockScreen: unlockScreen(),
    unlockingScreen: unlockingScreen(),
    "cantUnlockScreen no-prf": cantUnlockScreen({ state: "no-prf" }),
    "cantUnlockScreen not-wrapped": cantUnlockScreen({ state: "not-wrapped" }),
    "cantUnlockScreen github": cantUnlockScreen({ state: "github" }),
    signInScreen: signInScreen({ github: true }),
    "connect welcome": connectScreen({ view, signupOpen: true, github: true }),
    "connect invite-only": connectScreen({ view, signupOpen: false }),
    "connect ask": connectScreen({ view, me, session: { state: "locked", me }, fragment }),
    "connect pairing": connectScreen({ view, me, phase: "pairing" }),
    "connect typed": connectScreen({ view, me, phase: "connected" }),
    "connect cancelled": connectScreen({ view, me, phase: "cancelled" }),
    "connect loading": connectScreen({}),
    "connect unsupported": connectScreen({ supported: false }),
    addPasskeyScreen: addPasskeyScreen({ login: "octocat" }),
    "connect after GitHub": connectScreen({ view, me: github, session: { state: "locked", me: github }, fragment }),
  };
  for (const [name, html] of Object.entries(screens)) {
    assert.ok(isAuthScreen(html), name);
    assert.match(html, /<div class="auth-intro">.*<\/div><div class="auth-art" aria-hidden="true">.*<div class="auth-form">.*<div class="auth-actions">/, name);
  }
});

const hrefs = (html) => [...html.matchAll(/<a\b[^>]*href="([^"]*)"[^>]*>([^<]*)</g)].map(([, href, label]) => ({ label: label.trim(), href: href.replace(/&#38;/g, "&") }));

test("the GitHub buttons start GitHub sign-in and come back to the page they left", () => {
  assert.deepEqual(hrefs(connectScreen({ view, signupOpen: true, github: true })), [{ label: "Continue with GitHub", href: "/auth/github?connect=Z78K-XF53&next=%2Fconnect%3Fcode%3DZ78K-XF53" }]);
  assert.deepEqual(hrefs(connectScreen({ view, signupOpen: false, github: true })), [{ label: "Sign in with GitHub", href: "/auth/github?next=%2Fconnect%3Fcode%3DZ78K-XF53" }]);
  assert.deepEqual(hrefs(signInScreen({ github: true })), [{ label: "Sign in with GitHub", href: "/auth/github" }]);
  assert.deepEqual(hrefs(signInScreen({ github: true, next: "/v1/x/page" })), [{ label: "Sign in with GitHub", href: "/auth/github?next=%2Fv1%2Fx%2Fpage" }]);
  assert.deepEqual(hrefs(signupScreen({ invite: "K7QF-M2XD", next: "/connect?code=Z78K-XF53", github: true })), [{ label: "Continue with GitHub", href: "/auth/github?invite=K7QF-M2XD&next=%2Fconnect%3Fcode%3DZ78K-XF53" }]);
  assert.deepEqual(hrefs(setupScreen({ phase: "start", needsSetup: true, token: "kho_x", github: true })), [{ label: "Continue with GitHub", href: "/auth/github?setup=kho_x" }]);
  assert.doesNotMatch(signupScreen({ invite: "K7QF-M2XD", github: false }) + setupScreen({ phase: "start", needsSetup: true, token: "kho_x" }), /GitHub/);
});

test("after GitHub a new account is asked to add a passkey, with Not now beside it", () => {
  const html = addPasskeyScreen({ login: "octocat" });
  assert.equal(
    text(html),
    "kebbie hub ✓ Signed in with GitHub as octocat Add a passkey to unlock your boards GitHub signs you in. A passkey also unlocks your boards on every phone and browser, so you never approve one by hand. Add passkey Not now",
  );
  assert.deepEqual(buttons(html), [
    { label: "Add passkey", disabled: false },
    { label: "Not now", disabled: false },
  ]);
  assert.deepEqual(buttons(addPasskeyScreen({ login: "octocat", busy: true }))[0], { label: "Add passkey", disabled: true });
  assert.match(addPasskeyScreen({ login: "<b>" }), /as &#60;b&#62;/);
});

test("the connect link after GitHub asks for a passkey first, and Not now says honestly that GitHub can't unlock", () => {
  const github = { ...me, github: "octocat", key: null };
  assert.match(text(connectScreen({ view, me: github, session: { state: "locked", me: github }, fragment })), /^kebbie hub ✓ Signed in with GitHub as octocat Add a passkey to unlock your boards/);
  const later = text(connectScreen({ view, me: github, session: { state: "github", me: github }, fragment }));
  assert.equal(later, text(cantUnlockScreen({ state: "github" })));
  assert.match(later, /This browser can't unlock your boards You're signed in with GitHub, but GitHub can't unlock your boards\. What works today: .* Use a passkey instead$/);
  const keyed = { ...github, key: { account: "acc_1", version: 1 } };
  assert.match(text(connectScreen({ view, me: keyed, session: { state: "locked", me: keyed }, fragment })), /^kebbie hub Connect mac\?/);
});

test("the account page connects GitHub or removes it", () => {
  const linked = accountScreen({ me: { ...me, github: "octocat" }, github: true });
  assert.equal(text(linked), "kebbie hub Account GitHub Signed in with GitHub as octocat Remove Back to your boards Sign out");
  assert.deepEqual(buttons(linked)[0], { label: "Remove", disabled: false });
  const unlinked = accountScreen({ me, github: true });
  assert.deepEqual(hrefs(unlinked)[0], { label: "Connect GitHub", href: "/auth/github?link=1&next=%2Faccount" });
  assert.doesNotMatch(text(accountScreen({ me, github: false })), /GitHub/);
});

test("every way GitHub sign-in comes back refused reads as one line", () => {
  assert.equal(githubErrorText(null), null);
  assert.equal(githubErrorText("no-account"), "No account on this hub uses this GitHub account. New to kebbie? Start in your terminal: kebbie setup");
  assert.equal(githubErrorText("github-taken"), "That GitHub account already belongs to another kebbie account.");
  assert.equal(githubErrorText("something-new"), githubErrorText("failed"));
  for (const code of ["expired", "denied", "signup-closed", "unauthorized", "signed-out", "deactivated"]) assert.ok(githubErrorText(code) && githubErrorText(code) !== githubErrorText("failed"), code);
});

const sha = (digit) => String(digit).repeat(40);
const payments = {
  address: "payments@api",
  group: "WORKING",
  summary: "2 commits",
  next_for_you: null,
  git: { upstream: "main", ahead: 2, behind: 0, uncommitted: 0, rules: { state: "ok" } },
  subtasks: [
    { id: "2", title: "Charge through a PaymentGateway", state: "ready", commits: [sha(2)] },
    { id: "3", title: "Add the new gateway", state: "in-progress", commits: [] },
    { id: "4", title: "Remove the old <payment> client", state: "planned", commits: [] },
  ],
  order: [{ sha: sha(2), subject: "Charge through a PaymentGateway", subtask: "2" }],
};
const rowOf = (item) => ({ machine: "mch_a", name: "devbox", item, task: "payments", repo: "api" });

const required = { ...payments, require_accept: true };

const subtask = (id, title, state, commits, extra = {}) => ({ id, title, state, commits: commits.map(sha), ...extra });
const row = (digit, subject, id) => ({ sha: sha(digit), subject, subtask: id });
const mindin = {
  ...required,
  address: "mindin@mindin",
  subtasks: [
    subtask("env", "Clean test environment", "accepted", [1, 2]),
    subtask("fm", "Frontmatter in the index", "accepted", [3], { added: { at: 1_761_069_120, reason: "The index has no frontmatter" } }),
    subtask("lint", "Lint from the index", "ready", [4, 6]),
    subtask("ci", "CI on Python 3.14", "accepted", [5]),
    subtask("split", "Split notes.py", "in-progress", [7]),
    subtask("wal", "SQLite in WAL mode", "planned", []),
  ],
  order: [row(1, "Give the tests their own vault", "env"), row(2, "Clean the vault between tests", "env"), row(3, "Keep frontmatter in the index", "fm"), row(4, "Read lint's fields from the index", "lint"), row(5, "Run CI on Python 3.14", "ci"), row(6, "Drop lint's fetch of every note", "lint"), row(7, "Move the backend factory", "split")],
  pushable: { count: 3, through: sha(3), subtasks: ["env", "fm"] },
};
mindin.order = mindin.order.map((one) => ({ ...one, accepted: mindin.subtasks.find((part) => part.id === one.subtask).state === "accepted" }));

test("a push order row says whether its tests passed, failed, run or were not run, and a commit moved up says why", () => {
  const order = mindin.order.map((one, index) => (index === 2 ? { ...one, tests: { state: "passed", at: 1_761_069_000 }, moved: { from: 4, reason: "Lint reads these fields" } } : index === 3 ? { ...one, tests: { state: "failed" } } : index === 4 ? { ...one, tests: { state: "running" } } : index === 5 ? { ...one, tests: { state: "not-run" } } : one));
  const html = pushOrder({ ...mindin, order });
  assert.match(text(html), /3 Keep frontmatter in the index Frontmatter in the index · tests ✓ moved up · Lint reads these fields 3333333/);
  assert.match(text(html), /4 Read lint's fields from the index Lint from the index · to review · tests ✕ 4444444/);
  assert.match(text(html), /5 Run CI on Python 3.14 CI on Python 3.14 · waits for no. 4 · tests running 5555555/);
  assert.match(text(html), /6 Drop lint's fetch of every note Lint from the index · to review · tests – 6666666/);
  assert.match(html, /<span class="tests t-failed" title="tests failed">tests ✕<\/span>/);
  assert.match(html, /<span class="moved">moved up · Lint reads these fields<\/span>/);
  assert.doesNotMatch(pushOrder(mindin), /class="tests|class="moved/, "a row without either says nothing of them");
});

test("commits that go out together sit in one bracket, and the run stops before a group it would split", () => {
  const order = mindin.order.map((one, index) => (index === 2 || index === 3 ? { ...one, group: "index" } : one));
  const html = pushOrder({ ...mindin, order, pushable: { count: 2, through: sha(2), subtasks: ["env"] } });
  assert.match(html, /<\/div><\/div><div class="together" role="group" aria-label="goes out together"><span class="together-h">goes out together<\/span><div class="step s-accepted">.*?3333333.*?<div class="step s-ready">.*?4444444<\/span><\/div><\/div><div class="step s-accepted">/);
  assert.match(text(html), /^Ready to push: 2 commits subtask Clean test environment /);
  assert.match(text(html), /3 Keep frontmatter in the index Frontmatter in the index · waits for no. 4 3333333/, "an accepted commit held by its group waits for the one not yet accepted");
  const inside = pushOrder({ ...mindin, order: mindin.order.map((one, index) => (index < 2 ? { ...one, group: "env" } : one)) });
  assert.match(inside, /^<div class="steps go order"><div class="run"><div class="run-h">.*?<\/div><div class="together" role="group"/, "a group inside the run sits inside its box");
});

test("a subtask with some of its commits accepted says how many, on the card and in the push order", () => {
  const order = mindin.order.map((one) => (one.sha === sha(4) ? { ...one, accepted: true } : one));
  const partly = { ...mindin, order, pushable: { count: 5, through: sha(5), subtasks: ["env", "fm", "lint", "ci"] } };
  assert.match(text(subtaskList(partly)), /● Lint from the index 1 of 2 commits accepted · to review/);
  assert.match(text(pushOrder(partly)), /✓ 4 Read lint's fields from the index Lint from the index · accepted 4444444/);
  assert.match(text(pushOrder(partly)), /● 6 Drop lint's fetch of every note Lint from the index · to review 6666666/);
  assert.match(text(pushOrder(partly)), /^Ready to push: 5 commits subtasks Clean test environment, Frontmatter in the index, Lint from the index, CI on Python 3.14 /);
  const building = { ...partly, subtasks: partly.subtasks.map((one) => (one.id === "lint" ? { ...one, state: "in-progress" } : one)) };
  assert.match(text(subtaskList(building)), /◐ Lint from the index in progress · 1 of 2 commits accepted/);
  assert.match(text(subtaskList(mindin)), /● Lint from the index 2 commits · to review/);
});

test("a board row says in one line how far the subtasks have come and what is yours to do, never the whole list", () => {
  const html = taskRow(rowOf(required));
  assert.equal(text(html), "payments devbox api · 2 commits ◆ 1 of 3 subtasks to review · 1 in progress you: review Charge through a PaymentGateway on devbox");
  assert.match(html, /<div class="step-line"><span class="accept">◆ 1 of 3 subtasks to review<\/span> · <span class="prog">1 in progress<\/span><\/div>/);
  assert.doesNotMatch(html, /class="step |<payment>|to accept/, "the board row lists no subtasks");
  assert.equal(text(stepSummary(mindin)), "✓ 3 of 6 subtasks accepted · ◆ 1 to review · 1 in progress");
  assert.match(text(taskRow(rowOf(mindin))), /you: push 3 commits or review Lint from the index on devbox$/);
  const done = { ...mindin, subtasks: mindin.subtasks.map((one) => ({ ...one, state: one.id === "env" ? "pushed" : "accepted", commits: one.commits.length ? one.commits : [sha(9)] })), order: mindin.order.slice(2).map((one) => (one.subtask === "split" ? { ...one, subtask: "split" } : one)), pushable: undefined };
  done.order = [row(3, "Keep frontmatter in the index", "fm")];
  assert.equal(text(stepSummary(done)), "✓ 6 of 6 subtasks accepted · ↑ 7 commits pushed");
  const asks = { ...required, obstacle: { id: "o1", state: "asked", text: "x", at: 1, decision: { kind: "ask", by: "investigator", text: "Yours." } } };
  assert.equal(text(stepSummary(asks)), "◆ 1 of 3 subtasks to review · 1 in progress · obstacle: the investigator asks you");
  assert.equal(text(stepSummary({ ...asks, obstacle: { ...asks.obstacle, state: "decided", decision: { kind: "subtask", by: "investigator", text: "x" } } })), "◆ 1 of 3 subtasks to review · 1 in progress", "a decided obstacle asks nothing on the board");
});

test("what kebbie asks of you comes before a subtask to review", () => {
  assert.equal(text(taskRow(rowOf({ ...required, next_for_you: "answer: Keep it?" }))).endsWith("you: answer: Keep it?"), true);
  assert.doesNotMatch(text(taskRow(rowOf({ ...payments, subtasks: payments.subtasks.map((one) => ({ ...one, state: one.state === "ready" ? "in-progress" : one.state })) }))), /you:/);
});

test("the task card lists the subtasks without numbers as the way in, each saying where it stands, a late one with why it was added", () => {
  const html = subtaskList(mindin, { href: (id) => `#/review/m/a/${id}`, selected: "lint" });
  assert.equal(text(html), "✓ Clean test environment accepted · ready to push ✓ Frontmatter in the index accepted · ready to push added " + text(lastSeenText(1_761_069_120)) + " · The index has no frontmatter ● Lint from the index 2 commits · to review ✓ CI on Python 3.14 accepted · waits for Lint from the index ◐ Split notes.py in progress · 1 commit ○ SQLite in WAL mode");
  assert.match(html, /<a class="step s-ready on" href="#\/review\/m\/a\/lint" aria-current="true">.*<svg class="chev"/);
  assert.match(html, /<a class="step s-accepted" href="#\/review\/m\/a\/ci">/);
  assert.match(html, /<div class="step s-in-progress">/);
  assert.match(html, /<span class="label">accepted <span class="dim">· waits for Lint from the index<\/span><\/span>/);
  assert.match(html, /<span class="added">added .* · The index has no frontmatter<\/span>/);
  assert.doesNotMatch(html, /class="number"/, "subtasks carry no numbers: the order lives in the commits");
  assert.doesNotMatch(subtaskList(required), /<a /, "without a link the rows are text");
  assert.doesNotMatch(subtaskList(payments), /<payment>/);
});

test("the push order lists the commits top first, each under its subtask, with the run that can go out now in its own box", () => {
  const html = pushOrder(mindin);
  assert.match(html, /^<div class="steps go order"><div class="run"><div class="run-h"><b>Ready to push: 3 commits<\/b><span>subtasks Clean test environment, Frontmatter in the index<\/span><\/div>(<div class="step s-accepted">.*?<\/div>){0,1}/);
  const run = html.slice(0, html.indexOf("</div><div class=\"step s-ready\""));
  assert.equal((run.match(/class="step /g) ?? []).length, 3);
  assert.match(text(html), /4 Read lint's fields from the index Lint from the index · to review 4444444 ✓ 5 Run CI on Python 3.14 CI on Python 3.14 · waits for no. 4 5555555/);
  assert.match(text(html), /7 Move the backend factory Split notes.py · in progress 7777777$/);
  assert.equal(pushOrder({ ...mindin, pushable: undefined }).includes('class="run"'), false, "nothing pushable, no box");
  assert.equal(pushOrder(payments.subtasks ? { ...payments, order: [] } : payments), "");
  assert.equal(pushOrder({ ...payments, subtasks: undefined }), "");
});

test("an obstacle shows what stands in the way and who raised it, then the ruling and the subtask it added", () => {
  const at = 1_761_068_460;
  const base = { ...mindin, subtasks: mindin.subtasks.map((one) => (one.id === "fm" ? { ...one, added: { at: at + 600, reason: "The index has no frontmatter" } } : one)) };
  const raised = obstacleBlock({ ...base, obstacle: { id: "o1", state: "investigating", text: "Lint should read <the> fields.", at } }, at + 60);
  assert.ok(text(raised).startsWith(`Obstacle Lint should read <the> fields. raised by the task · ${lastSeenText(at, at + 60)}`));
  assert.match(text(raised), /The investigator is looking into it The task waits for its ruling\.$/);
  assert.doesNotMatch(raised, /<the>/);
  const decided = obstacleBlock({ ...base, obstacle: { id: "o1", state: "decided", text: "x", at, decision: { kind: "subtask", by: "investigator", text: "Frontmatter belongs in the index." } } }, at);
  assert.match(decided, /<div class="sent done" role="status"><b>Investigator: add a preparatory subtask first<\/b><span>Frontmatter belongs in the index.<\/span><\/div><div class="change"><span class="mark">\+<\/span><span>Subtask added: <b>Frontmatter in the index<\/b><\/span><\/div>/);
  const asked = obstacleBlock({ ...base, obstacle: { id: "o1", state: "asked", text: "x", at, decision: { kind: "ask", by: "investigator", text: "Both hold." } } }, at);
  assert.match(asked, /<div class="sent wait" role="status"><b>Investigator: asks you<\/b>/);
  assert.doesNotMatch(asked, /Subtask added/);
  assert.match(obstacleBlock({ ...base, obstacle: { id: "o1", state: "with-lead", text: "x", at } }, at), /<b>With the lead<\/b>/);
  assert.equal(obstacleBlock(base), "");
});

test("a task without subtasks shows its row as before, and the board's count only when something waits", () => {
  const plain = { ...payments, subtasks: undefined, order: undefined };
  assert.equal(text(taskRow(rowOf(plain))), "payments devbox api · 2 commits");
  assert.equal(subtaskList(plain), "");
  assert.equal(stepSummary(plain), "");
  assert.equal(subtaskList({ ...payments, subtasks: [] }), "");
  assert.equal(text(acceptCount(3)), "◆ 3 to accept");
  assert.equal(acceptCount(0), "");
  assert.match(text(subtaskList(payments)), /accepting each subtask is optional: push takes what is committed$/);
  assert.doesNotMatch(text(subtaskList(required)), /optional/);
});

test("after the two nearest planned subtasks the rest fold into one line that opens them", () => {
  const items = [subtask("one", "One", "ready", [1]), ...[2, 3, 4, 5, 6].map((number) => subtask(`s${number}`, `Subtask ${number}`, "planned", []))];
  const folded = subtaskList({ ...required, subtasks: items });
  assert.equal(text(folded), "● One 1 commit · to review ○ Subtask 2 ○ Subtask 3 3 more planned");
  assert.match(folded, /<button type="button" class="more" data-action="steps-more" aria-expanded="false">3 more planned<svg/);
  const open = subtaskList({ ...required, subtasks: items }, { more: true });
  assert.match(text(open), /○ Subtask 6 Fewer planned$/);
});

test("a question's options show as cards with only the recommended one marked, and answering points to the machine", () => {
  const options = [{ label: "Keep it", meaning: "deprecate now" }, { label: "Remove <it>", meaning: "drop it in this task" }];
  const html = optionCards(options, 2);
  assert.equal(text(html), "Keep it deprecate now Recommended Remove <it> drop it in this task");
  assert.equal(html.match(/class="opt rec"/g).length, 1);
  assert.match(html, /class="opt rec"><span class="tag">Recommended<\/span><b>Remove &#60;it&#62;/);
  assert.doesNotMatch(optionCards(options, null), /opt rec|Recommended/);
  assert.doesNotMatch(html, /<button/, "nothing to press until answering ships");
  assert.equal(text(machineAnswerNote()), "Answer at the machine: N on the board, or ask the lead.");
});

test("the nav names the tabs with the backlog between board and questions, counts waiting questions in a badge, marks the open tab and links the account", () => {
  const html = nav({ active: "questions", questions: 2 });
  assert.deepEqual(hrefs(html), [{ label: "Board", href: "/#/board" }, { label: "Backlog", href: "/#/backlog" }, { label: "Questions", href: "/#/questions" }, { label: "Machines", href: "/#/machines" }, { label: "Account", href: "/account" }]);
  assert.match(html, /class="on" aria-current="page">Questions <em class="badge">2<\/em>/);
  assert.doesNotMatch(nav({ active: "board" }), /badge|Admin/);
  assert.match(nav({ active: "account", deskOnly: true }), /^<nav class="tabbar desk-nav">/);
});

test("the selected task is outlined on the board, and only that one", () => {
  const selected = taskKey("mch_a", "payments@api");
  assert.match(taskRow(rowOf(payments), selected), /^<a class="task sel" href="#\/task\/mch_a\/payments%40api" aria-current="true">/);
  assert.match(taskRow(rowOf(payments), taskKey("mch_b", "payments@api")), /^<a class="task" href=/);
});

test("a machine reads awake or asleep beside its dot, and the board's header lists every machine", () => {
  assert.equal(text(machineRow({ machine: "mch_a", name: "mac", awake: true, facts: ["kebbie 0.24.0"] })), "mac awake · kebbie 0.24.0 Remove");
  assert.match(machineRow({ machine: "mch_a", name: "studio", awake: false, facts: [] }), /class="mrow asleep"><span class="dot"/);
  assert.equal(text(machineDots([{ name: "mac", awake: true, lastSeen: "12:00" }, { name: "studio", awake: false, lastSeen: "Fri 17:40" }])), "mac studio · last seen Fri 17:40");
});

const NOW = 1_761_070_000;
const nextRow = (rank, part, address, line, since, item = null, waits = 0) => ({ rank, part, address, line, since, waits_behind: waits, machine: "mch_a", name: "mac", item, task: address.split("@")[0], repo: address.split("@")[1] });
const codexTask = { ...payments, address: "hub@kebbie", group: "NEEDS YOU", title: "Clarify machine removal", agent: { status: "blocked", kind: "codex", model: "gpt-6.1-sol" }, prompt: { id: "3f2a9c1d", question: "Would you like to run the following command?", options: [] }, subtasks: undefined };

test("Next lists what needs you in kebbie's order under its parts, with each task's title, agent, line and wait, and the counts below", () => {
  const rows = [nextRow(1, "stopped", "hub@kebbie", "answer Codex's prompt", NOW - 12 * 60, codexTask), nextRow(2, "push", "card@web", "2 commits · 2 tasks wait on it", NOW - 3600, { ...payments, title: "Card component" }, 2), nextRow(3, "read", "research@kebbie", "result ready · stay on the TUI", null)];
  const html = nextList([{ machine: "mch_a", name: "mac", rows, counts: { working: 4, queued: 1, parked: 1 } }], NOW);
  assert.equal(text(html), "Stopped on you 1 Clarify machine removal codex answer Codex's prompt 12 min Push, in this order 2 Card component 2 commits · 2 tasks wait on it 1 h To read 3 research result ready · stay on the TUI 4 working · 1 queued · 1 parked · same order as N on the board");
  assert.match(html, /<a class="nrow" href="#\/task\/mch_a\/hub%40kebbie">/);
  assert.match(html, /<span class="chip codex">codex<\/span>/);
});

test("an empty Next says nothing needs you, several machines get a list each under their name, and a board without Next shows none", () => {
  assert.equal(text(nextList([{ machine: "mch_a", name: "mac", rows: [], counts: { working: 2, queued: 0, parked: 0 } }], NOW)), "Nothing needs you. 2 working");
  const two = nextList([{ machine: "mch_a", name: "mac", rows: [nextRow(1, "push", "a@web", "1 commit", NOW)], counts: null }, { machine: "mch_b", name: "studio", rows: [], counts: null }], NOW);
  assert.equal(text(two), "mac Push, in this order 1 a 1 commit now same order as N on the board studio Nothing needs you.");
  assert.equal(nextList([], NOW), "");
  assert.deepEqual([waited(30), waited(42 * 60), waited(3600), waited(3 * 86_400)], ["now", "42 min", "1 h", "3 d"]);
});

test("a row shows the task's title, its agent and model, its recipe and the review assessment", () => {
  const item = { ...payments, subtasks: undefined, title: "Round VAT per invoice", agent: { status: "working", kind: "claude", model: "opus" }, recipe: "small fix", assessment: "review suggested: touches money" };
  assert.equal(text(taskRow(rowOf(item))), "Round VAT per invoice devbox api · claude · opus · recipe: small fix · review suggested: touches money · 2 commits");
  assert.equal(text(taskRow(rowOf({ ...payments, subtasks: undefined }))), "payments devbox api · 2 commits", "a board from an older kebbie names the task as before");
  assert.equal(agentChip({ status: "idle" }), "", "an agent without its kind shows no chip");
});

test("the task card names its agent, profile and recipe, and its review section says what holds the push", () => {
  const item = { ...payments, agent: { status: "working", kind: "claude", model: "opus" }, profile: "task", recipe: "small fix", assessment: "review suggested: touches money and its tests", verdict: { verdict: "ok", findings: 0, sha: "abc", stale: false, running: false, held: true }, second_opinion: { by: "codex-review", points: 2, running: false } };
  assert.equal(text(taskFacts(item)), "claude · opus · profile task · recipe: small fix");
  assert.equal(text(reviewLines(item)), "Review review suggested: touches money and its tests reviewer ✓ ok · second opinion codex-review: 2 points the push waits for the reviewer's pass; the second opinion never holds it");
  const changes = { ...item, second_opinion: undefined, verdict: { ...item.verdict, verdict: "changes", findings: 1, held: false } };
  assert.equal(text(reviewLines(changes)), "Review review suggested: touches money and its tests reviewer: 1 finding the push does not wait for the reviewer");
  assert.equal(text(reviewLines({ ...payments, verdict: { verdict: null, findings: null, sha: null, stale: false, running: true } })), "Review reviewer running", "an older kebbie's verdict says nothing about holding");
  assert.equal(reviewLines(payments), "");
  assert.equal(taskFacts(payments), "");
});

test("an agent's own prompt shows its question, the command apart, the reason, and each option with its key", () => {
  const question = "Would you like to run the following command?\nEnvironment: local\nReason: May I record that this task is waiting on the lead?\n$ kebbie report --state blocked --next nothing 'Read-only\ninspection complete.'";
  assert.deepEqual(promptParts(question), { question: "Would you like to run the following command?", command: "kebbie report --state blocked --next nothing 'Read-only inspection complete.'", details: ["Environment: local", "May I record that this task is waiting on the lead?"] });
  const options = [{ label: "Yes, proceed (y)", takes_text: false, key: "y" }, { label: "No, and tell Codex what to do differently (esc)", takes_text: false, key: "esc" }, { label: "Type something", takes_text: true }];
  const html = promptBlock({ item: { ...codexTask, prompt: { id: "3f2a9c1d", question, options } }, repo: "kebbie" });
  assert.equal(text(html), "Would you like to run the following command? kebbie · codex Environment: local May I record that this task is waiting on the lead? kebbie report --state blocked --next nothing 'Read-only inspection complete.' Yes, proceed (y) No, and tell Codex what to do differently (esc) Type something with text Answer at the machine: N on the board, or ask the lead.");
  assert.match(html, /<div class="cmd">kebbie report/);
  assert.doesNotMatch(html, /<button/);
  const claude = promptParts("Bash command\ncp ../web/logo.png Assets/\nCopy the logo\nDo you want to proceed?");
  assert.deepEqual(claude, { question: "Do you want to proceed?", command: "", details: ["Bash command", "cp ../web/logo.png Assets/", "Copy the logo"] });
});

test("queued and parked tasks list under their own groups with their line, and a parked one says when a question waits", () => {
  const groups = [
    { key: "queued", title: "Queued", rows: [{ machine: "mch_a", name: "mac", task: "mail-dns", repo: "shop", later: { address: "mail-dns@shop", title: "Mail DNS records", state: "queued", line: "waits for launch", until: null, question: false } }] },
    { key: "parked", title: "Parked", rows: [{ machine: "mch_a", name: "mac", task: "editor", repo: "app", later: { address: "editor@app", title: null, state: "parked", line: "nothing yet", until: NOW + 3600, question: true } }] },
  ];
  const said = text(laterGroups(groups, NOW));
  assert.match(said, /^Queued · 1 Mail DNS records mac shop · waits for launch Parked · 1 editor mac app · nothing yet · until \d\d:\d\d · a question waits$/);
  assert.equal(laterGroups([], NOW), "");
});

test("routines list with their next run and last outcome, and away mode shows as a line", () => {
  const said = text(routinesBlock([{ machine: "mch_a", name: "mac", routine: { id: "r1", title: "Nightly deps", repo: "shop", mode: "fix", next_run: NOW + 600, last_outcome: "ok" } }, { machine: "mch_a", name: "mac", routine: { id: "r2", title: "Weekly lint", repo: "web", mode: "report", next_run: null, last_outcome: null } }], NOW));
  assert.match(said, /^Routines · 2 Nightly deps · shop · next \d\d:\d\d · last run ok · mac Weekly lint · web · not scheduled · mac$/);
  assert.equal(routinesBlock([], NOW), "");
  assert.match(text(awayLine([{ machine: "mch_a", name: "mac", since: NOW - 60 }], NOW)), /^mac is away since \d\d:\d\d$/);
  assert.equal(awayLine([], NOW), "");
});

test("a signed-in visitor whose boards are locked is told they are signed in and asked to unlock", () => {
  const locked = text(unlockScreen());
  assert.match(locked, /Unlock your boards You're signed in\. Unlock with your passkey to read your boards\. They stay unlocked on this device for an hour\. Unlock with passkey/);
  assert.doesNotMatch(locked, /Sign in/);
  const signedOut = text(signInScreen({ github: false }));
  assert.doesNotMatch(signedOut, /You're signed in|Unlock/);
  assert.match(signedOut, /See your boards .* Sign in with passkey/);
});

test("the account page signs out from a plain button, held while busy", () => {
  assert.match(accountScreen({ me }), /<button type="button" class="btn" data-action="sign-out">Sign out<\/button>/);
  assert.match(accountScreen({ me, busy: true }), /data-action="sign-out" disabled>Sign out</);
});

test("the account page keeps the account id out of sight", () => {
  assert.doesNotMatch(accountScreen({ me, github: true, email: true, codes: true, address: { email: "ada@example.com", verified: true } }), /acc_1|copy/i);
});

test("a hub that does not keep email never asks for one", () => {
  const pages = [
    signupScreen({ invite: "K7QF-M2XD", github: true }),
    connectWelcome({ name: "mac", code: "Z78K-XF53", signupOpen: true, github: true }),
    connectScreen({ view, signupOpen: true }),
    setupScreen({ phase: "start", needsSetup: true, token: "kho_x" }),
    accountScreen({ me, github: true }),
  ];
  for (const page of pages) assert.doesNotMatch(page, /email/i);
});

test("a hub that keeps email asks for it at passkey sign-up and shows its state on the account page", () => {
  assert.match(signupScreen({ invite: "K7QF-M2XD", email: true }), /type="email"/);
  assert.match(connectScreen({ view, signupOpen: true, email: true }), /type="email"/);
  assert.match(text(signupScreen({ invite: "K7QF-M2XD", email: true })), new RegExp(EMAIL_LABEL.replace(/[()]/g, "\\$&")));
  assert.match(text(accountScreen({ me, github: false, email: true })), /Email No email yet .* Save email/);
  const ada = { email: "ada@example.com", verified: true };
  assert.equal(text(accountScreen({ me, github: false, email: true, address: ada })), "kebbie hub Account Email ada@example.com · confirmed Change Back to your boards Sign out");
  assert.doesNotMatch(accountScreen({ me, github: false, email: true, address: ada }), /type="email"/);
  assert.match(text(accountScreen({ me, github: false, email: true, changing: true, address: ada })), /Email ada@example\.com · confirmed New email Save email Remove email Cancel/);
  assert.match(text(accountScreen({ me, github: false, email: true, address: { email: "ada@example.com", verified: false, sent: true } })), /not confirmed yet\. We sent a link/);
  assert.match(accountScreen({ me, github: false, email: true, address: { email: "a\"<b>@example.com", verified: false } }), /a&#34;&#60;b&#62;@example\.com · not confirmed/);
});

test("on a hub that signs in by email the account page shows the address and opens a field only on Change", () => {
  const ada = { email: "ada@example.com", verified: true };
  const closed = accountScreen({ me, github: false, email: true, codes: true, address: ada });
  assert.equal(text(closed), "kebbie hub Account Email ada@example.com · confirmed Change Back to your boards Sign out");
  assert.doesNotMatch(closed, /type="email"|Send code/);
  const open = accountScreen({ me, github: false, email: true, codes: true, changing: true, address: ada });
  assert.match(text(open), /Email ada@example\.com · confirmed New email Send code Cancel Back to your boards Sign out$/);
  assert.match(open, /<input id="email"[^>]* value="">/);
});

test("the account page shows this device's switch only when the hub takes commands, off as view only", () => {
  const me = { account: "acc_1", handle: "k1", github: null };
  assert.doesNotMatch(accountScreen({ me }), /This device|role="switch"/);
  const off = accountScreen({ me, allowed: false });
  assert.match(off, /<h2 class="group g-muted">This device<\/h2>/);
  assert.match(off, /Allow changes from this device<\/div><div class="meta" id="allow-state">View only<\/div>/);
  assert.match(off, /<input type="checkbox" role="switch" class="switch" name="allow-changes"[^>]*aria-checked="false">/);
  const on = accountScreen({ me, allowed: true });
  assert.match(on, /<div class="meta" id="allow-state">Changes go to each machine that takes them\.<\/div>/);
  assert.match(on, /aria-checked="true" checked>/);
});

test("a machine row says what the machine takes from the hub", async () => {
  const { machinePolicy } = await import("./decide.js");
  const row = (accepts) => machineRow({ machine: "mch_1", name: "devbox", awake: true, facts: [machinePolicy(accepts), "kebbie 0.48.0"] });
  assert.match(row(["answer", "push", "prompt-answer"]), /awake · takes answers and pushes · kebbie 0\.48\.0/);
  assert.match(row(["answer", "prompt-answer"]), /awake · takes answers, not pushes · kebbie/);
  assert.match(row([]), /awake · takes nothing from the hub · kebbie/);
});

const AT_TWO_PM = Math.floor(new Date(2026, 10, 11, 14, 9).getTime() / 1000);
const devices = [
  { id: "dev_01JB8Q6Z3V9K2M4N5P6R7S8T9V", name: "iPhone 16", kind: "web", self: true, added: AT_TWO_PM - 660, seen: AT_TWO_PM - 5, key: "7F3A-91C2", standing: [{ machine: "MacBook", state: "approved", at: AT_TWO_PM - 30 }, { machine: "work-mac", state: "waiting", until: AT_TWO_PM + 21 * 3600 + 60 }] },
  { id: "dev_01JB8Q6Z3V9K2M4N5P6R7S8T9W", name: "Chrome on Mac", kind: "web", added: AT_TWO_PM - 44 * 86_400, seen: null, key: "91AB-44F0", standing: [{ machine: "MacBook", state: "denied", at: AT_TWO_PM - 40 * 86_400 }] },
];

test("Account lists the account's devices under This device with where each one stands on each machine", () => {
  const html = accountScreen({ me: { account: "acc_x" }, github: false, allowed: true, devices, now: AT_TWO_PM });
  const shown = text(html);
  assert.ok(shown.indexOf("This device") < shown.indexOf("Devices · 2"), shown);
  assert.match(shown, /iPhone 16 this device web app · added today · last seen now ✓ Approved on MacBook · ⧗ Waiting on work-mac · 21 h/);
  assert.match(shown, /Chrome on Mac web app · added \d+ \w+ · last seen not yet ✗ Denied on MacBook/);
  assert.match(shown, /Each machine decides which devices make changes on it\. Approve or deny a device in kebbie on that machine, or open it here from a device that machine approved\./);
  assert.match(html, /data-action="device" data-id="dev_01JB8Q6Z3V9K2M4N5P6R7S8T9W"/);
  assert.match(html, /name="allow-changes"/, "the switch stays");
  assert.doesNotMatch(accountScreen({ me: { account: "acc_x" }, github: false, allowed: true, devices: null }), /Devices/);
});

test("a device's page shows its key and what it sent, newest first and grouped by day", () => {
  const at = (days, hour) => Math.floor(new Date(2026, 10, 11 + days, hour, 5).getTime() / 1000);
  const answer = { type: "answer", address: "auth@api", question: "q", choice: 0, comment: "" };
  const history = [
    { payload: answer, kind: "answer", machineName: "work-mac", label: "Keep the old endpoint", at: at(-6, 16), exp: at(-6, 17), result: { outcome: "done" } },
    { payload: { type: "push", address: "fix-login@shop", head: "9f3c2a1e5b7d4c6a8b0e2f4d6c8a0b2e4f6d8c0a" }, short: "9f3c2a1", machineName: "MacBook", at: at(-1, 9), exp: at(-1, 10), result: { outcome: "done" } },
    { payload: { type: "push", address: "search@shop", head: "9f3c2a1e5b7d4c6a8b0e2f4d6c8a0b2e4f6d8c0a" }, machineName: "work-mac", at: at(-3, 22), exp: at(-3, 23), result: null },
    { payload: { type: "prompt-answer", address: "wt3@kebbie", prompt: "p", key: "y" }, label: "Yes", machineName: "MacBook", at: at(0, 9), exp: at(0, 9) + 600, result: { outcome: "refused", reason: "not-approved", text: "" } },
  ];
  const html = deviceScreen({ device: devices[1], history, now: AT_TWO_PM });
  const shown = text(html);
  assert.match(shown, /^kebbie hub Chrome on Mac web app · added \d+ \w+ · last seen not yet · key 91AB-44F0 ✗ Denied on MacBook Sent from Chrome on Mac · 4/);
  const order = ["Choice on wt3@kebbie not delivered", "Pushed fix-login@shop", "Push of search@shop not delivered", "Answered auth@api"].map((what) => shown.indexOf(what));
  assert.deepEqual([...order].sort((a, b) => a - b), order, shown);
  assert.ok(order.every((index) => index > 0), shown);
  assert.match(shown, /Today Choice on wt3@kebbie not delivered MacBook didn't approve this device within the hour/);
  assert.match(shown, /9f3c2a1 · on MacBook/);
  assert.match(shown, /expired before work-mac took it/);
  assert.match(shown, /"Keep the old endpoint" · on work-mac/);
  assert.match(html, /data-action="devices-back"/);
  assert.match(text(deviceScreen({ device: devices[0], history: [], now: AT_TWO_PM })), /Nothing sent from iPhone 16 in the last 90 days\./);
  assert.deepEqual(historyEntry({ payload: answer, machineName: "MacBook", at: AT_TWO_PM, exp: AT_TWO_PM + 3600, result: null }, AT_TWO_PM), { icon: "⧗", what: "Answer to auth@api waiting", said: "option 1 · on MacBook", at: AT_TWO_PM });
  assert.equal(historyEntry({ payload: answer, machineName: "MacBook", at: AT_TWO_PM, exp: AT_TWO_PM + 3600, result: { outcome: "refused", reason: "denied", text: "" } }, AT_TWO_PM).said, "denied on MacBook");
});


test("a device's history names what it sent to the backlog: the idea and its machine, the item answered and the item started", () => {
  const now = 1_762_956_540;
  const entry = (payload, fields = {}) => ({ machineName: "MacBook", payload, at: now - 60, exp: now + 3_000, result: { outcome: "done", reason: null, text: "ok" }, label: null, ...fields });
  assert.deepEqual(
    [
      historyEntry(entry({ type: "idea", text: "ci push retry flaky", repo: null }), now).what,
      historyEntry(entry({ type: "item-answer", item: "vault:Projects/session.md", index: 0, question: "Q?", answer: "Renew it" }, { label: "Session drops on reload" }), now).what,
      historyEntry(entry({ type: "start-item", item: "local:retry-a-flaky-ci-push", anyway: false }), now).what,
    ],
    ["Idea for MacBook: ci push retry flaky", "Answered a question on Session drops on reload", "Started retry-a-flaky-ci-push"],
  );
  const refused = historyEntry(entry({ type: "start-item", item: "vault:Projects/session.md", anyway: false }, { label: "Session drops on reload", result: { outcome: "refused", reason: "questions-open", blocking: 2, text: "Not started: 2 questions wait on you." } }), now);
  assert.deepEqual([refused.what, refused.said], ["Start of Session drops on reload refused", "Not started: 2 questions wait on you."]);
  assert.equal(historyEntry(entry({ type: "idea", text: "x".repeat(80), repo: null }, { result: null }), now).what, `Idea for MacBook: ${"x".repeat(59)}… waiting`);
});

test("Account shows one Notifications row under This device, and its own page holds the switch, where notices come from, the last failure and the categories", () => {
  const off = accountScreen({ me: { account: "acc_x" }, github: false, allowed: true, devices, notify: { phase: "off", text: "Off", categories: [] }, now: AT_TWO_PM });
  assert.ok(text(off).indexOf("Allow changes") < text(off).indexOf("Notifications Off"), text(off));
  assert.ok(text(off).indexOf("Notifications Off") < text(off).indexOf("Devices · 2"));
  assert.match(off, /<a class="task row-link" href="#notifications" data-action="notifications"><span class="name">Notifications<\/span><span class="meta">Off<\/span><svg class="chev"/);
  assert.doesNotMatch(off, /notify-card|Notify me when|name="notify"/);
  assert.match(accountScreen({ me: { account: "acc_x" }, github: false, allowed: true, notify: { phase: "on", text: "x", categories: [] } }), /<span class="meta ok">On<\/span>/);
  const on = notifySection({ phase: "on", text: "On for this iPhone.", senders: "MacBook and work-mac send here.", failure: "The last notification, at 14:02 from MacBook, couldn't be opened.", categories: ["questions", "new-device", "idea-questions", "ready", "ci-failed"] });
  assert.equal(text(on), "Notifications On for this iPhone. MacBook and work-mac send here. The last notification, at 14:02 from MacBook, couldn't be opened. Notify me when it needs you Questions and prompts A task asks you something, or an agent's prompt waits for a choice A new device to approve A device wants to make changes on one of your machines The lead's questions on an idea The lead wrote out an idea and needs answers before it starts Notify me when work moves Ready to push A task is reviewed and waits for your push CI failed A push came back red A start went through A task you started from here began on its machine");
  assert.equal((on.match(/name="notice-category"/g) ?? []).length, 6);
  assert.equal((on.match(/ checked/g) ?? []).length, 6, "the main switch and five categories");
  assert.match(on, /name="notice-category" value="started" aria-labelledby="n-start-name" aria-describedby="n-start-state" aria-checked="false">/);
  assert.doesNotMatch(notifySection({ phase: "off", text: "Off", senders: "MacBook sends here." }), /sends here/, "machines that send here show only while it is on");
  assert.match(notifySection({ phase: "asking", text: "Asking iOS for permission…" }), /aria-checked="true" checked>/);
  for (const phase of ["denied", "install", "unsupported"]) assert.match(notifySection({ phase, text: "x" }), /aria-checked="false" disabled>/, phase);
  const page = notificationsScreen({ notify: { phase: "off", text: "Off", categories: [] } });
  assert.match(text(page), /^kebbie hub Notifications Notifications Off Back to Account/);
  assert.equal((page.match(/data-action="notifications-back"/g) ?? []).length, 2);
  assert.doesNotMatch(accountScreen({ me: { account: "acc_x" }, github: false, allowed: null, notify: { phase: "off", text: "Off" } }), /notifications/, "no row where the hub takes no commands");
});

test("a subtask shows a rules check that is running, broken or pushed past on its own line, and a passed one not at all", () => {
  const row = (rules) => text(subtaskList({ ...required, subtasks: [subtask("docs", "Docs", "ready", [1], { rules })] }));
  const docs = "● Docs 1 commit · to review";
  assert.equal(row({ state: "ok" }), docs);
  assert.equal(row({ state: "checking" }), `${docs} checking project rules`);
  assert.equal(row({ state: "breaks", breaks: 1 }), `${docs} breaks 1 project rule`);
  assert.equal(row({ state: "breaks", breaks: 3, names: ["Every commit says why"] }), `${docs} breaks 3 project rules`, "without expanding, a row only counts");
  assert.equal(row({ state: "breaks", breaks: 2, override: { by: "user" } }), `${docs} breaks 2 project rules pushed past the rules check by you`);
  assert.equal(row({ state: "checking", override: { by: "lead" } }), `${docs} pushed past the rules check by the lead`);
  const card = subtaskList({ ...required, subtasks: [subtask("docs", "Docs", "ready", [1], { rules: { state: "breaks", breaks: 2, names: ["Every commit says why", "<No> TODO"] } })] }, { expandable: true });
  assert.match(card, /<details class="rules-names"><summary class="warn">breaks 2 project rules<\/summary><ul><li>Every commit says why<\/li><li>&#60;No&#62; TODO<\/li><\/ul><\/details>/);
  assert.equal(rulesLines(null), "");
  assert.doesNotMatch(subtaskList(payments), /approved|rules ✓/);
});

test("a device waiting on a machine gets Approve and Deny there, only from a device that machine approved and that takes decisions", async () => {
  const { decisionBlock } = await import("./decide.js");
  const waiting = devices[1];
  const standing = { machine: "MacBook", machineId: "mch_1", state: "waiting", fingerprint: "91AB-44F0", until: AT_TWO_PM + 3600, mine: "approved", accepts: ["answer", "device-decision"] };
  const block = (changes = {}, sent = null, self = false) => text(decisionBlock({ standing: { ...standing, ...changes }, device: waiting, sent, self, now: AT_TWO_PM }));
  const html = decisionBlock({ standing, device: waiting, now: AT_TWO_PM });
  assert.match(html, /data-action="decide-device" data-machine="mch_1" data-approve="false">Deny<\/button><button type="button" class="btn primary" data-action="decide-device" data-machine="mch_1" data-approve="true">Approve<\/button>/);
  assert.equal(block({ mine: "waiting" }), "Approve or deny it in kebbie on MacBook.");
  assert.equal(block({ accepts: ["answer"] }), "MacBook takes approvals only at the machine.");
  assert.equal(block({}, null, true), "Approve this device in kebbie on MacBook.");
  assert.equal(block({ state: "approved" }), "");
  const sent = (result, approve = true) => ({ approve, machineName: "MacBook", at: AT_TWO_PM - 60, exp: AT_TWO_PM + 3540, deviceName: "iPhone 15 Pro", result });
  assert.match(block({}, sent(null)), /^Approval sent to MacBook MacBook approves Chrome on Mac when it takes it\. Sent from iPhone 15 Pro at \d\d:\d\d · expires/);
  assert.doesNotMatch(decisionBlock({ standing, device: waiting, sent: sent(null), now: AT_TWO_PM }), /decide-device/, "no second decision while one is on its way");
  assert.match(block({}, sent({ outcome: "done" }, false)), /^✓ Denied on MacBook/);
  const refused = (reason) => block({}, sent({ outcome: "refused", reason, text: "" }));
  assert.match(refused("not-allowed"), /^MacBook takes approvals only at the machine Approve or deny Chrome on Mac in kebbie on MacBook\./);
  assert.match(refused("not-approved"), /^MacBook hasn't approved this device yet/);
  assert.match(refused("unknown-device"), /^Chrome on Mac no longer waits on MacBook/);
  assert.match(decisionBlock({ standing, device: waiting, sent: sent({ outcome: "refused", reason: "not-allowed", text: "" }), now: AT_TWO_PM }), /data-approve="true"/, "a refusal leaves the buttons to try again");
  assert.match(block({}, { ...sent(null), exp: AT_TWO_PM - 1 }), /^Not delivered: MacBook didn't take it within the hour/);
  const page = deviceScreen({ device: { ...waiting, standing: [standing] }, history: [], decisions: new Map([["mch_1", decisionBlock({ standing, device: waiting, now: AT_TWO_PM })]]), now: AT_TWO_PM });
  assert.match(text(page), /⧗ Waiting on MacBook · 1 h Deny Approve Sent from Chrome on Mac/);
});

test("a device's history names a decision by the device it was about", () => {
  const entry = (result) => ({ payload: { type: "device-decision", device: "dev_x", fingerprint: "91AB-44F0", approve: true }, targetName: "iPad", machineName: "MacBook", at: AT_TWO_PM, exp: AT_TWO_PM + 3600, result });
  assert.deepEqual(historyEntry(entry({ outcome: "done" }), AT_TWO_PM), { icon: "✓", what: "Approved iPad", said: "on MacBook", at: AT_TWO_PM });
  assert.deepEqual(historyEntry(entry({ outcome: "refused", reason: "not-allowed", text: "" }), AT_TWO_PM), { icon: "✗", what: "Decision on iPad refused", said: "MacBook takes approvals only at the machine", at: AT_TWO_PM });
  assert.equal(historyEntry(entry(null), AT_TWO_PM).what, "Decision on iPad waiting");
});

test("the device's history names review comments by count and an accept by its step", () => {
  const done = { outcome: "done", reason: null, text: "" };
  const entry = (payload) => historyEntry({ payload, machineName: "MacBook", at: 1_761_070_000, exp: 1_761_073_600, result: done }, 1_761_070_000).what;
  const comment = { path: "src/a.rs", line: 1, side: "new", text: "x" };
  assert.equal(entry({ type: "review-comments", address: "retry-push@kebbie", step: 1, comments: [comment, comment, comment] }), "Sent 3 review comments on retry-push@kebbie");
  assert.equal(entry({ type: "accept-step", address: "retry-push@kebbie", step: 1, head: "3f9a2d1c8e7b6a5f4d3c2b1a0f9e8d7c6b5a4f3e" }), "Accepted step 1 of retry-push@kebbie");
  assert.equal(entry({ type: "accept-subtask", address: "retry-push@kebbie", subtask: "retry", head: "3f9a2d1c8e7b6a5f4d3c2b1a0f9e8d7c6b5a4f3e" }), "Accepted a subtask of retry-push@kebbie");
  assert.equal(entry({ type: "review-read", address: "retry-push@kebbie", path: "src/ci.rs", read: true, content: "a".repeat(40) }), "Marked src/ci.rs read on retry-push@kebbie");
  assert.equal(entry({ type: "review-open", address: "retry-push@kebbie" }), "Opened the review of retry-push@kebbie");
});

test("the device's history names an item edit by its section, or the title", () => {
  const done = { outcome: "done", reason: null, text: "" };
  const entry = (payload, heading) => historyEntry({ payload, heading, label: "Session drops on reload", machineName: "MacBook", at: 1_761_070_000, exp: 1_761_073_600, result: done }, 1_761_070_000).what;
  const edit = { type: "item-edit", item: "vault:Projects/session.md", text: "x", base: "a".repeat(64) };
  assert.equal(entry({ ...edit, section: "goal" }), "Edited “goal” of Session drops on reload");
  assert.equal(entry({ ...edit, section: "what-we-know" }, "What we know"), "Edited “What we know” of Session drops on reload");
  assert.equal(entry({ ...edit, section: null }), "Edited the title of Session drops on reload");
});

test("a device's page shows older history on request and removes the device only after a confirm that says what stays", () => {
  const history = [{ payload: { type: "answer", address: "auth@api", question: "q", choice: 0, comment: "" }, kind: "answer", machineName: "work-mac", at: AT_TWO_PM - 60, exp: AT_TWO_PM + 3540, result: { outcome: "done" } }];
  const more = deviceScreen({ device: devices[1], history, older: true, now: AT_TWO_PM });
  assert.match(text(more), /Sent from Chrome on Mac · 1\+ /);
  assert.match(more, /data-action="device-older">Show older</);
  assert.doesNotMatch(deviceScreen({ device: devices[1], history, now: AT_TWO_PM }), /device-older/);
  assert.match(more, /data-action="device-remove-ask">Remove Chrome on Mac</);
  const asking = text(deviceScreen({ device: devices[0], history, removing: true, now: AT_TWO_PM }));
  assert.match(asking, /Remove iPhone 16\? It can't send through this hub any more and gets no notifications\. What it sent stays listed\. Each machine keeps its own approval of it; deny it in kebbie on a machine to stop it there too\. This browser makes a new device key the next time you send something from it\. Cancel Remove/);
  const removed = { ...devices[1], removed: AT_TWO_PM - 120, standing: [] };
  const page = deviceScreen({ device: removed, history, now: AT_TWO_PM });
  assert.match(text(page), /web app · added \d+ \w+ · removed today · key 91AB-44F0/);
  assert.match(text(page), /Removed from your account\. It sends nothing through this hub and gets no notifications; what it sent stays listed here\./);
  assert.doesNotMatch(page, /device-remove/);
  const listed = text(accountScreen({ me: { account: "acc_x" }, github: false, allowed: true, devices: [devices[0], removed], now: AT_TWO_PM }));
  assert.match(listed, /Devices · 1 iPhone 16 .* Removed · 1 Chrome on Mac web app · added \d+ \w+ · removed today/);
});

test("a card that is current says nothing about its age, and one written before the last commit warns that the code changed", () => {
  assert.equal(cardAge({ sha: "9f3c2a1", current: true }), "");
  assert.equal(cardAge({ sha: "9f3c2a1", current: false }), `<div class="meta warn">Written before the last commit; the code changed since.</div>`);
});
