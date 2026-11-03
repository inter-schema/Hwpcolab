export class ActiveHttpRequests {
  private readonly controllers = new Map<string, AbortController>();

  register(requestId: string, controller: AbortController): void {
    if (this.controllers.has(requestId)) {
      throw new Error(`Request ID "${requestId}" is already active.`);
    }
    this.controllers.set(requestId, controller);
  }

  cancel(requestId: string): boolean {
    const controller = this.controllers.get(requestId);
    if (!controller) return false;
    this.controllers.delete(requestId);
    controller.abort();
    return true;
  }

  cleanup(requestId: string, controller: AbortController): void {
    if (this.controllers.get(requestId) === controller) {
      this.controllers.delete(requestId);
    }
  }
}
