import assert from "node:assert/strict";
import { test } from "node:test";
import { clientUpdateDetail, clientUpdateView, type ClientUpdate } from "./updateCheck.ts";

test("an in-flight check wins over a previous result", () => {
  const previous: ClientUpdate = { status: "latest", current: "0.1.16" };
  assert.deepEqual(clientUpdateView(previous, true), {
    key: "about.updateChecking",
    canDownload: false,
  });
});

test("an idle about page asks the user to check", () => {
  assert.deepEqual(clientUpdateView(null, false), {
    key: "about.updateIdle",
    canDownload: false,
  });
});

test("an available update carries the latest version and the download action", () => {
  const update: ClientUpdate = {
    status: "update_available",
    current: "0.1.16",
    latest: "0.1.17",
  };
  assert.deepEqual(clientUpdateView(update, false), {
    key: "about.updateAvailable",
    vars: { version: "0.1.17" },
    canDownload: true,
  });
});

test("latest and unknown results do not offer a download", () => {
  assert.equal(
    clientUpdateView({ status: "latest", current: "0.1.17", latest: "0.1.17" }, false).key,
    "toast.updateLatest",
  );
  assert.equal(
    clientUpdateView({ status: "unknown", current: "0.1.17", latest: null }, false).key,
    "about.updateUnknown",
  );
  assert.equal(
    clientUpdateView({ status: "unreachable", current: "0.1.17", detail: "api HTTP 503" }, false).key,
    "about.updateUnknown",
  );
});

test("only unknown and unreachable results expose the raw detail", () => {
  assert.equal(
    clientUpdateDetail({ status: "unreachable", current: "0.1.17", detail: "api HTTP 503" }),
    "api HTTP 503",
  );
  assert.equal(
    clientUpdateDetail({ status: "unknown", current: "0.1.17", detail: "  no version  " }),
    "no version",
  );
  assert.equal(clientUpdateDetail({ status: "latest", current: "0.1.17" }), null);
  assert.equal(clientUpdateDetail(null), null);
});
