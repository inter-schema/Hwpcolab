export class ActiveHttpRequests {
  private readonly controllers = new Map<string, AbortController>();

  register(requestId: string, controller: AbortController): void {
    this.controllers.set(requestId, controller);
  }

  cancel(requestId: string): boolean {
    const controller = this.controllers.get(requestId);
    if (!controller) return false;
    this.controllers.delete(requestId);
    controller.abort();
    return true;
  }

  cleanup(requestId: string, _controller: AbortController): void {
    this.controllers.delete(requestId);
  }
}
