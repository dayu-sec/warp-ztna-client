import assert from "node:assert/strict";
import { test } from "node:test";

import { DAEMON_UNAVAILABLE_CODE, isDaemonDown, isDaemonLive } from "./daemonDown.ts";

test("a dial failure to the daemon reads as the daemon being down", () => {
  assert.equal(isDaemonDown({ code: DAEMON_UNAVAILABLE_CODE }), true);
  assert.equal(isDaemonDown(new Error("connect netbird daemon")), false);

  const coded = Object.assign(new Error("connect netbird daemon"), {
    code: DAEMON_UNAVAILABLE_CODE,
  });
  assert.equal(isDaemonDown(coded), true);
});

test("a daemon that answers elsewhere failing is not the daemon being down", () => {
  assert.equal(isDaemonDown({ code: "sys.external" }), false);
  assert.equal(isDaemonDown({ code: "sys.timeout" }), false);
  assert.equal(isDaemonDown({ code: "biz.invalid_argument" }), false);
  assert.equal(isDaemonDown({ code: "biz.unauthorized" }), false);
});

test("a payload without a code never reads as the daemon being down", () => {
  assert.equal(isDaemonDown(undefined), false);
  assert.equal(isDaemonDown(null), false);
  assert.equal(isDaemonDown("connect netbird daemon"), false);
  assert.equal(isDaemonDown({ message: "transport error" }), false);
  assert.equal(isDaemonDown({ code: 503 }), false);
});

test("only a live frame proves the daemon answered", () => {
  assert.equal(isDaemonLive("live"), true);
  assert.equal(isDaemonLive("reconnecting"), false);
  assert.equal(isDaemonLive("unavailable"), false);
});
