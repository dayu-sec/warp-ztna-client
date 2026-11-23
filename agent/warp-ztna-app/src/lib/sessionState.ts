import type { UiConn } from "../types/ui";

export type MainGate = "pending" | "welcome" | "home";

export function armSessionExpiry(armed: boolean, daemonState: string): boolean {
  return daemonState === "connected" ? true : armed;
}

export function normalizeDaemonState(
  daemonState: string,
  connectPending: boolean,
  connection: UiConn,
): string {
  if (daemonState === "needs_login" && connectPending && connection !== "connected") {
    return "connecting";
  }
  return daemonState;
}

export function shouldMarkSessionExpired(
  mainGate: MainGate,
  connection: UiConn,
  armed: boolean,
): boolean {
  return armed && mainGate === "home" && connection === "needs-login";
}
