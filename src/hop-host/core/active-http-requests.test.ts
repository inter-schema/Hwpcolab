import { describe, expect, it } from 'vitest';
import { ActiveHttpRequests } from './active-http-requests';

describe('ActiveHttpRequests', () => {
  it('rejects duplicate active request IDs without replacing the original controller', () => {
    const requests = new ActiveHttpRequests();
    const original = new AbortController();
    const duplicate = new AbortController();

    requests.register('same-id', original);

    expect(() => requests.register('same-id', duplicate)).toThrow(/already active/i);
    expect(requests.cancel('same-id')).toBe(true);
    expect(original.signal.aborted).toBe(true);
    expect(duplicate.signal.aborted).toBe(false);
  });

  it('does not let stale cleanup remove a replacement request after cancellation', () => {
    const requests = new ActiveHttpRequests();
    const previous = new AbortController();
    const replacement = new AbortController();

    requests.register('reused-id', previous);
    expect(requests.cancel('reused-id')).toBe(true);
    requests.register('reused-id', replacement);

    requests.cleanup('reused-id', previous);

    expect(requests.cancel('reused-id')).toBe(true);
    expect(replacement.signal.aborted).toBe(true);
  });
});
