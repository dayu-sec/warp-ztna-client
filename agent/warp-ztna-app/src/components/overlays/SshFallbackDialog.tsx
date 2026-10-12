import { IconCopy } from "../Icons";
import type { SshLaunchPlan } from "../../api/native";

export function sshFallbackReason(
  plan: SshLaunchPlan,
  tr: (key: string) => string,
): string | null {
  if (plan.launch_error?.includes("-1743")) return tr("ssh.automationDenied");
  if (plan.launch_error) return `${tr("ssh.launchFailed")} ${plan.launch_error}`;
  return null;
}

export function sshFallbackCommands(plan: SshLaunchPlan) {
  const commands = [plan.command];
  if (plan.alternate_command && plan.alternate_command !== plan.command) {
    commands.push(plan.alternate_command);
  }
  return commands;
}

export function SshFallbackDialog({
  title,
  body,
  reason,
  commands,
  copyLabel,
  closeLabel,
  onCopy,
  onClose,
}: {
  title: string;
  body: string;
  reason: string | null;
  commands: string[];
  copyLabel: string;
  closeLabel: string;
  onCopy: (text: string) => void;
  onClose: () => void;
}) {
  return (
    <div className="overlay overlay-form">
      <div className="profile-dialog" role="dialog" aria-label={title}>
        <div className="profile-dialog-head">
          <h3>{title}</h3>
          <p>{body}</p>
        </div>
        <div className="profile-dialog-body">
          {reason && <p className="ssh-dialog-reason">{reason}</p>}
          {commands.map((command) => (
            <div className="ssh-dialog-command" key={command}>
              <code>{command}</code>
              <button
                type="button"
                className="ico-btn"
                onClick={() => onCopy(command)}
                aria-label={copyLabel}
                title={copyLabel}
              >
                <IconCopy />
              </button>
            </div>
          ))}
        </div>
        <div className="profile-dialog-foot">
          <button type="button" className="btn btn-ghost" onClick={onClose}>
            {closeLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
