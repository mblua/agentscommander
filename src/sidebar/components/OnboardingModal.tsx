import { Component, createSignal, onMount, Show } from "solid-js";
import type { AgentConfig, AppSettings } from "../../shared/types";
import { SettingsAPI } from "../../shared/ipc";
import { settingsStore } from "../../shared/stores/settings";

/* ── Agent presets (mirrored from SettingsModal) ── */
const AGENT_PRESETS: {
  key: string;
  label: string;
  description: string;
  color: string;
  config: Omit<AgentConfig, "id">;
}[] = [
  {
    key: "claude",
    label: "Claude Code",
    description: "AI coding agent by Anthropic",
    color: "#d97706",
    config: {
      label: "Claude Code",
      command: "claude --enable-auto-mode",
      color: "#d97706",
      gitPullBefore: false,
      excludeGlobalClaudeMd: true,
    },
  },
  {
    key: "codex",
    label: "Codex",
    description: "AI coding agent by OpenAI",
    color: "#10b981",
    config: {
      label: "Codex",
      command: "codex",
      color: "#10b981",
      gitPullBefore: false,
      excludeGlobalClaudeMd: false,
    },
  },
  {
    key: "gemini",
    label: "Gemini CLI",
    description: "AI coding agent by Google",
    color: "#4285f4",
    config: {
      label: "Gemini CLI",
      command: "gemini --approval-mode=yolo -m gemini-3-pro-preview",
      color: "#4285f4",
      gitPullBefore: false,
      excludeGlobalClaudeMd: false,
    },
  },
  {
    key: "custom",
    label: "Custom Agent",
    description: "Configure your own agent command",
    color: "#6366f1",
    config: {
      label: "",
      command: "",
      color: "#6366f1",
      gitPullBefore: false,
      excludeGlobalClaudeMd: true,
    },
  },
];

let idCounter = 0;
function newId(): string {
  return `agent_${Date.now()}_${idCounter++}`;
}

const OnboardingModal: Component<{ onClose: () => void }> = (props) => {
  const [selectedPreset, setSelectedPreset] = createSignal<string | null>(null);
  const [saving, setSaving] = createSignal(false);
  const [done, setDone] = createSignal(false);
  const [addedLabel, setAddedLabel] = createSignal("");

  // Custom agent fields
  const [customLabel, setCustomLabel] = createSignal("");
  const [customCommand, setCustomCommand] = createSignal("");

  const isCustom = () => selectedPreset() === "custom";
  const canConfirm = () => {
    if (!selectedPreset()) return false;
    if (isCustom()) return customLabel().trim() !== "" && customCommand().trim() !== "";
    return true;
  };

  const handleSelect = (key: string) => {
    setSelectedPreset(key === selectedPreset() ? null : key);
  };

  const handleConfirm = async () => {
    const key = selectedPreset();
    if (!key) return;

    const preset = AGENT_PRESETS.find((p) => p.key === key);
    if (!preset) return;

    setSaving(true);
    try {
      const settings = await SettingsAPI.get();

      let agent: AgentConfig;
      if (key === "custom") {
        agent = {
          id: newId(),
          label: customLabel().trim(),
          command: customCommand().trim(),
          color: preset.config.color,
          gitPullBefore: false,
          excludeGlobalClaudeMd: true,
        };
      } else {
        agent = { id: newId(), ...preset.config };
      }

      const updated: AppSettings = {
        ...settings,
        agents: [...settings.agents, agent],
      };
      await SettingsAPI.update(updated);
      settingsStore.refresh();

      setAddedLabel(agent.label);
      setDone(true);
    } catch (e) {
      console.error("Onboarding save failed:", e);
    } finally {
      setSaving(false);
    }
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") props.onClose();
  };

  const handleOverlayClick = (e: MouseEvent) => {
    if ((e.target as HTMLElement).classList.contains("modal-overlay")) props.onClose();
  };

  let overlayRef!: HTMLDivElement;
  onMount(() => overlayRef.focus());

  return (
    <div class="modal-overlay" ref={overlayRef} onClick={handleOverlayClick} onKeyDown={handleKeyDown} tabIndex={0}>
      <div class="agent-modal onboarding-modal">
        <div class="agent-modal-header">
          <span class="agent-modal-title">Welcome</span>
        </div>

        <div class="wizard-body onboarding-body">
          <Show when={!done()} fallback={
            <div class="onboarding-done">
              <div class="onboarding-done-icon">&#x2713;</div>
              <div class="onboarding-done-text">
                <strong>{addedLabel()}</strong> configured!
              </div>
              <div class="onboarding-done-hint">
                You can add more agents later in Settings.
              </div>
            </div>
          }>
            <p class="onboarding-welcome">
              Welcome to AgentsCommander! Let's set up your first AI coding agent.
            </p>

            <div class="onboarding-cards">
              {AGENT_PRESETS.map((preset) => (
                <button
                  class={`onboarding-card ${selectedPreset() === preset.key ? "selected" : ""}`}
                  onClick={() => handleSelect(preset.key)}
                  style={{ "--card-accent": preset.color }}
                >
                  <div
                    class="onboarding-card-icon"
                    style={{ background: preset.color }}
                  >
                    {preset.label[0]}
                  </div>
                  <div class="onboarding-card-info">
                    <div class="onboarding-card-name">{preset.label}</div>
                    <div class="onboarding-card-desc">{preset.description}</div>
                  </div>
                </button>
              ))}
            </div>

            <Show when={isCustom()}>
              <div class="onboarding-custom-fields">
                <label class="onboarding-field-label">
                  Agent name
                  <input
                    class="onboarding-field-input"
                    type="text"
                    placeholder="My Agent"
                    value={customLabel()}
                    onInput={(e) => setCustomLabel(e.currentTarget.value)}
                  />
                </label>
                <label class="onboarding-field-label">
                  Command
                  <input
                    class="onboarding-field-input"
                    type="text"
                    placeholder="my-agent --flag"
                    value={customCommand()}
                    onInput={(e) => setCustomCommand(e.currentTarget.value)}
                  />
                </label>
              </div>
            </Show>
          </Show>
        </div>

        <div class="new-agent-footer">
          <Show when={done()} fallback={
            <>
              <button class="new-agent-cancel-btn" onClick={props.onClose}>
                Skip
              </button>
              <button
                class="new-agent-create-btn"
                disabled={!canConfirm() || saving()}
                onClick={handleConfirm}
              >
                {saving() ? "Setting up..." : "Set up agent"}
              </button>
            </>
          }>
            <button class="new-agent-create-btn" onClick={props.onClose}>
              Get started
            </button>
          </Show>
        </div>
      </div>
    </div>
  );
};

export default OnboardingModal;
