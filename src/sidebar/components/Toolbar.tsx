import { Component, createSignal, For, onMount } from "solid-js";
import type { ActionButton } from "../../shared/types";
import { SessionAPI, SettingsAPI } from "../../shared/ipc";
import SettingsModal from "./SettingsModal";

const Toolbar: Component = () => {
  const [buttons, setButtons] = createSignal<ActionButton[]>([]);
  const [showSettings, setShowSettings] = createSignal(false);

  const loadButtons = async () => {
    const settings = await SettingsAPI.get();
    setButtons(settings.actionButtons);
  };

  onMount(loadButtons);

  const handleNewSession = () => {
    SessionAPI.create();
  };

  const handleActionButton = (btn: ActionButton) => {
    const cwd = btn.workingDirectory === "~" ? undefined : btn.workingDirectory;
    SessionAPI.create({
      shell: btn.command,
      shellArgs: btn.args,
      cwd,
      sessionName: btn.label,
    });
  };

  return (
    <>
      <div class="toolbar-section">
        <For each={buttons()}>
          {(btn) => (
            <button
              class="toolbar-action-btn"
              style={{ "--btn-color": btn.color }}
              onClick={() => handleActionButton(btn)}
              title={`Launch ${btn.label}`}
            >
              {btn.label}
            </button>
          )}
        </For>
      </div>
      <div class="toolbar">
        <button class="toolbar-btn" onClick={handleNewSession}>
          + New Session
        </button>
        <button
          class="toolbar-gear-btn"
          onClick={() => setShowSettings(true)}
          title="Settings"
        >
          &#x2699;
        </button>
      </div>
      {showSettings() && (
        <SettingsModal
          onClose={() => {
            setShowSettings(false);
            loadButtons();
          }}
        />
      )}
    </>
  );
};

export default Toolbar;
