import type { StreamHealth } from "../api/native";
import type { UiConn } from "../types/ui";

export type StatusFrame = {
  state: string;
  stream: StreamHealth;
};

export function mapDaemonState(state: string): UiConn {
  if (state === "connected") return "connected";
  if (state === "connecting") return "connecting";
  if (state === "needs_login") return "needs-login";
  if (state === "error") return "error";
  return "disconnected";
}

export function nextConnection(
  current: UiConn,
  frame: StatusFrame,
  signed = true,
  connectPending = false,
): UiConn {
  if (frame.stream === "reconnecting") {
    if (!signed || current === "needs-login") return current;
    return mapDaemonState(frame.state) === "error" ? "error" : "reconnecting";
  }

  const mapped = mapDaemonState(frame.state);
  if (!signed && mapped !== "needs-login" && mapped !== "connected") return "needs-login";
  if (connectPending && current === "connecting" && mapped === "disconnected") return current;
  return mapped;
}
