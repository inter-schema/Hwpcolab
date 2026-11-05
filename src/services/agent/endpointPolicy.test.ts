import { describe, it, expect } from "vitest";
import { validateCustomEndpoint } from "./endpointPolicy.ts";

describe("validateCustomEndpoint", () => {
  it("allows IPv6 loopback [::1]", () => {
    const res = validateCustomEndpoint("http://[::1]:11434");
    expect(res.ok).toBe(true);
    if (res.ok) {
      expect(res.normalized).toBe("http://[::1]:11434/chat/completions");
    }
  });

  it("rejects 172.15.x.x on http (not private range)", () => {
    const res = validateCustomEndpoint("http://172.15.0.1:8000/v1");
    expect(res.ok).toBe(false);
  });

  it("allows 172.16.x.x on http (start of RFC 1918 172.16/12)", () => {
    const res = validateCustomEndpoint("http://172.16.0.1:8000/v1");
    expect(res.ok).toBe(true);
    if (res.ok) {
      expect(res.normalized).toBe("http://172.16.0.1:8000/v1/chat/completions");
    }
  });

  it("allows 172.31.x.x on http (end of RFC 1918 172.16/12)", () => {
    const res = validateCustomEndpoint("http://172.31.255.1:8080");
    expect(res.ok).toBe(true);
    if (res.ok) {
      expect(res.normalized).toBe("http://172.31.255.1:8080/chat/completions");
    }
  });

  it("rejects 172.32.x.x on http (outside private range)", () => {
    const res = validateCustomEndpoint("http://172.32.0.1:8080");
    expect(res.ok).toBe(false);
  });

  it("allows 192.168.x.x on http (RFC 1918)", () => {
    const res = validateCustomEndpoint("http://192.168.1.100:11434/v1");
    expect(res.ok).toBe(true);
    if (res.ok) {
      expect(res.normalized).toBe("http://192.168.1.100:11434/v1/chat/completions");
    }
  });

  it("allows 10.x.x.x on http (RFC 1918)", () => {
    const res = validateCustomEndpoint("http://10.0.1.5:5000");
    expect(res.ok).toBe(true);
    if (res.ok) {
      expect(res.normalized).toBe("http://10.0.1.5:5000/chat/completions");
    }
  });

  it("rejects 169.254.169.254 (link-local / AWS metadata) on http", () => {
    const res = validateCustomEndpoint("http://169.254.169.254/latest/meta-data");
    expect(res.ok).toBe(false);
    if (!res.ok) {
      expect(res.reason).toContain("169.254");
    }
  });

  it("rejects 169.254.169.254 on https as well", () => {
    const res = validateCustomEndpoint("https://169.254.169.254/api");
    expect(res.ok).toBe(false);
    if (!res.ok) {
      expect(res.reason).toContain("169.254");
    }
  });

  it("allows localhost on http", () => {
    const res = validateCustomEndpoint("http://localhost:11434");
    expect(res.ok).toBe(true);
    if (res.ok) {
      expect(res.normalized).toBe("http://localhost:11434/chat/completions");
    }
  });

  it("handles uppercase hosts and schemes properly", () => {
    const res = validateCustomEndpoint("HTTP://LOCALHOST:11434/V1");
    expect(res.ok).toBe(true);
    if (res.ok) {
      expect(res.normalized).toBe("http://localhost:11434/V1/chat/completions");
    }
  });

  it("handles custom ports on https", () => {
    const res = validateCustomEndpoint("https://api.custom-ai.org:8443/v1");
    expect(res.ok).toBe(true);
    if (res.ok) {
      expect(res.normalized).toBe("https://api.custom-ai.org:8443/v1/chat/completions");
    }
  });

  it("rejects userinfo in url", () => {
    const res = validateCustomEndpoint("http://admin:secret@localhost:11434/v1");
    expect(res.ok).toBe(false);
    if (!res.ok) {
      expect(res.reason).toContain("자격증명");
    }
  });

  it("rejects non-http schemes (ftp, file, ws)", () => {
    expect(validateCustomEndpoint("ftp://localhost:21/api").ok).toBe(false);
    expect(validateCustomEndpoint("file:///etc/passwd").ok).toBe(false);
    expect(validateCustomEndpoint("ws://localhost:8000").ok).toBe(false);
  });

  it("does not duplicate /chat/completions if already present", () => {
    const res = validateCustomEndpoint("https://api.example.com/v1/chat/completions");
    expect(res.ok).toBe(true);
    if (res.ok) {
      expect(res.normalized).toBe("https://api.example.com/v1/chat/completions");
    }
  });

  it("allows 127.0.0.1 loopback", () => {
    const res = validateCustomEndpoint("http://127.0.0.1:8080/v1");
    expect(res.ok).toBe(true);
    if (res.ok) {
      expect(res.normalized).toBe("http://127.0.0.1:8080/v1/chat/completions");
    }
  });
});
