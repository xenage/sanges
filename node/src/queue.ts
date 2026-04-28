export class AsyncQueue<T> {
  private items: Array<T | Error> = [];
  private waiters: Array<(value: T | Error) => void> = [];
  private closed?: Error;

  push(item: T): void {
    const waiter = this.waiters.shift();
    if (waiter) {
      waiter(item);
      return;
    }
    this.items.push(item);
  }

  fail(error: Error): void {
    this.closed = error;
    const waiters = this.waiters.splice(0);
    for (const waiter of waiters) {
      waiter(error);
    }
  }

  async next(): Promise<T> {
    const item = this.items.shift();
    if (item instanceof Error) {
      throw item;
    }
    if (item !== undefined) {
      return item;
    }
    if (this.closed) {
      throw this.closed;
    }
    const value = await new Promise<T | Error>((resolve) => this.waiters.push(resolve));
    if (value instanceof Error) {
      throw value;
    }
    return value;
  }
}
