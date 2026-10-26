export type ClientUpdateStatus = "update_available" | "latest" | "unknown" | "unreachable";

export type ClientUpdate = {
  status: ClientUpdateStatus;
  current: string;
  latest?: string | null;
  detail?: string | null;
};

export type ClientUpdateView = {
  key: string;
  vars?: Record<string, string>;
  canDownload: boolean;
};

export function clientUpdateView(result: ClientUpdate | null, busy: boolean): ClientUpdateView {
  if (busy) return { key: "about.updateChecking", canDownload: false };
  if (!result) return { key: "about.updateIdle", canDownload: false };
  switch (result.status) {
    case "update_available":
      return {
        key: "about.updateAvailable",
        vars: { version: result.latest ?? "" },
        canDownload: true,
      };
    case "latest":
      return { key: "toast.updateLatest", canDownload: false };
    case "unknown":
    case "unreachable":
      return { key: "about.updateUnknown", canDownload: false };
    default:
      return { key: "about.updateIdle", canDownload: false };
  }
}

export function clientUpdateDetail(result: ClientUpdate | null): string | null {
  if (!result || (result.status !== "unknown" && result.status !== "unreachable")) return null;
  const detail = result.detail?.trim();
  return detail ? detail : null;
}
