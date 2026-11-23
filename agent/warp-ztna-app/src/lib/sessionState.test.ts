import assert from "node:assert/strict";
import { test } from "node:test";

import {
  armSessionExpiry,
  normalizeDaemonState,
  shouldMarkSessionExpired,
} from "./sessionState.ts";

test("a stale needs-login frame reads as connecting during a connect attempt", () => {
  assert.equal(normalizeDaemonState("needs_login", true, "connecting"), "connecting");
  assert.equal(normalizeDaemonState("needs_login", true, "needs-login"), "connecting");
});

test("needs-login stays terminal outside a connect attempt", () => {
  assert.equal(normalizeDaemonState("needs_login", false, "connected"), "needs_login");
  assert.equal(normalizeDaemonState("needs_login", false, "disconnected"), "needs_login");
});

test("a connect attempt never downgrades a connected session", () => {
  assert.equal(normalizeDaemonState("needs_login", true, "connected"), "needs_login");
  assert.equal(normalizeDaemonState("connected", true, "connecting"), "connected");
});

test("only a connected frame arms the expiry check", () => {
  assert.equal(armSessionExpiry(false, "needs_login"), false);
  assert.equal(armSessionExpiry(false, "connecting"), false);
  assert.equal(armSessionExpiry(false, "connected"), true);
});

test("a fresh login is not an expired session", () => {
  const armed = armSessionExpiry(false, "needs_login");
  assert.equal(shouldMarkSessionExpired("home", "needs-login", armed), false);
});

test("a connected session that falls back to needs-login has expired", () => {
  const armed = armSessionExpiry(false, "connected");
  assert.equal(shouldMarkSessionExpired("home", "needs-login", armed), true);
});

test("re-entering the connect window disarms expiry until the daemon connects", () => {
  let armed = armSessionExpiry(false, "connected");
  armed = false;
  const state = normalizeDaemonState("needs_login", true, "connecting");
  armed = armSessionExpiry(armed, state);
  assert.equal(shouldMarkSessionExpired("home", "needs-login", armed), false);
});

test("expiry only applies while the main gate is home", () => {
  assert.equal(shouldMarkSessionExpired("welcome", "needs-login", true), false);
  assert.equal(shouldMarkSessionExpired("pending", "needs-login", true), false);
});
