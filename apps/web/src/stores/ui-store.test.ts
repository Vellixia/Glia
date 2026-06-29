import { describe, it, expect } from "vitest";
import { useUIStore } from "./ui-store";

describe("useUIStore", () => {
  it("starts with sidebarOpen=true", () => {
    const { sidebarOpen } = useUIStore.getState();
    expect(sidebarOpen).toBe(true);
  });

  it("toggleSidebar flips sidebarOpen to false", () => {
    useUIStore.setState({ sidebarOpen: true });
    useUIStore.getState().toggleSidebar();
    expect(useUIStore.getState().sidebarOpen).toBe(false);
  });

  it("toggleSidebar flips sidebarOpen back to true", () => {
    useUIStore.setState({ sidebarOpen: false });
    useUIStore.getState().toggleSidebar();
    expect(useUIStore.getState().sidebarOpen).toBe(true);
  });

  it("setSidebarOpen sets explicitly to false", () => {
    useUIStore.setState({ sidebarOpen: true });
    useUIStore.getState().setSidebarOpen(false);
    expect(useUIStore.getState().sidebarOpen).toBe(false);
  });

  it("setSidebarOpen sets explicitly to true", () => {
    useUIStore.setState({ sidebarOpen: false });
    useUIStore.getState().setSidebarOpen(true);
    expect(useUIStore.getState().sidebarOpen).toBe(true);
  });
});
