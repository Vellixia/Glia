import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ErrorState } from "./error-state";

describe("ErrorState", () => {
  it("renders with default title", () => {
    render(<ErrorState />);
    expect(screen.getByRole("heading", { name: /something went wrong/i })).toBeInTheDocument();
  });

  it("renders custom title and description", () => {
    render(<ErrorState title="Custom Error" description="Something broke" />);
    expect(screen.getByRole("heading", { name: /custom error/i })).toBeInTheDocument();
    expect(screen.getByText(/something broke/i)).toBeInTheDocument();
  });

  it("renders retry button when onRetry is provided", () => {
    const onRetry = vi.fn();
    render(<ErrorState onRetry={onRetry} />);
    const button = screen.getByRole("button", { name: /try again/i });
    expect(button).toBeInTheDocument();
  });

  it("calls onRetry when button is clicked", async () => {
    const onRetry = vi.fn();
    const user = userEvent.setup();
    render(<ErrorState onRetry={onRetry} />);
    const button = screen.getByRole("button", { name: /try again/i });
    await user.click(button);
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it("does not render retry button when onRetry is omitted", () => {
    render(<ErrorState />);
    expect(screen.queryByRole("button", { name: /try again/i })).not.toBeInTheDocument();
  });

  it("shows error icon (AlertCircle) in a destructive circle", () => {
    const { container } = render(<ErrorState />);
    const circle = container.querySelector(".rounded-full");
    expect(circle).toBeInTheDocument();
    expect(circle?.classList.contains("bg-destructive/10")).toBe(true);
    const icon = container.querySelector("svg");
    expect(icon).toBeInTheDocument();
  });
});
