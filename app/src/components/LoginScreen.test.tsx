import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const login = vi.hoisted(() => vi.fn());
vi.mock("../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/api")>()),
  login,
}));

import { LoginScreen } from "./LoginScreen";
import type { Profile } from "../lib/api";

const profile: Profile = {
  user_id: "@bob:example.org",
  device_id: "HZTIUXZKUU",
  homeserver: "https://example.org/",
  display_name: "Bob",
  avatar_url: null,
};

function fields() {
  return {
    homeserver: screen.getByLabelText(/homeserver/i),
    username: screen.getByLabelText(/username/i),
    password: screen.getByLabelText(/password/i),
    // The label becomes "Signing in…" while a request is in flight, so the
    // matcher has to cover both states.
    submit: screen.getByRole("button", { name: /sign(ing)? in/i }),
  };
}

async function fillIn(user: ReturnType<typeof userEvent.setup>) {
  const { homeserver, username, password } = fields();
  await user.type(homeserver, "example.org");
  await user.type(username, "bob");
  await user.type(password, "hunter2");
}

describe("LoginScreen", () => {
  beforeEach(() => {
    login.mockReset();
  });

  it("focuses the homeserver field, which is the first thing to fill in", () => {
    render(<LoginScreen onSignedIn={vi.fn()} />);

    expect(fields().homeserver).toHaveFocus();
  });

  it("gives every input a name so a password manager can save the login", () => {
    // Without `name`, browsers and password managers offer to save nothing.
    render(<LoginScreen onSignedIn={vi.fn()} />);

    expect(fields().homeserver).toHaveAttribute("name", "homeserver");
    expect(fields().username).toHaveAttribute("name", "username");
    expect(fields().password).toHaveAttribute("name", "password");
  });

  it("sets the autocomplete hints the browser needs", () => {
    render(<LoginScreen onSignedIn={vi.fn()} />);

    expect(fields().username).toHaveAttribute("autocomplete", "username");
    expect(fields().password).toHaveAttribute("autocomplete", "current-password");
  });

  it("masks the password field", () => {
    render(<LoginScreen onSignedIn={vi.fn()} />);

    expect(fields().password).toHaveAttribute("type", "password");
  });

  it("keeps submit disabled until all three fields have something in them", async () => {
    const user = userEvent.setup();
    render(<LoginScreen onSignedIn={vi.fn()} />);

    expect(fields().submit).toBeDisabled();

    await user.type(fields().homeserver, "example.org");
    expect(fields().submit).toBeDisabled();

    await user.type(fields().username, "bob");
    expect(fields().submit).toBeDisabled();

    await user.type(fields().password, "hunter2");
    expect(fields().submit).toBeEnabled();
  });

  it("treats a whitespace-only server or username as empty", async () => {
    const user = userEvent.setup();
    render(<LoginScreen onSignedIn={vi.fn()} />);

    await user.type(fields().homeserver, "   ");
    await user.type(fields().username, "   ");
    await user.type(fields().password, "hunter2");

    expect(fields().submit).toBeDisabled();
  });

  it("passes what was typed to the login command", async () => {
    const user = userEvent.setup();
    login.mockResolvedValue(profile);
    render(<LoginScreen onSignedIn={vi.fn()} />);

    await fillIn(user);
    await user.click(fields().submit);

    await waitFor(() =>
      expect(login).toHaveBeenCalledWith("example.org", "bob", "hunter2"),
    );
  });

  it("hands the profile up when the login succeeds", async () => {
    const user = userEvent.setup();
    const onSignedIn = vi.fn();
    login.mockResolvedValue(profile);
    render(<LoginScreen onSignedIn={onSignedIn} />);

    await fillIn(user);
    await user.click(fields().submit);

    await waitFor(() => expect(onSignedIn).toHaveBeenCalledWith(profile));
  });

  it("shows the person-facing message when the login fails", async () => {
    const user = userEvent.setup();
    login.mockRejectedValue({
      message: "Incorrect username or password.",
      detail: "M_FORBIDDEN: unknown user",
    });
    vi.spyOn(console, "error").mockImplementation(() => {});
    render(<LoginScreen onSignedIn={vi.fn()} />);

    await fillIn(user);
    await user.click(fields().submit);

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Incorrect username or password.");
  });

  it("never shows the raw server detail to the user", async () => {
    const user = userEvent.setup();
    login.mockRejectedValue({
      message: "Incorrect username or password.",
      detail: "M_FORBIDDEN: unknown user",
    });
    vi.spyOn(console, "error").mockImplementation(() => {});
    render(<LoginScreen onSignedIn={vi.fn()} />);

    await fillIn(user);
    await user.click(fields().submit);

    await screen.findByRole("alert");
    expect(screen.queryByText(/M_FORBIDDEN/)).not.toBeInTheDocument();
  });

  it("re-enables the form after a failure so the password can be corrected", async () => {
    const user = userEvent.setup();
    login.mockRejectedValue({ message: "Incorrect username or password.", detail: "x" });
    vi.spyOn(console, "error").mockImplementation(() => {});
    render(<LoginScreen onSignedIn={vi.fn()} />);

    await fillIn(user);
    await user.click(fields().submit);
    await screen.findByRole("alert");

    expect(fields().submit).toBeEnabled();
    expect(fields().password).toBeEnabled();
  });

  it("clears the previous error when the form is submitted again", async () => {
    const user = userEvent.setup();
    login.mockRejectedValueOnce({ message: "Incorrect username or password.", detail: "x" });
    login.mockResolvedValueOnce(profile);
    vi.spyOn(console, "error").mockImplementation(() => {});
    render(<LoginScreen onSignedIn={vi.fn()} />);

    await fillIn(user);
    await user.click(fields().submit);
    await screen.findByRole("alert");

    await user.click(fields().submit);

    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
  });

  it("disables the fields while the login is in flight", async () => {
    const user = userEvent.setup();
    let release: (value: Profile) => void = () => {};
    login.mockReturnValue(new Promise<Profile>((resolve) => (release = resolve)));
    render(<LoginScreen onSignedIn={vi.fn()} />);

    await fillIn(user);
    await user.click(fields().submit);

    await waitFor(() => expect(fields().submit).toBeDisabled());
    expect(fields().homeserver).toBeDisabled();
    expect(fields().username).toBeDisabled();
    expect(fields().password).toBeDisabled();

    // Settle it inside act. Resolving after the test body returns updates
    // state outside React's batching and produces a warning that would then
    // be background noise hiding a real one.
    await act(async () => release(profile));
  });

  it("does not fire a second login while the first is still running", async () => {
    // The Rust side has its own guard, but the UI should not be the thing
    // that needs it.
    const user = userEvent.setup();
    let release: (value: Profile) => void = () => {};
    login.mockReturnValue(new Promise<Profile>((resolve) => (release = resolve)));
    render(<LoginScreen onSignedIn={vi.fn()} />);

    await fillIn(user);
    await user.click(fields().submit);
    await user.click(fields().submit);
    await user.click(fields().submit);

    expect(login).toHaveBeenCalledTimes(1);
    await act(async () => release(profile));
  });

  it("does nothing when submitted with empty fields", async () => {
    const user = userEvent.setup();
    render(<LoginScreen onSignedIn={vi.fn()} />);

    // Bypass the disabled button and submit the form directly, which is what
    // pressing Enter in a field does.
    const form = document.querySelector("form");
    form?.requestSubmit?.();
    await user.click(document.body);

    expect(login).not.toHaveBeenCalled();
  });

  it("logs the developer-facing detail to the console", async () => {
    const user = userEvent.setup();
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    login.mockRejectedValue({ message: "Incorrect username or password.", detail: "M_FORBIDDEN" });
    render(<LoginScreen onSignedIn={vi.fn()} />);

    await fillIn(user);
    await user.click(fields().submit);
    await screen.findByRole("alert");

    expect(consoleError).toHaveBeenCalledWith("login failed", "M_FORBIDDEN");
  });
});
