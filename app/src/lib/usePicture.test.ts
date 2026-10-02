import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";

const selfView = vi.hoisted(() => vi.fn());
const screenView = vi.hoisted(() => vi.fn());
vi.mock("./api", () => ({
  selfView,
  screenView,
  asCommandError: (raw: unknown) => raw,
}));

import { usePicture } from "./usePicture";

/**
 * Comfortably longer than the hook's own interval.
 *
 * A timer the cleanup failed to clear fires one interval later, so a wait
 * shorter than that would pass with the poll still running.
 */
const PAST_A_POLL = 200;

const A = "data:image/jpeg;base64,aaaa";
const B = "data:image/jpeg;base64,bbbb";

beforeEach(() => {
  selfView.mockReset().mockResolvedValue(A);
  screenView.mockReset().mockResolvedValue(B);
});

describe("a picture of what this session is sending", () => {
  it("draws the first frame it is given", async () => {
    const { result } = renderHook(() => usePicture("camera"));

    await waitFor(() => expect(result.current).toBe(A));
  });

  it("asks the screen for a screen rather than the camera", async () => {
    // One hook, two sources. Reading the camera here would draw somebody's
    // face in the square that is supposed to hold their slides.
    const { result } = renderHook(() => usePicture("screen"));

    await waitFor(() => expect(result.current).toBe(B));
    expect(selfView).not.toHaveBeenCalled();
  });

  it("keeps asking, so the picture moves", async () => {
    // A still that is fetched once is a photograph. The whole point is that it
    // keeps up with whatever is being captured.
    selfView.mockResolvedValueOnce(A).mockResolvedValue(B);

    const { result } = renderHook(() => usePicture("camera"));

    await waitFor(() => expect(result.current).toBe(B));
    expect(selfView.mock.calls.length).toBeGreaterThan(1);
  });

  it("stops asking once it is unmounted", async () => {
    // The card can be hidden mid-call. A poll that outlived it would be a
    // frame converted every tick for the rest of the session.
    const { unmount } = renderHook(() => usePicture("camera"));
    await waitFor(() => expect(selfView).toHaveBeenCalled());

    unmount();
    const asked = selfView.mock.calls.length;
    await new Promise((resume) => setTimeout(resume, PAST_A_POLL));

    expect(selfView.mock.calls.length).toBe(asked);
  });

  it("draws nothing when there is no frame yet", async () => {
    // Between opening a device and its first frame. Rust answers null and the
    // card draws what it draws without one.
    selfView.mockResolvedValue(null);

    const { result } = renderHook(() => usePicture("camera"));

    await waitFor(() => expect(selfView).toHaveBeenCalled());
    expect(result.current).toBe(null);
  });

  it("gives up rather than asking forever when a poll fails", async () => {
    // A failing poll twelve times a second would be twelve log lines a second.
    // What is actually running is reported on its own channel, so falling back
    // loses nothing somebody needed.
    const complained = vi
      .spyOn(console, "error")
      .mockImplementation(() => undefined);
    selfView
      .mockReset()
      .mockImplementation(() => Promise.reject(new Error("no")));

    const { result } = renderHook(() => usePicture("camera"));

    await waitFor(() => expect(complained).toHaveBeenCalledTimes(1));
    expect(result.current).toBe(null);
    await new Promise((resume) => setTimeout(resume, PAST_A_POLL));
    expect(selfView).toHaveBeenCalledTimes(1);
    complained.mockRestore();
  });

  it("does not stack requests when one answer is slow", async () => {
    // Chained rather than on an interval. A camera the IPC cannot keep up with
    // would otherwise queue a poll per tick and never drain.
    let answer: (url: string) => void = () => undefined;
    selfView.mockReset().mockImplementation(
      () =>
        new Promise<string>((resolve) => {
          answer = resolve;
        }),
    );

    renderHook(() => usePicture("camera"));
    await waitFor(() => expect(selfView).toHaveBeenCalledTimes(1));
    await new Promise((resume) => setTimeout(resume, PAST_A_POLL));

    expect(selfView).toHaveBeenCalledTimes(1);
    await act(async () => {
      answer(A);
    });
  });
});
