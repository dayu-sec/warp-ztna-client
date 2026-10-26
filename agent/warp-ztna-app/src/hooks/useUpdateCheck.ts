import { useCallback, useRef, useState } from "react";
import { formatInvokeError, isTauri, native } from "../api/native";
import type { ClientUpdate } from "../lib/updateCheck";

export function useUpdateCheck() {
  const [result, setResult] = useState<ClientUpdate | null>(null);
  const [busy, setBusy] = useState(false);
  const inFlight = useRef<Promise<ClientUpdate | null> | null>(null);

  const check = useCallback(async (): Promise<ClientUpdate | null> => {
    if (!isTauri()) return null;
    if (inFlight.current) return inFlight.current;
    setBusy(true);
    const run = native
      .checkClientUpdate()
      .then((next) => {
        setResult(next);
        return next;
      })
      .catch((cause) => {
        const fallback: ClientUpdate = {
          status: "unreachable",
          current: "",
          latest: null,
          detail: formatInvokeError(cause) || null,
        };
        setResult(fallback);
        return fallback;
      })
      .finally(() => {
        inFlight.current = null;
        setBusy(false);
      });
    inFlight.current = run;
    return run;
  }, []);

  return { result, busy, check };
}
