import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const css = readFileSync(new URL("./app.css", import.meta.url), "utf8").replace(/\/\*[\s\S]*?\*\//g, "");
const rules = [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)].map(([, selector, body]) => ({ selector: selector.trim(), body }));
const FIELD = /(^|[\s,>+~(])(input|textarea|select)(?![\w-])/;
const sizes = (body) => [...body.matchAll(/font(?:-size)?\s*:\s*([^;]+)/g)].flatMap(([, value]) => [...value.matchAll(/(\d+(?:\.\d+)?)px/g)].map((match) => Number(match[1])).slice(0, 1));

test("every field iOS can focus is at least 16px, so focusing it never zooms the page", () => {
  const fields = rules.filter((rule) => FIELD.test(rule.selector));
  assert.ok(fields.length > 3, "the scan finds the field rules");
  for (const rule of fields) for (const size of sizes(rule.body)) assert.ok(size >= 16, `${rule.selector} sets ${size}px`);
  assert.ok(rules.some((rule) => rule.selector === "input, textarea, select" && /font-size:\s*16px/.test(rule.body)), "fields without a size of their own start at 16px");
});

test("the viewport lets people zoom", () => {
  const html = readFileSync(new URL("./index.html", import.meta.url), "utf8");
  assert.doesNotMatch(html, /user-scalable\s*=\s*no|maximum-scale/);
});

test("on the unfolded Duo in landscape the card button sits under Back, never on top of it", () => {
  const rule = (selector) => rules.find((one) => one.selector === selector)?.body ?? "";
  const offset = (body, side) => Number(body.match(new RegExp(`${side}:\\s*calc\\(var\\(--strip-status(?:-bottom)?, 120px\\) \\+ (\\d+)px\\)`))?.[1] ?? 0);
  const at = (body, side) => body.match(new RegExp(`${side}:\\s*var\\(--strip-status(?:-bottom)?, 120px\\)`)) !== null;
  assert.ok(at(rule(".pose-split .back-btn"), "top"), "Back sits at the clock");
  assert.ok(at(rule(".pose-split.strip-cam-bottom .back-btn"), "bottom"), "Back sits at the clock with the camera at the bottom");
  const size = Number(rule(".pose-split .back-btn").match(/height:\s*(\d+)px/)?.[1]);
  assert.ok(offset(rule(".pose-split .back-row:has(> .back-btn) .switch-btn"), "top") > size, "the card button starts below Back");
  const bottom = rule(".pose-split.strip-cam-bottom .back-row:has(> .back-btn) .switch-btn");
  assert.ok(offset(bottom, "bottom") > size && /top:\s*auto/.test(bottom), "with the camera at the bottom the card button sits above Back, further from the clock");
});

test("a page with a Back row shows Back in every pose, and only a pending strip hides it", () => {
  const index = (selector) => rules.findIndex((one) => one.selector === selector);
  const shown = index(".back-row > .back-btn");
  assert.ok(shown >= 0 && /display:\s*grid/.test(rules[shown].body), "Back on a Back row is shown");
  assert.ok(index(".back-btn") < shown, "the default that hides a lone Back comes first");
  assert.ok(shown < index(".strip-pending :is(.back-btn, .switch-btn)"), "a strip that has not settled still hides it");
});

test("a task row keeps its meta, step and you lines to one line each, cut with an ellipsis", () => {
  const body = rules.find((one) => one.selector === ".task-row > :is(.meta, .step-line, .you)")?.body ?? "";
  for (const want of [/white-space:\s*nowrap/, /overflow:\s*hidden/, /text-overflow:\s*ellipsis/]) assert.match(body, want);
});
