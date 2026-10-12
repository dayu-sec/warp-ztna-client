import assert from "node:assert/strict";
import { test } from "node:test";

import { ssoSignInUrl } from "./ssoUrl.ts";

test("the complete uri wins when present", () => {
  assert.equal(
    ssoSignInUrl("https://idp.example.com/device?user_code=ABCD", "https://idp.example.com/device"),
    "https://idp.example.com/device?user_code=ABCD",
  );
});

test("a blank complete uri falls back to the bare uri", () => {
  assert.equal(ssoSignInUrl("", "https://idp.example.com/device"), "https://idp.example.com/device");
  assert.equal(
    ssoSignInUrl("   ", "https://idp.example.com/device"),
    "https://idp.example.com/device",
  );
});

test("both blank yields nothing to copy", () => {
  assert.equal(ssoSignInUrl("", ""), "");
  assert.equal(ssoSignInUrl("  ", " \t "), "");
});

test("padding is stripped from whichever uri is copied", () => {
  assert.equal(ssoSignInUrl("  https://a.example/x  ", "https://b.example/y"), "https://a.example/x");
  assert.equal(ssoSignInUrl("", "  https://b.example/y  "), "https://b.example/y");
});
