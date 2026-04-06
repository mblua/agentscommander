import { Component, createSignal } from "solid-js";
import { isTauri } from "../../shared/platform";
import { SessionAPI, WindowAPI } from "../../shared/ipc";

const RootAgentBanner: Component = () => {
  const [creating, setCreating] = createSignal(false);

  const handleClick = async () => {
    if (creating()) return;
    setCreating(true);
    try {
      const session = await SessionAPI.createRootAgent();
      await SessionAPI.switch(session.id);
      if (isTauri) {
        await WindowAPI.ensureTerminal();
      }
    } catch (e) {
      console.error("[RootAgentBanner] Failed to create root agent session:", e);
    } finally {
      setCreating(false);
    }
  };

  return (
    <button class="root-agent-banner" onClick={handleClick} disabled={creating()} title="Open Root Agent session">
      <div class="root-agent-avatar">
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
          <path d="M12 2L2 7l10 5 10-5-10-5z" />
          <path d="M2 17l10 5 10-5" />
          <path d="M2 12l10 5 10-5" />
        </svg>
      </div>
      <div class="root-agent-text">
        <span class="root-agent-title">Agent's Commander</span>
        <span class="root-agent-subtitle">Root Agent</span>
      </div>
    </button>
  );
};

export default RootAgentBanner;
