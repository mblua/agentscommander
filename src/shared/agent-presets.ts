import type { AgentConfig } from "./types";

export interface AgentPreset {
  key: string;
  label: string;
  description: string;
  color: string;
  config: Omit<AgentConfig, "id">;
}

export const AGENT_PRESETS: AgentPreset[] = [
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
];

/** Record-based lookup for SettingsModal quick-add buttons */
export const AGENT_PRESET_MAP: Record<string, Omit<AgentConfig, "id">> =
  Object.fromEntries(AGENT_PRESETS.map((p) => [p.key, p.config]));

let idCounter = 0;
export function newAgentId(): string {
  return `agent_${Date.now()}_${idCounter++}`;
}
