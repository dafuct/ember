import { Check, Plus, Settings as SettingsIcon } from "lucide-react";
import type { AccountInfo } from "../lib/api";

export function AccountSwitcher({
  accounts,
  onSwitch,
  onAdd,
  onManage,
  onClose,
}: {
  accounts: AccountInfo[];
  onSwitch: (email: string) => void;
  onAdd: () => void;
  onManage: () => void;
  onClose: () => void;
}) {
  const initials = (email: string) => email.slice(0, 2).toUpperCase();
  return (
    <>
      <div className="account-backdrop" onClick={onClose} />
      <div className="account-switcher" role="menu" aria-label="Accounts">
        <div className="account-switcher-head">Accounts</div>
        {accounts.map((a) => (
          <button
            key={a.email}
            className={`account-row${a.active ? " active" : ""}`}
            role="menuitem"
            onClick={() => {
              if (!a.active) onSwitch(a.email);
              onClose();
            }}
          >
            <span className="account-initials">{initials(a.email)}</span>
            <span className="account-email">{a.email}</span>
            {a.unread > 0 && <span className="account-unread">{a.unread}</span>}
            {a.active && <Check size={16} className="account-check" />}
          </button>
        ))}
        <div className="account-switcher-sep" />
        <button
          className="account-row account-action"
          role="menuitem"
          onClick={() => {
            onAdd();
            onClose();
          }}
        >
          <span className="account-action-icon">
            <Plus size={16} />
          </span>
          <span>Add account</span>
        </button>
        <button
          className="account-row account-action"
          role="menuitem"
          onClick={() => {
            onManage();
            onClose();
          }}
        >
          <span className="account-action-icon">
            <SettingsIcon size={16} />
          </span>
          <span>Manage in Settings</span>
        </button>
      </div>
    </>
  );
}
