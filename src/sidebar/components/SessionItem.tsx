import { Component, createSignal, Show, For } from "solid-js";
import type { Session, SessionStatus, TelegramBotConfig } from "../../shared/types";
import { SessionAPI, TelegramAPI, SettingsAPI, WindowAPI, PtyAPI, VoiceAPI } from "../../shared/ipc";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { bridgesStore } from "../stores/bridges";
import { settingsStore } from "../stores/settings";

// Global recording state: only one session can record at a time
const [recordingSessionId, setRecordingSessionId] = createSignal<string | null>(null);
let activeMediaRecorder: MediaRecorder | null = null;
let audioChunks: Blob[] = [];

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
  const [showBotMenu, setShowBotMenu] = createSignal(false);
  const [availableBots, setAvailableBots] = createSignal<TelegramBotConfig[]>([]);
  let inputRef!: HTMLInputElement;

  const bridge = () => bridgesStore.getBridge(props.session.id);

  const handleTelegramClick = async (e: MouseEvent) => {
    e.stopPropagation();
    const b = bridge();
    if (b) {
      // Detach existing bridge
      await TelegramAPI.detach(props.session.id);
    } else {
      // Load bots and show menu (or auto-attach if only one)
      const settings = await SettingsAPI.get();
      const bots = settings.telegramBots || [];
      if (bots.length === 1) {
        await TelegramAPI.attach(props.session.id, bots[0].id);
      } else if (bots.length > 1) {
        setAvailableBots(bots);
        setShowBotMenu(true);
      }
    }
  };

  const handleBotSelect = async (botId: string) => {
    setShowBotMenu(false);
    await TelegramAPI.attach(props.session.id, botId);
  };

  const handleClick = async () => {
    if (!editing()) {
      await SessionAPI.switch(props.session.id);
      const detachedLabel = `terminal-${props.session.id.replace(/-/g, "")}`;
      const detachedWin = await WebviewWindow.getByLabel(detachedLabel);
      if (!detachedWin) {
        (await WebviewWindow.getByLabel("terminal"))?.setFocus();
      }
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

  const [micError, setMicError] = createSignal(false);

  const isRecording = () => recordingSessionId() === props.session.id;

  const stopRecording = () => {
    if (activeMediaRecorder && activeMediaRecorder.state !== "inactive") {
      activeMediaRecorder.stop();
    }
  };

  const startRecording = async () => {
    // Stop any other session's recording first
    if (recordingSessionId()) {
      stopRecording();
    }

    setMicError(false);

    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      audioChunks = [];

      const recorder = new MediaRecorder(stream);
      activeMediaRecorder = recorder;
      setRecordingSessionId(props.session.id);

      recorder.ondataavailable = (e) => {
        if (e.data.size > 0) audioChunks.push(e.data);
      };

      recorder.onstop = async () => {
        // Stop all tracks to release the mic
        stream.getTracks().forEach((t) => t.stop());
        setRecordingSessionId(null);
        activeMediaRecorder = null;

        if (audioChunks.length === 0) return;

        const blob = new Blob(audioChunks, { type: recorder.mimeType });
        const buffer = await blob.arrayBuffer();
        const bytes = Array.from(new Uint8Array(buffer));

        try {
          const text = await VoiceAPI.transcribe(bytes);
          if (text) {
            const encoder = new TextEncoder();
            await PtyAPI.write(props.session.id, encoder.encode(text));
          }
        } catch (err) {
          console.error("Voice transcription failed:", err);
          setMicError(true);
          setTimeout(() => setMicError(false), 3000);
        }
      };

      recorder.start();
    } catch (err) {
      console.error("Microphone access failed:", err);
      setMicError(true);
      setRecordingSessionId(null);
      setTimeout(() => setMicError(false), 3000);
    }
  };

  const handleMicClick = (e: MouseEvent) => {
    e.stopPropagation();
    if (isRecording()) {
      stopRecording();
    } else {
      void startRecording();
    }
  };

  const handleDetach = (e: MouseEvent) => {
    e.stopPropagation();
    WindowAPI.detach(props.session.id);
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
        class={`session-item-status ${props.session.waitingForInput ? "waiting" : statusClass(props.session.status)}`}
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
        <Show when={props.session.gitBranch}>
          <div class="session-item-branch" title={props.session.gitBranch!}>
            {props.session.gitBranch}
          </div>
        </Show>
        <div class="session-item-shell">{props.session.shell}</div>
      </div>
      <Show when={settingsStore.voiceEnabled}>
        <button
          class={`session-item-mic ${isRecording() ? "recording" : ""} ${micError() ? "error" : ""}`}
          onClick={handleMicClick}
          title={isRecording() ? "Stop recording" : micError() ? "Mic error" : "Voice to text"}
        >
          &#x1F399;
        </button>
      </Show>
      <button
        class="session-item-detach"
        onClick={handleDetach}
        title="Detach to own window"
      >
        &#x29C9;
      </button>
      <Show when={bridge()}>
        <div
          class="session-item-bridge-dot"
          style={{ background: bridge()!.color }}
          title={`Telegram: ${bridge()!.botLabel}`}
        />
      </Show>
      <button
        class={`session-item-telegram ${bridge() ? "active" : ""}`}
        onClick={handleTelegramClick}
        title={bridge() ? "Detach Telegram" : "Attach Telegram"}
        style={bridge() ? { color: bridge()!.color } : {}}
      >
        T
      </button>
      <Show when={showBotMenu()}>
        <div class="session-item-bot-menu" onClick={(e) => e.stopPropagation()}>
          <For each={availableBots()}>
            {(bot) => (
              <button
                class="session-item-bot-option"
                onClick={() => handleBotSelect(bot.id)}
              >
                <span class="settings-color-dot" style={{ background: bot.color }} />
                {bot.label}
              </button>
            )}
          </For>
        </div>
      </Show>
      <button class="session-item-close" onClick={handleClose} title="Close session">
        &#x2715;
      </button>
    </div>
  );
};

export default SessionItem;
