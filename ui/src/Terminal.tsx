import { onCleanup, onMount } from "solid-js";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { decodeB64ToBytes, onPtyEvent, resizePty, writePty } from "./lib/ipc";

export interface TerminalProps {
  sessionId: string;
}

export default function Terminal(props: TerminalProps) {
  let host!: HTMLDivElement;
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

    // Push initial size to the PTY so prompts redraw correctly.
    await resizePty(props.sessionId, xterm.cols, xterm.rows);

    unlisten = await onPtyEvent((e) => {
      if (e.session_id !== props.sessionId) return;
      if (e.kind === "data") {
        const bytes = decodeB64ToBytes(e.data_b64);
        xterm.write(bytes);
      } else if (e.kind === "exit") {
        xterm.write("\r\n[session exited]\r\n");
      }
    });

    xterm.onData((data) => {
      void writePty(props.sessionId, data);
    });

    const resizeObserver = new ResizeObserver(() => {
      fit.fit();
      void resizePty(props.sessionId, xterm.cols, xterm.rows);
    });
    resizeObserver.observe(host);

    onCleanup(() => {
      resizeObserver.disconnect();
      unlisten?.();
      xterm.dispose();
    });
  });

  return <div class="terminal-host" ref={host} />;
}
