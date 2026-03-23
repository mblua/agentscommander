import { Component, onMount, onCleanup, createEffect } from "solid-js";
import { Terminal } from "@xterm/xterm";
import { WebglAddon } from "@xterm/addon-webgl";
import { FitAddon } from "@xterm/addon-fit";
import { PtyAPI, SessionAPI, onPtyOutput } from "../../shared/ipc";
import { terminalStore } from "../stores/terminal";
import type { UnlistenFn } from "@tauri-apps/api/event";
import "@xterm/xterm/css/xterm.css";

const TerminalView: Component = () => {
  let containerRef!: HTMLDivElement;
  let terminal: Terminal | null = null;
  let fitAddon: FitAddon | null = null;
  let unlistenPtyOutput: UnlistenFn | null = null;
  let resizeObserver: ResizeObserver | null = null;
  let currentSessionId: string | null = null;
  let inputBuffer = "";

  const updateSize = () => {
    if (terminal) {
      terminalStore.setTermSize(terminal.cols, terminal.rows);
    }
  };

  const syncViewport = (sessionId: string) => {
    if (!terminal || !fitAddon) {
      return;
    }

    fitAddon.fit();
    terminal.scrollToBottom();
    terminal.refresh(0, Math.max(terminal.rows - 1, 0));
    updateSize();
    void PtyAPI.resize(sessionId, terminal.cols, terminal.rows);
  };

  const scheduleViewportSync = (sessionId: string) => {
    requestAnimationFrame(() => {
      if (sessionId !== terminalStore.activeSessionId) {
        return;
      }

      syncViewport(sessionId);

      requestAnimationFrame(() => {
        if (sessionId === terminalStore.activeSessionId) {
          syncViewport(sessionId);
        }
      });
    });
  };

  const disposeTerminal = () => {
    resizeObserver?.disconnect();
    resizeObserver = null;
    terminal?.dispose();
    terminal = null;
    fitAddon = null;
    inputBuffer = "";
    currentSessionId = null;

    if (containerRef) {
      containerRef.replaceChildren();
    }
  };

  const initTerminal = (sessionId: string) => {
    terminal = new Terminal({
      fontFamily: "'Cascadia Code', 'JetBrains Mono', 'Fira Code', monospace",
      fontSize: 14,
      lineHeight: 1.2,
      cursorBlink: true,
      cursorStyle: "block",
      scrollback: 10000,
      theme: {
        background: "#0a0a0f",
        foreground: "#e8e8e8",
        cursor: "#00d4ff",
        selectionBackground: "rgba(0, 212, 255, 0.25)",
        black: "#1a1a2e",
        red: "#ff3b5c",
        green: "#33ff99",
        yellow: "#ffcc33",
        blue: "#3399ff",
        magenta: "#ff33cc",
        cyan: "#33ccff",
        white: "#e8e8e8",
        brightBlack: "#4a4a5e",
        brightRed: "#ff6699",
        brightGreen: "#66ffbb",
        brightYellow: "#ffdd66",
        brightBlue: "#66bbff",
        brightMagenta: "#ff66dd",
        brightCyan: "#66ddff",
        brightWhite: "#ffffff",
      },
      allowTransparency: false,
    });

    fitAddon = new FitAddon();
    terminal.loadAddon(fitAddon);
    terminal.open(containerRef);

    try {
      const webglAddon = new WebglAddon();
      webglAddon.onContextLoss(() => {
        webglAddon.dispose();
      });
      terminal.loadAddon(webglAddon);
    } catch {
      // Canvas renderer fallback is automatic.
    }

    syncViewport(sessionId);

    terminal.onData((data) => {
      const activeSessionId = terminalStore.activeSessionId;
      if (activeSessionId) {
        const encoder = new TextEncoder();
        void PtyAPI.write(activeSessionId, encoder.encode(data));
      }

      if (data === "\r") {
        const trimmed = inputBuffer.trim();
        if (trimmed && activeSessionId) {
          void SessionAPI.setLastPrompt(activeSessionId, trimmed);
        }
        inputBuffer = "";
      } else if (data === "\x7f") {
        inputBuffer = inputBuffer.slice(0, -1);
      } else if (data.length === 1 && data >= " ") {
        inputBuffer += data;
      } else if (data.length > 1 && !data.startsWith("\x1b")) {
        inputBuffer += data;
      }
    });

    terminal.onResize(({ cols, rows }) => {
      const activeSessionId = terminalStore.activeSessionId;
      if (activeSessionId) {
        void PtyAPI.resize(activeSessionId, cols, rows);
      }
      terminalStore.setTermSize(cols, rows);
    });

    resizeObserver = new ResizeObserver(() => {
      const activeSessionId = terminalStore.activeSessionId;
      if (activeSessionId) {
        syncViewport(activeSessionId);
      }
    });
    resizeObserver.observe(containerRef);
  };

  onMount(async () => {
    unlistenPtyOutput = await onPtyOutput(({ sessionId, data }) => {
      if (sessionId === terminalStore.activeSessionId && terminal) {
        terminal.write(new Uint8Array(data));
      }
    });
  });

  createEffect(() => {
    const sessionId = terminalStore.activeSessionId;
    if (!sessionId) {
      disposeTerminal();
      return;
    }

    if (sessionId !== currentSessionId) {
      disposeTerminal();
      initTerminal(sessionId);
      currentSessionId = sessionId;
    }

    scheduleViewportSync(sessionId);
  });

  onCleanup(() => {
    unlistenPtyOutput?.();
    disposeTerminal();
  });

  return <div class="terminal-container" ref={containerRef!} />;
};

export default TerminalView;
