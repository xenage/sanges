import WebSocket from "ws";

import { SagensError } from "./errors.js";
import { AsyncQueue } from "./queue.js";

export type WireObject = Record<string, unknown>;
export type ExecEvent = WireObject & { type: "exec_output" | "exec_exit"; request_id: string };
export type ShellEvent = WireObject & { type: "shell_output" | "shell_exit"; shell_id: string };

interface PendingResponse {
  expectedType: string;
  resolve: (response: WireObject) => void;
  reject: (error: Error) => void;
}

export class Transport {
  private nextId = 1;
  private pending = new Map<string, PendingResponse>();
  private execStreams = new Map<string, AsyncQueue<ExecEvent>>();
  private shellStreams = new Map<string, AsyncQueue<ShellEvent>>();
  private bufferedShellEvents = new Map<string, ShellEvent[]>();

  private constructor(
    public readonly endpoint: string,
    private readonly socket: WebSocket
  ) {}

  static async connect(
    endpoint: string,
    authMessage: WireObject,
    expectedPrincipal: WireObject
  ): Promise<Transport> {
    const socket = new WebSocket(endpoint);
    await waitForOpen(socket);
    const transport = new Transport(endpoint, socket);
    await transport.authenticate(authMessage, expectedPrincipal);
    socket.on("message", (data) => transport.dispatchFrame(data.toString()));
    socket.on("error", (error) => transport.failAll(error.message));
    socket.on("close", () => transport.failAll("websocket connection closed"));
    return transport;
  }

  close(): void {
    this.socket.close();
    this.failAll("websocket connection closed");
  }

  nextRequestId(): string {
    return String(this.nextId++);
  }

  requestResponse(request: WireObject, expectedType: string): Promise<WireObject> {
    const requestId = String(request.request_id);
    return new Promise((resolve, reject) => {
      this.pending.set(requestId, { expectedType, resolve, reject });
      try {
        this.sendJson({ type: "request", request });
      } catch (error) {
        this.pending.delete(requestId);
        reject(error);
      }
    });
  }

  openExecStream(request: WireObject): AsyncQueue<ExecEvent> {
    const requestId = String(request.request_id);
    const queue = new AsyncQueue<ExecEvent>();
    this.execStreams.set(requestId, queue);
    try {
      this.sendJson({ type: "request", request });
    } catch (error) {
      this.execStreams.delete(requestId);
      queue.fail(asError(error));
    }
    return queue;
  }

  registerShell(shellId: string): AsyncQueue<ShellEvent> {
    const queue = new AsyncQueue<ShellEvent>();
    this.shellStreams.set(shellId, queue);
    const buffered = this.bufferedShellEvents.get(shellId) ?? [];
    this.bufferedShellEvents.delete(shellId);
    for (const event of buffered) {
      queue.push(event);
    }
    return queue;
  }

  async sendShellRequest(request: WireObject): Promise<void> {
    await this.requestResponse(request, "ack");
  }

  private async authenticate(authMessage: WireObject, principal: WireObject): Promise<void> {
    const payload = await new Promise<WireObject>((resolve, reject) => {
      const onMessage = (data: WebSocket.RawData) => {
        cleanup();
        resolve(JSON.parse(data.toString()) as WireObject);
      };
      const onError = (error: Error) => {
        cleanup();
        reject(error);
      };
      const cleanup = () => {
        this.socket.off("message", onMessage);
        this.socket.off("error", onError);
      };
      this.socket.once("message", onMessage);
      this.socket.once("error", onError);
      this.sendJson(authMessage);
    });
    if (payload.type !== "authenticated") {
      throw new SagensError(`unexpected auth message: ${JSON.stringify(payload)}`);
    }
    if (JSON.stringify(payload.principal) !== JSON.stringify(principal)) {
      throw new SagensError(`unexpected auth principal: ${JSON.stringify(payload.principal)}`);
    }
  }

  private dispatchFrame(frame: string): void {
    const payload = JSON.parse(frame) as WireObject;
    if (payload.type === "event") {
      this.dispatchEvent(payload.event as WireObject);
    }
  }

  private dispatchEvent(event: WireObject): void {
    switch (event.type) {
      case "response":
        this.resolveResponse(String(event.request_id), event.response as WireObject);
        break;
      case "exec_output":
      case "exec_exit":
        this.pushExecEvent(event as ExecEvent);
        break;
      case "shell_output":
      case "shell_exit":
        this.pushShellEvent(event as ShellEvent);
        break;
      case "error":
        this.resolveError(event.request_id ? String(event.request_id) : null, String(event.message));
        break;
    }
  }

  private resolveResponse(requestId: string, response: WireObject): void {
    const pending = this.pending.get(requestId);
    this.pending.delete(requestId);
    if (!pending) {
      return;
    }
    if (response.type !== pending.expectedType) {
      pending.reject(new SagensError(`unexpected response type ${String(response.type)}`));
      return;
    }
    pending.resolve(response);
  }

  private pushExecEvent(event: ExecEvent): void {
    const queue = this.execStreams.get(event.request_id);
    if (event.type === "exec_exit") {
      this.execStreams.delete(event.request_id);
    }
    queue?.push(event);
  }

  private pushShellEvent(event: ShellEvent): void {
    const queue = this.shellStreams.get(event.shell_id);
    if (!queue) {
      const buffered = this.bufferedShellEvents.get(event.shell_id) ?? [];
      buffered.push(event);
      this.bufferedShellEvents.set(event.shell_id, buffered);
      return;
    }
    if (event.type === "shell_exit") {
      this.shellStreams.delete(event.shell_id);
    }
    queue.push(event);
  }

  private resolveError(requestId: string | null, message: string): void {
    const error = new SagensError(message);
    if (!requestId) {
      this.failAll(message);
      return;
    }
    const pending = this.pending.get(requestId);
    this.pending.delete(requestId);
    if (pending) {
      pending.reject(error);
      return;
    }
    const execQueue = this.execStreams.get(requestId);
    this.execStreams.delete(requestId);
    execQueue?.fail(error);
  }

  private failAll(message: string): void {
    const error = new SagensError(message);
    for (const pending of this.pending.values()) {
      pending.reject(error);
    }
    for (const queue of [...this.execStreams.values(), ...this.shellStreams.values()]) {
      queue.fail(error);
    }
    this.pending.clear();
    this.execStreams.clear();
    this.shellStreams.clear();
    this.bufferedShellEvents.clear();
  }

  private sendJson(payload: WireObject): void {
    this.socket.send(JSON.stringify(payload));
  }
}

function waitForOpen(socket: WebSocket): Promise<void> {
  return new Promise((resolve, reject) => {
    socket.once("open", () => resolve());
    socket.once("error", reject);
  });
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}
