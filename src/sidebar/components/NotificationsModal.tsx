import { Component, For, Show, createSignal, onCleanup } from "solid-js";
import { sessionsStore } from "../stores/sessions";
import { SessionAPI } from "../../shared/ipc";

const NotificationsModal: Component<{ onClose: () => void }> = (props) => {
  // Mark all as read when opening
  sessionsStore.markAllRead();

  // Reactive timer for elapsed time display
  const [now, setNow] = createSignal(Date.now());
  const timer = setInterval(() => setNow(Date.now()), 60000);
  onCleanup(() => clearInterval(timer));

  const formatTime = (ts: number) => {
    const mins = Math.floor((now() - ts) / 60000);
    if (mins < 1) return "just now";
    if (mins < 60) return `${mins}m ago`;
    const hours = Math.floor(mins / 60);
    if (hours < 24) return `${hours}h ago`;
    return `${Math.floor(hours / 24)}d ago`;
  };

  const typeLabel = (type: string) => {
    if (type === "agent_hung") return "Hung";
    if (type === "agent_completed") return "Completed";
    return type;
  };

  const typeClass = (type: string) => {
    if (type === "agent_hung") return "notif-type-hung";
    if (type === "agent_completed") return "notif-type-completed";
    return "";
  };

  return (
    <div class="modal-overlay" onClick={props.onClose}>
      <div class="modal-container notif-modal" onClick={(e) => e.stopPropagation()}>
        <div class="modal-header">
          <span class="modal-title">Notifications</span>
          <div class="notif-header-actions">
            <Show when={sessionsStore.notifications.length > 0}>
              <button
                class="notif-clear-all"
                onClick={() => sessionsStore.clearAllNotifications()}
              >
                Clear all
              </button>
            </Show>
            <button class="modal-close" onClick={props.onClose}>&times;</button>
          </div>
        </div>
        <div class="modal-body">
          <Show
            when={sessionsStore.notifications.length > 0}
            fallback={<div class="notif-empty">No notifications</div>}
          >
            <div class="notif-list">
              <For each={sessionsStore.notifications}>
                {(notif) => (
                  <div class="notif-item">
                    <div class="notif-item-header">
                      <span class={`notif-type-badge ${typeClass(notif.type)}`}>
                        {typeLabel(notif.type)}
                      </span>
                      <span class="notif-session-name">{notif.sessionName}</span>
                      <span class="notif-time">{formatTime(notif.timestamp)}</span>
                    </div>
                    <div class="notif-item-body">{notif.message}</div>
                    <div class="notif-item-actions">
                      <button
                        class="notif-action-btn"
                        onClick={() => {
                          SessionAPI.switch(notif.sessionId);
                          props.onClose();
                        }}
                      >
                        Switch to session
                      </button>
                      <button
                        class="notif-action-btn notif-action-dismiss"
                        onClick={() => sessionsStore.clearNotification(notif.id)}
                      >
                        Dismiss
                      </button>
                    </div>
                  </div>
                )}
              </For>
            </div>
          </Show>
        </div>
      </div>
    </div>
  );
};

export default NotificationsModal;
