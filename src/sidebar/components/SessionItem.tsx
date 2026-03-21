import { Component, createSignal, Show } from "solid-js";
import type { Session, SessionStatus } from "../../shared/types";
import { SessionAPI } from "../../shared/ipc";

function statusClass(status: SessionStatus): string {
  if (typeof status === "string") return status;
  return "exited";
}

const SessionItem: Component<{
  session: Session;
  isActive: boolean;
}> = (props) => {
  const [editing, setEditing] = createSignal(false);
  const [editValue, setEditValue] = createSignal("");
  let inputRef!: HTMLInputElement;

  const handleClick = () => {
    if (!editing()) {
      SessionAPI.switch(props.session.id);
    }
  };

  const handleDoubleClick = (e: MouseEvent) => {
    e.stopPropagation();
    setEditValue(props.session.name);
    setEditing(true);
    // Focus input after it renders
    requestAnimationFrame(() => {
      inputRef?.focus();
      inputRef?.select();
    });
  };

  const confirmRename = () => {
    const val = editValue().trim();
    if (val && val !== props.session.name) {
      SessionAPI.rename(props.session.id, val);
    }
    setEditing(false);
  };

  const cancelRename = () => {
    setEditing(false);
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter") {
      e.preventDefault();
      confirmRename();
    } else if (e.key === "Escape") {
      e.preventDefault();
      cancelRename();
    }
  };

  const handleClose = (e: MouseEvent) => {
    e.stopPropagation();
    SessionAPI.destroy(props.session.id);
  };

  return (
    <div
      class={`session-item session-item-enter ${props.isActive ? "active" : ""}`}
      onClick={handleClick}
    >
      <div
        class={`session-item-status ${statusClass(props.session.status)}`}
      />
      <div class="session-item-info">
        <Show
          when={editing()}
          fallback={
            <div class="session-item-name" onDblClick={handleDoubleClick}>
              {props.session.name}
            </div>
          }
        >
          <input
            ref={inputRef!}
            class="session-item-rename-input"
            value={editValue()}
            onInput={(e) => setEditValue(e.currentTarget.value)}
            onKeyDown={handleKeyDown}
            onBlur={confirmRename}
            maxLength={50}
            onClick={(e) => e.stopPropagation()}
          />
        </Show>
        <div class="session-item-shell">{props.session.shell}</div>
      </div>
      <button class="session-item-close" onClick={handleClose} title="Close session">
        &#x2715;
      </button>
    </div>
  );
};

export default SessionItem;
