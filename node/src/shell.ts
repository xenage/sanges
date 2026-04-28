import { Buffer } from "node:buffer";

import { AsyncQueue } from "./queue.js";
import { ShellEvent, Transport } from "./transport.js";

export class ShellOutputEvent {
  constructor(public readonly data: Buffer) {}

  get text(): string {
    return this.data.toString();
  }
}

export class ShellExitEvent {
  constructor(public readonly code: number) {}
}

export class BoxShell {
  constructor(
    public readonly shellId: string,
    private readonly transport: Transport,
    private readonly events: AsyncQueue<ShellEvent>
  ) {}

  async sendInput(data: Buffer | string): Promise<void> {
    const payload = Buffer.isBuffer(data) ? data : Buffer.from(data);
    await this.transport.sendShellRequest({
      type: "shell_input",
      request_id: this.transport.nextRequestId(),
      shell_id: this.shellId,
      data: payload.toString("base64")
    });
  }

  async resize(cols: number, rows: number): Promise<void> {
    await this.transport.sendShellRequest({
      type: "resize_shell",
      request_id: this.transport.nextRequestId(),
      shell_id: this.shellId,
      cols,
      rows
    });
  }

  async close(): Promise<void> {
    await this.transport.sendShellRequest({
      type: "close_shell",
      request_id: this.transport.nextRequestId(),
      shell_id: this.shellId
    });
  }

  async nextEvent(): Promise<ShellOutputEvent | ShellExitEvent> {
    const event = await this.events.next();
    if (event.type === "shell_output") {
      return new ShellOutputEvent(Buffer.from(String(event.data), "base64"));
    }
    return new ShellExitEvent(Number(event.code));
  }

  async *iterEvents(): AsyncIterable<ShellOutputEvent | ShellExitEvent> {
    while (true) {
      const event = await this.nextEvent();
      yield event;
      if (event instanceof ShellExitEvent) {
        return;
      }
    }
  }
}
