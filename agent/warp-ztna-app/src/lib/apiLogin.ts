export const API_LOGIN_REQUIRED_CODE = "biz.api_login_required";
export const API_LOGIN_IN_PROGRESS_CODE = "biz.api_login_in_progress";
export const API_LOGIN_TIMEOUT_CODE = "biz.api_login_timeout";
export const API_LOGIN_CANCELLED_CODE = "biz.api_login_cancelled";
export const API_LOGIN_BROWSER_FAILED_CODE = "biz.api_login_browser_failed";

export type ApiLoginFailure =
  | "required"
  | "in_progress"
  | "timeout"
  | "cancelled"
  | "browser_failed";

const FAILURES: Record<string, ApiLoginFailure> = {
  [API_LOGIN_REQUIRED_CODE]: "required",
  [API_LOGIN_IN_PROGRESS_CODE]: "in_progress",
  [API_LOGIN_TIMEOUT_CODE]: "timeout",
  [API_LOGIN_CANCELLED_CODE]: "cancelled",
  [API_LOGIN_BROWSER_FAILED_CODE]: "browser_failed",
};

export function apiLoginFailure(cause: unknown): ApiLoginFailure | null {
  if (!cause || typeof cause !== "object") return null;
  const code = (cause as { code?: unknown }).code;
  if (typeof code !== "string") return null;
  return FAILURES[code] ?? null;
}

export function isApiLoginRequired(cause: unknown): boolean {
  return apiLoginFailure(cause) === "required";
}

export function isApiLoginInProgress(cause: unknown): boolean {
  return apiLoginFailure(cause) === "in_progress";
}

export function isApiLoginCancelled(cause: unknown): boolean {
  return apiLoginFailure(cause) === "cancelled";
}

export function apiLoginMessageKey(cause: unknown): string | null {
  switch (apiLoginFailure(cause)) {
    case "timeout":
      return "welcome.ssoTimeout";
    case "in_progress":
      return "welcome.ssoInProgress";
    case "browser_failed":
      return "welcome.ssoBrowserFailed";
    default:
      return null;
  }
}
