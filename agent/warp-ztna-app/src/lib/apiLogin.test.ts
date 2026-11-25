import assert from "node:assert/strict";
import { test } from "node:test";
import {
  API_LOGIN_BROWSER_FAILED_CODE,
  API_LOGIN_CANCELLED_CODE,
  API_LOGIN_IN_PROGRESS_CODE,
  API_LOGIN_REQUIRED_CODE,
  API_LOGIN_TIMEOUT_CODE,
  apiLoginFailure,
  apiLoginMessageKey,
  isApiLoginInProgress,
  isApiLoginRequired,
  isApiLoginCancelled,
  API_SESSION_BUSY_RETRY_MS,
  retryWhileLoginInProgress,
} from "./apiLogin.ts";

test("classifies each stable API login code", () => {
  assert.equal(apiLoginFailure({ code: API_LOGIN_REQUIRED_CODE }), "required");
  assert.equal(apiLoginFailure({ code: API_LOGIN_IN_PROGRESS_CODE }), "in_progress");
  assert.equal(apiLoginFailure({ code: API_LOGIN_TIMEOUT_CODE }), "timeout");
  assert.equal(apiLoginFailure({ code: API_LOGIN_CANCELLED_CODE }), "cancelled");
  assert.equal(apiLoginFailure({ code: API_LOGIN_BROWSER_FAILED_CODE }), "browser_failed");
});

test("does not confuse other failures with a login failure", () => {
  assert.equal(apiLoginFailure({ code: "biz.unauthorized" }), null);
  assert.equal(apiLoginFailure({ code: "biz.forbidden" }), null);
  assert.equal(apiLoginFailure({ code: "sys.external" }), null);
  assert.equal(apiLoginFailure({ message: "boom" }), null);
  assert.equal(apiLoginFailure("boom"), null);
  assert.equal(apiLoginFailure(null), null);
  assert.equal(apiLoginFailure(undefined), null);
});

test("narrows the failure kinds used by the agent hook", () => {
  assert.equal(isApiLoginRequired({ code: API_LOGIN_REQUIRED_CODE }), true);
  assert.equal(isApiLoginRequired({ code: API_LOGIN_TIMEOUT_CODE }), false);
  assert.equal(isApiLoginInProgress({ code: API_LOGIN_IN_PROGRESS_CODE }), true);
  assert.equal(isApiLoginCancelled({ code: API_LOGIN_CANCELLED_CODE }), true);
});

test("maps stable codes to overlay copy keys", () => {
  assert.equal(apiLoginMessageKey({ code: API_LOGIN_TIMEOUT_CODE }), "welcome.ssoTimeout");
  assert.equal(apiLoginMessageKey({ code: API_LOGIN_IN_PROGRESS_CODE }), "welcome.ssoInProgress");
  assert.equal(
    apiLoginMessageKey({ code: API_LOGIN_BROWSER_FAILED_CODE }),
    "welcome.ssoBrowserFailed",
  );
  assert.equal(apiLoginMessageKey({ code: API_LOGIN_REQUIRED_CODE }), null);
  assert.equal(apiLoginMessageKey({ code: API_LOGIN_CANCELLED_CODE }), null);
});

test("retries while a concurrent API sign-in holds the lock", async () => {
  let calls = 0;
  const sleeps: number[] = [];
  const token = await retryWhileLoginInProgress(
    async () => {
      calls += 1;
      if (calls < 3) throw { code: API_LOGIN_IN_PROGRESS_CODE };
      return "token";
    },
    async (ms) => {
      sleeps.push(ms);
    },
  );
  assert.equal(token, "token");
  assert.equal(calls, 3);
  assert.deepEqual(sleeps, [API_SESSION_BUSY_RETRY_MS, API_SESSION_BUSY_RETRY_MS]);
});

test("does not retry failures that are not a concurrent sign-in", async () => {
  let calls = 0;
  await assert.rejects(
    retryWhileLoginInProgress(
      async () => {
        calls += 1;
        throw { code: API_LOGIN_REQUIRED_CODE };
      },
      async () => {},
    ),
    (cause: unknown) => (cause as { code?: string }).code === API_LOGIN_REQUIRED_CODE,
  );
  assert.equal(calls, 1);
});

test("stops retrying at the limit", async () => {
  let calls = 0;
  await assert.rejects(
    retryWhileLoginInProgress(
      async () => {
        calls += 1;
        throw { code: API_LOGIN_IN_PROGRESS_CODE };
      },
      async () => {},
      2,
    ),
    (cause: unknown) => (cause as { code?: string }).code === API_LOGIN_IN_PROGRESS_CODE,
  );
  assert.equal(calls, 3);
});
