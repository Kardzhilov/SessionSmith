import { useEffect } from "react";
import { CheckCircle2, CircleAlert, FolderOpen, Info, X } from "lucide-react";

export type AppNotification = {
  id: number;
  key: string;
  tone: "success" | "error" | "info";
  title: string;
  message?: string;
  action?: {
    label: string;
    onClick: () => void;
  };
};

export function NotificationViewport({
  notifications,
  onDismiss,
}: {
  notifications: AppNotification[];
  onDismiss: (id: number) => void;
}) {
  useEffect(() => {
    const timers = notifications.filter((notification) => !notification.action).map((notification) => window.setTimeout(
      () => onDismiss(notification.id),
      notification.tone === "error" ? 9000 : 6000,
    ));
    return () => timers.forEach(window.clearTimeout);
  }, [notifications, onDismiss]);

  const errors = notifications.filter((notification) => notification.tone === "error");
  const updates = notifications.filter((notification) => notification.tone !== "error");

  return (
    <div className="toast-viewport" aria-label="Notifications">
      <div aria-live="assertive" aria-atomic="false">
        {errors.map((notification) => (
          <Toast notification={notification} onDismiss={onDismiss} key={notification.id} />
        ))}
      </div>
      <div aria-live="polite" aria-atomic="false">
        {updates.map((notification) => (
          <Toast notification={notification} onDismiss={onDismiss} key={notification.id} />
        ))}
      </div>
    </div>
  );
}

function Toast({
  notification,
  onDismiss,
}: {
  notification: AppNotification;
  onDismiss: (id: number) => void;
}) {
  const Icon = notification.tone === "success"
    ? CheckCircle2
    : notification.tone === "error"
      ? CircleAlert
      : Info;
  return (
    <div className={`toast toast--${notification.tone}`}>
      <Icon size={18} aria-hidden="true" />
      <div>
        <strong>{notification.title}</strong>
        {notification.message && <span>{notification.message}</span>}
        {notification.action && (
          <button className="toast__action" type="button" onClick={notification.action.onClick}>
            <FolderOpen size={14} aria-hidden="true" />
            {notification.action.label}
          </button>
        )}
      </div>
      <button className="toast__dismiss" type="button" onClick={() => onDismiss(notification.id)} aria-label={`Dismiss ${notification.title}`} title="Dismiss">
        <X size={15} aria-hidden="true" />
      </button>
    </div>
  );
}