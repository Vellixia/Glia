import { describe, it, expect } from "vitest";
import { settingsSchema } from "./settings";

describe("settingsSchema", () => {
  it("accepts valid settings with all fields", () => {
    const result = settingsSchema.safeParse({
      theme: "dark",
      logLevel: "info",
      hubUrl: "https://hub.example.com",
    });
    expect(result.success).toBe(true);
  });

  it("accepts system theme", () => {
    const result = settingsSchema.safeParse({
      theme: "system",
      logLevel: "debug",
      hubUrl: "https://hub.example.com",
    });
    expect(result.success).toBe(true);
  });

  it("rejects invalid theme values", () => {
    const result = settingsSchema.safeParse({
      theme: "neon",
      logLevel: "info",
      hubUrl: "https://hub.example.com",
    });
    expect(result.success).toBe(false);
    if (!result.success) {
      expect(result.error.issues[0].path).toContain("theme");
    }
  });

  it("rejects invalid logLevel values", () => {
    const result = settingsSchema.safeParse({
      theme: "light",
      logLevel: "critical",
      hubUrl: "https://hub.example.com",
    });
    expect(result.success).toBe(false);
    if (!result.success) {
      expect(result.error.issues[0].path).toContain("logLevel");
    }
  });

  it("rejects non-url hubUrl", () => {
    const result = settingsSchema.safeParse({
      theme: "light",
      logLevel: "warn",
      hubUrl: "not-a-url",
    });
    expect(result.success).toBe(false);
    if (!result.success) {
      expect(result.error.issues[0].path).toContain("hubUrl");
    }
  });

  it("rejects missing required fields", () => {
    const result = settingsSchema.safeParse({});
    expect(result.success).toBe(false);
    if (!result.success) {
      const paths = result.error.issues.map((i) => i.path[0]);
      expect(paths).toContain("theme");
      expect(paths).toContain("logLevel");
      expect(paths).toContain("hubUrl");
    }
  });

  it("rejects missing theme", () => {
    const result = settingsSchema.safeParse({
      logLevel: "info",
      hubUrl: "https://hub.example.com",
    });
    expect(result.success).toBe(false);
  });

  it("rejects missing logLevel", () => {
    const result = settingsSchema.safeParse({
      theme: "dark",
      hubUrl: "https://hub.example.com",
    });
    expect(result.success).toBe(false);
  });

  it("rejects missing hubUrl", () => {
    const result = settingsSchema.safeParse({
      theme: "dark",
      logLevel: "info",
    });
    expect(result.success).toBe(false);
  });
});
