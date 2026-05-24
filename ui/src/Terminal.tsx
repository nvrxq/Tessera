import { onCleanup, onMount } from "solid-js";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import {
  decodeB64ToBytes,
  killPty,
  onPtyEvent,
  resizePty,
  spawnShell,
  writePty,
} from "./lib/ipc";

export interface TerminalProps {
  cwd: string;
}

export default function Terminal(props: TerminalProps) {
  let host!: HTMLDivElement;
  let sessionId: string | null = null;
  let unlisten: (() => void) | null = null;

  onMount(async () => {
    const xterm = new XTerm({
      fontFamily: "ui-monospace, Menlo, monospace",
      fontSize: 13,
      theme: { background: "#1e1e1e", foreground: "#ddd" },
      convertEol: true,
    });
    const fit = new FitAddon();
    xterm.loadAddon(fit);
    xterm.open(host);
    fit.fit();

    const { cols, rows } = xterm;
    const resp = await spawnShell(props.cwd, cols, rows);
    sessionId = resp.session_id;

    unlisten = await onPtyEvent((e) => {
      if (e.session_id !== sessionId) return;
      if (e.kind === "data") {
        const bytes = decodeB64ToBytes(e.data_b64);
        xterm.write(bytes);
      } else if (e.kind === "exit") {
        xterm.write("\r\n[session exited]\r\n");
      }
    });

    xterm.onData((data) => {
      if (sessionId) void writePty(sessionId, data);
    });

    const resizeObserver = new ResizeObserver(() => {
      fit.fit();
      if (sessionId) void resizePty(sessionId, xterm.cols, xterm.rows);
    });
    resizeObserver.observe(host);

    onCleanup(() => {
      resizeObserver.disconnect();
      unlisten?.();
      if (sessionId) void killPty(sessionId);
      xterm.dispose();
    });
  });

  return <div class="terminal-host" ref={host} />;
}
