import { useCallback, useState } from "react";
import { formatInvokeError, isTauri, native, type ProfileRow } from "../api/native";

export function useProfiles(
  tr: (key: string, vars?: Record<string, string>) => string,
  showToast: (msg: string) => void,
) {
  const [profiles, setProfiles] = useState<ProfileRow[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [profileModal, setProfileModal] = useState<{ id: string | null } | null>(null);
  const [pfName, setPfName] = useState("");
  const [pfUrl, setPfUrl] = useState("");
  const [confirmSwitch, setConfirmSwitch] = useState<ProfileRow | null>(null);

  const active = profiles.find((p) => p.is_active) ?? profiles[0];

  const loadProfiles = useCallback(async () => {
    if (!isTauri()) return;
    try {
      setProfiles(await native.listProfiles());
    } catch (cause) {
      setError(formatInvokeError(cause));
    }
  }, []);

  async function saveProfile() {
    const name = pfName.trim();
    const url = pfUrl.trim();
    if (!name || !url) return;
    try {
      if (isTauri()) {
        if (profileModal?.id) await native.updateProfile(profileModal.id, name, url);
        else await native.addProfile(name, url);
        await loadProfiles();
      }
      setProfileModal(null);
      showToast(tr("toast.saved"));
    } catch (cause) {
      showToast(formatInvokeError(cause));
    }
  }

  async function deleteProfile(p: ProfileRow) {
    if (p.is_active || profiles.length <= 1) return;
    try {
      if (isTauri()) await native.removeProfile(p.id || p.name);
      await loadProfiles();
      showToast(tr("toast.deleted"));
    } catch (cause) {
      showToast(formatInvokeError(cause));
    }
  }

  async function doSwitch(p: ProfileRow) {
    try {
      if (isTauri()) await native.switchProfile(p.id || p.name);
      await loadProfiles();
      showToast(tr("toast.switched"));
      return true;
    } catch (cause) {
      showToast(formatInvokeError(cause));
      return false;
    }
  }

  return {
    profiles,
    active,
    error,
    profileModal,
    setProfileModal,
    pfName,
    setPfName,
    pfUrl,
    setPfUrl,
    confirmSwitch,
    setConfirmSwitch,
    loadProfiles,
    saveProfile,
    deleteProfile,
    doSwitch,
  };
}
