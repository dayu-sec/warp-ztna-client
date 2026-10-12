import assert from "node:assert/strict";
import { test } from "node:test";

import type { ResourceRow } from "../api/access";
import { describeFlips, livenessFlips, livenessState } from "./resourceLiveness.ts";

function row(key: string, address: string, live: boolean): ResourceRow {
  return {
    key,
    address,
    kind: "host",
    protocol: "TCP",
    ports: "5432",
    effective: live,
    live,
    pending: !live,
    ssh: null,
    source: "ticket",
  };
}

function tr(key: string, vars?: Record<string, string>) {
  if (!vars) return key;
  const pairs = Object.entries(vars).map(([name, value]) => `${name}=${value}`);
  return `${key} ${pairs.join(" ")}`;
}

test("the first pass is only a baseline", () => {
  assert.deepEqual(livenessFlips(null, [row("a", "10.0.0.1", true)]), []);
});

test("rows whose liveness did not move report nothing", () => {
  const previous = livenessState([row("a", "10.0.0.1", true), row("b", "10.0.0.2", false)]);

  assert.deepEqual(livenessFlips(previous, [row("a", "10.0.0.1", true), row("b", "10.0.0.2", false)]), []);
});

test("a row that comes online reports an activation", () => {
  const previous = livenessState([row("a", "10.0.0.1", false)]);

  assert.deepEqual(livenessFlips(previous, [row("a", "10.0.0.1", true)]), [
    { address: "10.0.0.1", live: true },
  ]);
});

test("a row that goes offline reports a loss", () => {
  const previous = livenessState([row("a", "10.0.0.1", true)]);

  assert.deepEqual(livenessFlips(previous, [row("a", "10.0.0.1", false)]), [
    { address: "10.0.0.1", live: false },
  ]);
});

test("rows the previous pass never saw are not a change", () => {
  const previous = livenessState([row("a", "10.0.0.1", true)]);

  assert.deepEqual(
    livenessFlips(previous, [row("a", "10.0.0.1", true), row("b", "10.0.0.2", true)]),
    [],
  );
});

test("a wave of activations reports one count, not one message per row", () => {
  const previous = livenessState([row("a", "10.0.0.1", false), row("b", "10.0.0.2", false)]);

  const flips = livenessFlips(previous, [row("a", "10.0.0.1", true), row("b", "10.0.0.2", true)]);

  assert.equal(flips.length, 2);
  assert.equal(describeFlips(flips, tr), "resources.livenessUpMany count=2");
});

test("copy names the address for one row and counts for several", () => {
  assert.equal(describeFlips([], tr), null);
  assert.equal(
    describeFlips([{ address: "10.0.0.1", live: true }], tr),
    "resources.livenessUp name=10.0.0.1",
  );
  assert.equal(
    describeFlips([{ address: "10.0.0.1", live: false }], tr),
    "resources.livenessDown name=10.0.0.1",
  );
  assert.equal(
    describeFlips(
      [
        { address: "10.0.0.1", live: false },
        { address: "10.0.0.2", live: false },
      ],
      tr,
    ),
    "resources.livenessDownMany count=2",
  );
  assert.equal(
    describeFlips(
      [
        { address: "10.0.0.1", live: true },
        { address: "10.0.0.2", live: false },
      ],
      tr,
    ),
    "resources.livenessMixed up=1 down=1",
  );
});
