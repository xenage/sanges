export class SagensError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "SagensError";
  }
}
