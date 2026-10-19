import { useCallback, useEffect, useState } from "react";
import { formatInvokeError, isTauri, native } from "../api/native";

export type ServiceConfigState = {
  apiUrl: string;
  effectiveApiUrl: string;
  needRestart: boolean;
  saving: boolean;
  setApiUrl: (value: string) => void;
  save: () => Promise<void>;
  restart: () => void;
};

export function useServiceConfig(
  tr: (key: string, vars?: Record<string, string>) => string,
  showToast: (msg: string) => void,
): ServiceConfigState {
  const [apiUrl, setApiUrl] = useState("");
  const [effectiveApiUrl, setEffectiveApiUrl] = useState("");
  const [needRestart, setNeedRestart] = useState(false);
  const [saving, setSaving] = useState(false);

  const load = useCallback(async () => {
    if (!isTauri()) return;
    const config = await native.getServiceConfig().catch(() => null);
    if (!config) return;
    setApiUrl(config.stored_api_url);
    setEffectiveApiUrl(config.effective_api_url);
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function save() {
    if (!isTauri()) return;
    setSaving(true);
    try {
      const api = apiUrl.trim();
      await native.saveServiceConfig(api);
      setApiUrl(api);
      setNeedRestart(true);
      showToast(tr("serviceConfig.saved"));
    } catch (cause) {
      showToast(formatInvokeError(cause));
    } finally {
      setSaving(false);
    }
  }

  function restart() {
    if (isTauri()) void native.restartApp();
  }

  return {
    apiUrl,
    effectiveApiUrl,
    needRestart,
    saving,
    setApiUrl,
    save,
    restart,
  };
}
