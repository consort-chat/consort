import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";

const selfView = vi.hoisted(() => vi.fn());
vi.mock("./api", () => ({ selfView, asCommandError: (raw: unknown) => raw }));

import { useSelfView } from "./useSelfView";

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
});

describe("the self view in the call card", () => {
  it("asks for nothing while the camera is off", () => {
    // The camera is off for most of every call, and a poll running through all
    // of it would convert frames for a picture nobody is drawing.
    renderHook(() => useSelfView(false));

    expect(selfView).not.toHaveBeenCalled();
  });

  it("draws the first frame it is given", async () => {
    const { result } = renderHook(() => useSelfView(true));

    await waitFor(() => expect(result.current).toBe(A));
  });

  it("keeps asking, so the picture moves", async () => {
    // A still that is fetched once is a photograph. The whole point is that it
    // keeps up with the camera.
    selfView.mockResolvedValueOnce(A).mockResolvedValue(B);

    const { result } = renderHook(() => useSelfView(true));

    await waitFor(() => expect(result.current).toBe(B));
    expect(selfView.mock.calls.length).toBeGreaterThan(1);
  });

  it("stops asking when the camera goes off, and forgets the picture", async () => {
    // Forgets, so the card goes back to faces at the click rather than at
    // whatever the next poll would have said.
    const { result, rerender } = renderHook(
      ({ on }: { on: boolean }) => useSelfView(on),
      { initialProps: { on: true } },
    );
    await waitFor(() => expect(result.current).toBe(A));

    rerender({ on: false });

    expect(result.current).toBe(null);
    const asked = selfView.mock.calls.length;
    await new Promise((resume) => setTimeout(resume, PAST_A_POLL));
    expect(selfView.mock.calls.length).toBe(asked);
  });

  it("stops asking once it is unmounted", async () => {
    // The card can be hidden mid-call. A poll that outlived it would be a
    // frame converted every tick for the rest of the session.
    const { unmount } = renderHook(() => useSelfView(true));
    await waitFor(() => expect(selfView).toHaveBeenCalled());

    unmount();
    const asked = selfView.mock.calls.length;
    await new Promise((resume) => setTimeout(resume, PAST_A_POLL));

    expect(selfView.mock.calls.length).toBe(asked);
  });

  it("draws nothing when there is no frame yet", async () => {
    // Between opening a device and its first frame. Rust answers null and the
    // card draws its faces.
    selfView.mockResolvedValue(null);

    const { result } = renderHook(() => useSelfView(true));

    await waitFor(() => expect(selfView).toHaveBeenCalled());
    expect(result.current).toBe(null);
  });

  it("gives up rather than asking forever when a poll fails", async () => {
    // A failing poll twelve times a second would be twelve log lines a second.
    // The camera's own state is reported on the self-video channel, so the card
    // falling back to faces loses nothing somebody needed.
    const complained = vi
      .spyOn(console, "error")
      .mockImplementation(() => undefined);
    selfView.mockReset().mockImplementation(() => Promise.reject(new Error("no")));

    const { result } = renderHook(() => useSelfView(true));

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

    renderHook(() => useSelfView(true));
    await waitFor(() => expect(selfView).toHaveBeenCalledTimes(1));
    await new Promise((resume) => setTimeout(resume, PAST_A_POLL));

    expect(selfView).toHaveBeenCalledTimes(1);
    await act(async () => {
      answer(A);
    });
  });
});
