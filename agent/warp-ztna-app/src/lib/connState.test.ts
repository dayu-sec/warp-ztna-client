import assert from "node:assert/strict";
import { test } from "node:test";

import { mapDaemonState, nextConnection, type StatusFrame } from "./connState.ts";

function frame(state: string, stream: StatusFrame["stream"] = "live"): StatusFrame {
  return { state, stream };
}

test("daemon states map onto UI states", () => {
  assert.equal(mapDaemonState("connected"), "connected");
  assert.equal(mapDaemonState("connecting"), "connecting");
  assert.equal(mapDaemonState("needs_login"), "needs-login");
  assert.equal(mapDaemonState("error"), "error");
  assert.equal(mapDaemonState("disconnected"), "disconnected");
  assert.equal(mapDaemonState("unknown"), "disconnected");
  assert.equal(mapDaemonState(""), "disconnected");
});

test("live frames carry the daemon state", () => {
  assert.equal(nextConnection("disconnected", frame("connecting")), "connecting");
  assert.equal(nextConnection("connecting", frame("connected")), "connected");
  assert.equal(nextConnection("connected", frame("disconnected")), "disconnected");
  assert.equal(nextConnection("connected", frame("needs_login")), "needs-login");
});

test("an in-flight connect is not flipped to disconnected", () => {
  assert.equal(nextConnection("connecting", frame("disconnected"), true, true), "connecting");
  assert.equal(nextConnection("connecting", frame("unknown"), true, true), "connecting");
});

test("an idle frame unsticks connecting when no connect is pending", () => {
  assert.equal(nextConnection("connecting", frame("disconnected"), true, false), "disconnected");
  assert.equal(nextConnection("connecting", frame("unknown"), true, false), "disconnected");
  assert.equal(nextConnection("connecting", frame(""), true, false), "disconnected");
});

test("the startup probe cannot pin the UI in connecting", () => {
  let state = nextConnection("disconnected", frame("connecting"), true, false);
  state = nextConnection(state, frame("disconnected"), true, false);
  assert.equal(state, "disconnected");
});

test("connect grace does not bypass the signed guard", () => {
  assert.equal(nextConnection("connecting", frame("disconnected"), false, true), "needs-login");
});

test("connect flags do not override terminal frames", () => {
  assert.equal(nextConnection("connecting", frame("connected"), true, true), "connected");
  assert.equal(nextConnection("connecting", frame("needs_login"), true, true), "needs-login");
  assert.equal(nextConnection("connecting", frame("error"), true, true), "error");
});

test("a degraded stream still wins during a pending connect", () => {
  assert.equal(nextConnection("connecting", frame("unknown", "reconnecting"), true, true), "reconnecting");
});

test("a degraded stream reads as reconnecting, not as the last known state", () => {
  assert.equal(nextConnection("connected", frame("connected", "reconnecting")), "reconnecting");
  assert.equal(nextConnection("disconnected", frame("unknown", "reconnecting")), "reconnecting");
});

test("the daemon escalates a long outage to error", () => {
  assert.equal(nextConnection("reconnecting", frame("error", "reconnecting")), "error");
  assert.equal(nextConnection("connected", frame("error", "reconnecting")), "error");
});

test("a signed-out UI is never pushed into reconnecting", () => {
  assert.equal(nextConnection("needs-login", frame("connected", "reconnecting")), "needs-login");
  assert.equal(nextConnection("needs-login", frame("error", "reconnecting")), "needs-login");
});

test("an unsigned bootstrap only accepts needs-login or connected", () => {
  assert.equal(nextConnection("needs-login", frame("connected"), false), "connected");
  assert.equal(nextConnection("connected", frame("disconnected"), false), "needs-login");
  assert.equal(nextConnection("connected", frame("needs_login"), false), "needs-login");
});

test("a daemon without the push stream is read like an ordinary frame", () => {
  assert.equal(nextConnection("connecting", frame("connected", "unavailable")), "connected");
  assert.equal(nextConnection("connected", frame("disconnected", "unavailable")), "disconnected");
});
