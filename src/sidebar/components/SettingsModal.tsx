import { Component, createSignal, For, onMount } from "solid-js";
import type { AppSettings, ActionButton } from "../../shared/types";
import { SettingsAPI } from "../../shared/ipc";

const SettingsModal: Component<{ onClose: () => void }> = (props) => {
  const [settings, setSettings] = createSignal<AppSettings | null>(null);
  const [saving, setSaving] = createSignal(false);

  onMount(async () => {
    const s = await SettingsAPI.get();
    setSettings(s);
  });

  const updateField = <K extends keyof AppSettings>(
    key: K,
    value: AppSettings[K]
  ) => {
    const s = settings();
    if (s) setSettings({ ...s, [key]: value });
  };

  const updateButton = (index: number, field: keyof ActionButton, value: string) => {
    const s = settings();
    if (!s) return;
    const buttons = [...s.actionButtons];
    buttons[index] = { ...buttons[index], [field]: value };
    updateField("actionButtons", buttons);
  };

  const updateButtonArgs = (index: number, value: string) => {
    const s = settings();
    if (!s) return;
    const buttons = [...s.actionButtons];
    buttons[index] = {
      ...buttons[index],
      args: value
        .split(" ")
        .map((a) => a.trim())
        .filter(Boolean),
    };
    updateField("actionButtons", buttons);
  };

  const handleSave = async () => {
    const s = settings();
    if (!s) return;
    setSaving(true);
    await SettingsAPI.update(s);
    setSaving(false);
    props.onClose();
  };

  const handleOverlayClick = (e: MouseEvent) => {
    if ((e.target as HTMLElement).classList.contains("modal-overlay")) {
      props.onClose();
    }
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") props.onClose();
  };

  return (
    <div class="modal-overlay" onClick={handleOverlayClick} onKeyDown={handleKeyDown} tabIndex={-1}>
      <div class="modal-container">
        <div class="modal-header">
          <span class="modal-title">Settings</span>
          <button class="modal-close" onClick={props.onClose}>
            &#x2715;
          </button>
        </div>

        {settings() && (
          <div class="modal-body">
            {/* General */}
            <div class="settings-section">
              <div class="settings-section-title">General</div>
              <label class="settings-field">
                <span class="settings-label">Default Shell</span>
                <input
                  class="settings-input"
                  value={settings()!.defaultShell}
                  onInput={(e) => updateField("defaultShell", e.currentTarget.value)}
                />
              </label>
              <label class="settings-field">
                <span class="settings-label">Shell Arguments</span>
                <input
                  class="settings-input"
                  value={settings()!.defaultShellArgs.join(" ")}
                  onInput={(e) =>
                    updateField(
                      "defaultShellArgs",
                      e.currentTarget.value.split(" ").filter(Boolean)
                    )
                  }
                />
              </label>
            </div>

            {/* Action Buttons */}
            <div class="settings-section">
              <div class="settings-section-title">Action Buttons</div>
              <For each={settings()!.actionButtons}>
                {(btn, i) => (
                  <div class="settings-button-card">
                    <div class="settings-button-card-header">
                      <div
                        class="settings-color-dot"
                        style={{ background: btn.color }}
                      />
                      <span>{btn.label}</span>
                    </div>
                    <label class="settings-field">
                      <span class="settings-label">Label</span>
                      <input
                        class="settings-input"
                        value={btn.label}
                        onInput={(e) =>
                          updateButton(i(), "label", e.currentTarget.value)
                        }
                      />
                    </label>
                    <label class="settings-field">
                      <span class="settings-label">Command</span>
                      <input
                        class="settings-input"
                        value={btn.command}
                        onInput={(e) =>
                          updateButton(i(), "command", e.currentTarget.value)
                        }
                      />
                    </label>
                    <label class="settings-field">
                      <span class="settings-label">Arguments</span>
                      <input
                        class="settings-input"
                        value={btn.args.join(" ")}
                        onInput={(e) =>
                          updateButtonArgs(i(), e.currentTarget.value)
                        }
                      />
                    </label>
                    <label class="settings-field">
                      <span class="settings-label">Working Directory</span>
                      <input
                        class="settings-input"
                        value={btn.workingDirectory}
                        onInput={(e) =>
                          updateButton(i(), "workingDirectory", e.currentTarget.value)
                        }
                      />
                    </label>
                    <label class="settings-field">
                      <span class="settings-label">Color</span>
                      <div class="settings-color-row">
                        <input
                          type="color"
                          class="settings-color-picker"
                          value={btn.color}
                          onInput={(e) =>
                            updateButton(i(), "color", e.currentTarget.value)
                          }
                        />
                        <input
                          class="settings-input settings-input-sm"
                          value={btn.color}
                          onInput={(e) =>
                            updateButton(i(), "color", e.currentTarget.value)
                          }
                        />
                      </div>
                    </label>
                  </div>
                )}
              </For>
            </div>
          </div>
        )}

        <div class="modal-footer">
          <button class="modal-btn modal-btn-cancel" onClick={props.onClose}>
            Cancel
          </button>
          <button
            class="modal-btn modal-btn-save"
            onClick={handleSave}
            disabled={saving()}
          >
            {saving() ? "Saving..." : "Save"}
          </button>
        </div>
      </div>
    </div>
  );
};

export default SettingsModal;
