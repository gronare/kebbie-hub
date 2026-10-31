import assert from "node:assert/strict";
import { test } from "node:test";

import { acceptCount, accountScreen, agentChip, awayLine, laterGroups, nextList, promptBlock, promptParts, reviewLines, routinesBlock, taskFacts, waited, addPasskeyScreen, cantUnlockScreen, connectDone, connectScreen, githubErrorText, hubProNote, installHint, lastSeenText, machineDots, machineRow, nav, optionCards, signInScreen, signupScreen, setupScreen, sleepingRow, stepLines, taskKey, taskRow, unlockingScreen } from "./screens.js";

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
  assert.equal(text(connectDone({ name: "mac", host: "hub.kebbie.dev" })), "✓ mac is connected Its board shows here now. On your phone, open hub.kebbie.dev and sign in with the same passkey.");
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
  const mark = /^<header class="brand"><svg class="brand-mark" viewBox="0 0 32 32" aria-hidden="true">.*<\/svg><b>kebbie hub<\/b><\/header>/;
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

const hrefs = (html) => [...html.matchAll(/<a\b[^>]*href="([^"]*)"[^>]*>([^<]*)</g)].map(([, href, label]) => ({ label: label.trim(), href: href.replace(/&#38;/g, "&") }));

test("the GitHub buttons start GitHub sign-in and come back to the page they left", () => {
  assert.deepEqual(hrefs(connectScreen({ view, signupOpen: true, github: true })), [{ label: "Continue with GitHub", href: "/auth/github?connect=Z78K-XF53&next=%2Fconnect%3Fcode%3DZ78K-XF53" }]);
  assert.deepEqual(hrefs(connectScreen({ view, signupOpen: false, github: true })), [{ label: "Sign in with GitHub", href: "/auth/github?next=%2Fconnect%3Fcode%3DZ78K-XF53" }]);
  assert.deepEqual(hrefs(signInScreen({ github: true })), [{ label: "Sign in with GitHub", href: "/auth/github" }]);
  assert.deepEqual(hrefs(signInScreen({ github: true, next: "/v1/a/billing/checkout" })), [{ label: "Sign in with GitHub", href: "/auth/github?next=%2Fv1%2Fa%2Fbilling%2Fcheckout" }]);
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
  assert.equal(text(linked), "kebbie hub Account acc_1 GitHub Signed in with GitHub as octocat Remove Back to your boards");
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

const stripe = {
  address: "stripe@billing",
  group: "WORKING",
  summary: "2 commits",
  next_for_you: null,
  git: { upstream: "main", ahead: 2, behind: 0, uncommitted: 0, approved: true },
  steps: {
    pushed: 1,
    items: [
      { number: 2, title: "Charge through a PaymentGateway", state: "committed", sha: "2222222bbbb" },
      { number: 3, title: "Add Stripe as the gateway", state: "in-progress", sha: null },
      { number: 4, title: "Remove the old <payment> client", state: "planned", sha: null },
    ],
  },
};
const rowOf = (item) => ({ machine: "mch_a", name: "devbox", item, task: "stripe", repo: "billing" });

const required = { ...stripe, steps: { ...stripe.steps, required: true } };

test("a task whose steps must be accepted lists them under its row, says how many wait to be accepted and which one is next", () => {
  const html = taskRow(rowOf(required));
  assert.equal(
    text(html),
    "stripe devbox billing · 1 to accept · 2 commits ↑ 1 pushed ● 2 Charge through a PaymentGateway committed · rules ✓ ◐ 3 Add Stripe as the gateway in progress ○ 4 Remove the old <payment> client you: accept 2 on devbox",
  );
  assert.match(html, /<span class="accept">1 to accept<\/span>/);
  assert.match(html, /class="step s-committed"/);
  assert.match(html, /class="step s-in-progress"/);
  assert.doesNotMatch(html, /<payment>/, "a step title is escaped");
});

test("a step's label follows its state, and rules show only when the rules passed", () => {
  const steps = { pushed: 0, items: [{ number: 1, title: "Docs", state: "accepted", sha: "aaa" }, { number: 2, title: "Tests", state: "committed", sha: "bbb" }] };
  assert.equal(text(stepLines({ ...stripe, git: { ...stripe.git, approved: false }, steps: { ...steps, required: true } })), "✓ 1 Docs accepted ● 2 Tests committed");
  assert.equal(text(taskRow(rowOf({ ...stripe, next_for_you: "answer: Keep it?", steps: { ...steps, required: true } }))).endsWith("you: answer: Keep it?"), true, "what kebbie asks of you comes before an accept");
});

test("steps that need no accepting count nothing to accept, ask nothing of you and say push takes what is committed", () => {
  for (const steps of [stripe.steps, { ...stripe.steps, required: false }]) {
    const said = text(taskRow(rowOf({ ...stripe, steps })));
    assert.equal(said, "stripe devbox billing · 2 commits ↑ 1 pushed ● 2 Charge through a PaymentGateway committed · rules ✓ ◐ 3 Add Stripe as the gateway in progress ○ 4 Remove the old <payment> client accepting each step is optional: push takes what is committed");
    assert.doesNotMatch(said, /to accept|you:/);
  }
});

test("a task without steps shows its row as before, and the board's count only when something waits", () => {
  const plain = { ...stripe, steps: undefined };
  assert.equal(text(taskRow(rowOf(plain))), "stripe devbox billing · 2 commits");
  assert.equal(stepLines(plain), "");
  assert.equal(stepLines({ ...stripe, steps: { pushed: 0, items: [] } }), "");
  assert.equal(text(acceptCount(3)), "◆ 3 to accept");
  assert.equal(acceptCount(0), "");
});

test("a question's options show as cards with only the recommended one marked, and answering is promised for Hub Pro", () => {
  const options = [{ label: "Keep it", meaning: "deprecate now" }, { label: "Remove <it>", meaning: "drop it in this task" }];
  const html = optionCards(options, 2);
  assert.equal(text(html), "Keep it deprecate now Recommended Remove <it> drop it in this task");
  assert.equal(html.match(/class="opt rec"/g).length, 1);
  assert.match(html, /class="opt rec"><span class="tag">Recommended<\/span><b>Remove &#60;it&#62;/);
  assert.doesNotMatch(optionCards(options, null), /opt rec|Recommended/);
  assert.doesNotMatch(html, /<button/, "nothing to press until answering ships");
  assert.match(text(hubProNote()), /^Coming soon · Hub Pro .* Today you answer at the machine: N on the board, or ask the lead\.$/);
});

test("the nav names the tabs, counts waiting questions in a badge, marks the open tab and links the account", () => {
  const html = nav({ active: "questions", questions: 2 });
  assert.deepEqual(hrefs(html), [{ label: "Board", href: "/#/board" }, { label: "Questions", href: "/#/questions" }, { label: "Machines", href: "/#/machines" }, { label: "Account", href: "/account" }]);
  assert.match(html, /class="on" aria-current="page">Questions <em class="badge">2<\/em>/);
  assert.doesNotMatch(nav({ active: "board" }), /badge|Admin/);
  assert.match(nav({ active: "account", deskOnly: true }), /^<nav class="tabbar desk-nav">/);
});

test("the selected task is outlined on the board, and only that one", () => {
  const selected = taskKey("mch_a", "stripe@billing");
  assert.match(taskRow(rowOf(stripe), selected), /^<a class="task sel" href="#\/task\/mch_a\/stripe%40billing" aria-current="true">/);
  assert.match(taskRow(rowOf(stripe), taskKey("mch_b", "stripe@billing")), /^<a class="task" href=/);
});

test("a machine reads awake or asleep beside its dot, and the board's header lists every machine", () => {
  assert.equal(text(machineRow({ machine: "mch_a", name: "mac", awake: true, facts: ["kebbie 0.24.0"] })), "mac awake · kebbie 0.24.0 Remove");
  assert.match(machineRow({ machine: "mch_a", name: "studio", awake: false, facts: [] }), /class="mrow asleep"><span class="dot"/);
  assert.equal(text(machineDots([{ name: "mac", awake: true, lastSeen: "12:00" }, { name: "studio", awake: false, lastSeen: "Fri 17:40" }])), "mac studio · last seen Fri 17:40");
});

const NOW = 1_761_070_000;
const nextRow = (rank, part, address, line, since, item = null, waits = 0) => ({ rank, part, address, line, since, waits_behind: waits, machine: "mch_a", name: "mac", item, task: address.split("@")[0], repo: address.split("@")[1] });
const codexTask = { ...stripe, address: "hub@kebbie", group: "NEEDS YOU", title: "Clarify machine removal", agent: { status: "blocked", kind: "codex", model: "gpt-6.1-sol" }, prompt: { id: "3f2a9c1d", question: "Would you like to run the following command?", options: [] }, steps: undefined };

test("Next lists what needs you in kebbie's order under its parts, with each task's title, agent, line and wait, and the counts below", () => {
  const rows = [nextRow(1, "stopped", "hub@kebbie", "answer Codex's prompt", NOW - 12 * 60, codexTask), nextRow(2, "push", "card@web", "2 commits · 2 tasks wait on it", NOW - 3600, { ...stripe, title: "Card component" }, 2), nextRow(3, "read", "research@kebbie", "result ready · stay on the TUI", null)];
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
  const item = { ...stripe, steps: undefined, title: "Round VAT per invoice", agent: { status: "working", kind: "claude", model: "opus" }, recipe: "small fix", assessment: "review suggested: touches money" };
  assert.equal(text(taskRow(rowOf(item))), "Round VAT per invoice devbox billing · claude · opus · recipe: small fix · review suggested: touches money · 2 commits");
  assert.equal(text(taskRow(rowOf({ ...stripe, steps: undefined }))), "stripe devbox billing · 2 commits", "a board from an older kebbie names the task as before");
  assert.equal(agentChip({ status: "idle" }), "", "an agent without its kind shows no chip");
});

test("the task card names its agent, profile and recipe, and its review section says what holds the push", () => {
  const item = { ...stripe, agent: { status: "working", kind: "claude", model: "opus" }, profile: "task", recipe: "small fix", assessment: "review suggested: touches money and its tests", verdict: { verdict: "ok", findings: 0, sha: "abc", stale: false, running: false, held: true }, second_opinion: { by: "codex-review", points: 2, running: false } };
  assert.equal(text(taskFacts(item)), "claude · opus · profile task · recipe: small fix");
  assert.equal(text(reviewLines(item)), "Review review suggested: touches money and its tests reviewer ✓ ok · second opinion codex-review: 2 points the push waits for the reviewer's pass; the second opinion never holds it");
  const changes = { ...item, second_opinion: undefined, verdict: { ...item.verdict, verdict: "changes", findings: 1, held: false } };
  assert.equal(text(reviewLines(changes)), "Review review suggested: touches money and its tests reviewer: 1 finding the push does not wait for the reviewer");
  assert.equal(text(reviewLines({ ...stripe, verdict: { verdict: null, findings: null, sha: null, stale: false, running: true } })), "Review reviewer running", "an older kebbie's verdict says nothing about holding");
  assert.equal(reviewLines(stripe), "");
  assert.equal(taskFacts(stripe), "");
});

test("an agent's own prompt shows its question, the command apart, the reason, and each option with its key", () => {
  const question = "Would you like to run the following command?\nEnvironment: local\nReason: May I record that this task is waiting on the lead?\n$ kebbie report --state blocked --next nothing 'Read-only\ninspection complete.'";
  assert.deepEqual(promptParts(question), { question: "Would you like to run the following command?", command: "kebbie report --state blocked --next nothing 'Read-only inspection complete.'", details: ["Environment: local", "May I record that this task is waiting on the lead?"] });
  const options = [{ label: "Yes, proceed (y)", takes_text: false, key: "y" }, { label: "No, and tell Codex what to do differently (esc)", takes_text: false, key: "esc" }, { label: "Type something", takes_text: true }];
  const html = promptBlock({ item: { ...codexTask, prompt: { id: "3f2a9c1d", question, options } }, repo: "kebbie" });
  assert.equal(text(html), "Would you like to run the following command? kebbie · codex Environment: local May I record that this task is waiting on the lead? kebbie report --state blocked --next nothing 'Read-only inspection complete.' Yes, proceed (y) No, and tell Codex what to do differently (esc) Type something with text Coming soon · Hub Pro Answer from your phone. Answer here. Today you answer at the machine: N on the board, or ask the lead.");
  assert.match(html, /<div class="cmd">kebbie report/);
  assert.doesNotMatch(html, /<button/);
  const claude = promptParts("Bash command\ncp ../web/logo.png Assets/\nCopy the logo\nDo you want to proceed?");
  assert.deepEqual(claude, { question: "Do you want to proceed?", command: "", details: ["Bash command", "cp ../web/logo.png Assets/", "Copy the logo"] });
});

test("queued and parked tasks list under their own groups with their line, and a parked one says when a question waits", () => {
  const groups = [
    { key: "queued", title: "Queued", rows: [{ machine: "mch_a", name: "mac", task: "mail-dns", repo: "greenhouse", later: { address: "mail-dns@greenhouse", title: "Mail DNS to Migadu", state: "queued", line: "waits for Forsmancare go-live", until: null, question: false } }] },
    { key: "parked", title: "Parked", rows: [{ machine: "mch_a", name: "mac", task: "herdr-app", repo: "kebbie", later: { address: "herdr-app@kebbie", title: null, state: "parked", line: "nothing yet", until: NOW + 3600, question: true } }] },
  ];
  const said = text(laterGroups(groups, NOW));
  assert.match(said, /^Queued · 1 Mail DNS to Migadu mac greenhouse · waits for Forsmancare go-live Parked · 1 herdr-app mac kebbie · nothing yet · until \d\d:\d\d · a question waits$/);
  assert.equal(laterGroups([], NOW), "");
});

test("routines list with their next run and last outcome, and away mode shows as a line", () => {
  const said = text(routinesBlock([{ machine: "mch_a", name: "mac", routine: { id: "r1", title: "Nightly deps", repo: "greenhouse", mode: "fix", next_run: NOW + 600, last_outcome: "ok" } }, { machine: "mch_a", name: "mac", routine: { id: "r2", title: "Weekly lint", repo: "web", mode: "report", next_run: null, last_outcome: null } }], NOW));
  assert.match(said, /^Routines · 2 Nightly deps · greenhouse · next \d\d:\d\d · last run ok · mac Weekly lint · web · not scheduled · mac$/);
  assert.equal(routinesBlock([], NOW), "");
  assert.match(text(awayLine([{ machine: "mch_a", name: "mac", since: NOW - 60 }], NOW)), /^mac is away since \d\d:\d\d$/);
  assert.equal(awayLine([], NOW), "");
});
