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
