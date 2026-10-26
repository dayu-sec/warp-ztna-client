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
