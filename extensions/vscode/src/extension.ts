// Shares the visible editor state with the Context desktop app over a local
// WebSocket, so answers can use the real project (docs/PLAN.md 8). It reads
// nothing else and has no commands. Its hover provider shows nothing: VS
// Code calls it when the mouse rests on a word, which tells Context exactly
// what the user is pointing at.
import { readFile } from "node:fs/promises";
import { homedir } from "node:os";
import * as vscode from "vscode";
import {
  bridgeFilePath,
  capOpenFiles,
  capRanges,
  capSelection,
  nextDelay,
  pointerAt,
  visibleText,
  type HelloMessage,
  type Pointer,
  type PointerMessage,
  type StateMessage,
} from "./bridge";

/** Starting values (docs/PLAN.md 8). */
const DEBOUNCE_MS = 150;
const CONNECT_TIMEOUT_MS = 5000;

export function activate(context: vscode.ExtensionContext): void {
  context.subscriptions.push(new BridgeClient());
}

export function deactivate(): void {}

class BridgeClient implements vscode.Disposable {
  private socket: WebSocket | undefined;
  private attempt = 0;
  private reconnect: NodeJS.Timeout | undefined;
  private debounce: NodeJS.Timeout | undefined;
  private disposed = false;
  private pointer: Pointer | null = null;
  private readonly status = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Right,
    0,
  );
  private readonly listeners: vscode.Disposable[];

  constructor() {
    const changed = () => this.schedule();
    this.listeners = [
      vscode.window.onDidChangeActiveTextEditor(changed),
      vscode.window.onDidChangeVisibleTextEditors(changed),
      vscode.window.onDidChangeTextEditorVisibleRanges((event) => {
        this.forgetPointer(event.textEditor.document.uri);
        changed();
      }),
      vscode.window.onDidChangeTextEditorSelection(changed),
      vscode.window.onDidChangeWindowState(changed),
      vscode.workspace.onDidChangeWorkspaceFolders(changed),
      vscode.workspace.onDidChangeTextDocument((event) => {
        if (event.contentChanges.length > 0)
          this.forgetPointer(event.document.uri);
        if (event.document === vscode.window.activeTextEditor?.document)
          changed();
      }),
      vscode.languages.registerHoverProvider(
        { scheme: "file" },
        {
          provideHover: (document, position) => {
            const range = document.getWordRangeAtPosition(position);
            this.setPointer(
              pointerAt(
                document.uri.fsPath,
                position.line,
                position.character,
                range ? document.getText(range) : "",
                document.lineAt(position.line).text,
                Date.now(),
              ),
            );
            return undefined;
          },
        },
      ),
    ];
    this.status.name = "Context";
    this.showConnected(false);
    void this.connect();
  }

  private async connect(): Promise<void> {
    if (this.disposed) return;
    let port: number;
    let token: string;
    try {
      const path = bridgeFilePath(process.platform, homedir(), process.env);
      ({ port, token } = JSON.parse(await readFile(path, "utf8")));
    } catch {
      return this.retry(); // Context isn't running (yet).
    }

    const socket = new WebSocket(`ws://127.0.0.1:${port}`);
    this.socket = socket;
    // A refused connection fires only "error" (not "close") and can stay
    // CONNECTING, so every way of failing ends up here, once.
    const drop = () => {
      clearTimeout(timeout);
      if (this.socket !== socket) return;
      this.socket = undefined;
      socket.close();
      this.showConnected(false);
      this.retry();
    };
    const timeout = setTimeout(drop, CONNECT_TIMEOUT_MS);
    socket.addEventListener("error", drop);
    socket.addEventListener("close", drop);
    socket.addEventListener("open", () => {
      clearTimeout(timeout);
      this.attempt = 0;
      const hello: HelloMessage = {
        type: "hello",
        token,
        vscodeVersion: vscode.version,
        uriScheme: vscode.env.uriScheme,
      };
      socket.send(JSON.stringify(hello));
      this.showConnected(true);
      this.send();
      this.sendPointer();
    });
  }

  private retry(): void {
    if (this.disposed) return;
    clearTimeout(this.reconnect);
    this.reconnect = setTimeout(
      () => void this.connect(),
      nextDelay(this.attempt++),
    );
  }

  private schedule(): void {
    clearTimeout(this.debounce);
    this.debounce = setTimeout(() => this.send(), DEBOUNCE_MS);
  }

  private send(): void {
    if (this.socket?.readyState === WebSocket.OPEN) {
      this.socket.send(JSON.stringify(currentState()));
    }
  }

  /** Sent at once: the user may be about to release the hotkey. */
  private setPointer(pointer: Pointer | null): void {
    this.pointer = pointer;
    this.sendPointer();
  }

  private forgetPointer(uri: vscode.Uri): void {
    if (this.pointer && this.pointer.file === uri.fsPath) this.setPointer(null);
  }

  private sendPointer(): void {
    if (this.socket?.readyState === WebSocket.OPEN) {
      const message: PointerMessage = {
        type: "pointer",
        pointer: this.pointer,
      };
      this.socket.send(JSON.stringify(message));
    }
  }

  private showConnected(connected: boolean): void {
    this.status.text = connected ? "$(eye) Context" : "$(eye-closed) Context";
    this.status.tooltip = connected
      ? "Context can read this project when you point at it."
      : "Context isn't running.";
    this.status.show();
  }

  dispose(): void {
    this.disposed = true;
    clearTimeout(this.reconnect);
    clearTimeout(this.debounce);
    this.socket?.close();
    for (const listener of this.listeners) listener.dispose();
    this.status.dispose();
  }
}

function currentState(): StateMessage {
  const editor = vscode.window.activeTextEditor;
  const document =
    editor?.document.uri.scheme === "file" ? editor.document : undefined;
  const ranges =
    editor && document
      ? capRanges(
          editor.visibleRanges.map((r) => ({
            start: r.start.line,
            end: r.end.line,
          })),
        )
      : [];
  const openFiles = vscode.window.tabGroups.all
    .flatMap((group) => group.tabs)
    .map((tab) => tab.input)
    .filter(
      (input): input is vscode.TabInputText =>
        input instanceof vscode.TabInputText,
    )
    .filter((input) => input.uri.scheme === "file")
    .map((input) => input.uri.fsPath);

  return {
    type: "state",
    focused: vscode.window.state.focused,
    workspaceFolders: (vscode.workspace.workspaceFolders ?? [])
      .filter((folder) => folder.uri.scheme === "file")
      .map((folder) => folder.uri.fsPath),
    activeFile: document?.uri.fsPath ?? null,
    visibleRanges: ranges,
    visibleText: document
      ? visibleText((line) => document.lineAt(line).text, ranges)
      : "",
    selections:
      editor && document
        ? editor.selections
            .filter((s) => !s.isEmpty)
            .map((s) => ({
              startLine: s.start.line,
              endLine: s.end.line,
              text: capSelection(document.getText(s)),
            }))
        : [],
    openFiles: capOpenFiles(openFiles),
  };
}
