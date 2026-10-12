import type { StreamHealth } from "../api/native";

export const DAEMON_UNAVAILABLE_CODE = "sys.unavailable";

export function isDaemonDown(cause: unknown): boolean {
  return Boolean(
    cause &&
      typeof cause === "object" &&
      (cause as { code?: unknown }).code === DAEMON_UNAVAILABLE_CODE,
  );
}

export function isDaemonLive(stream: StreamHealth): boolean {
  return stream === "live";
}
