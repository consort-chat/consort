import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import {
  goBack,
  goForward,
  pressBack,
  pressForward,
  pressPrimary,
} from "../test/traversal";
import { useHistory, type Where } from "./history";

const NOWHERE: Where = { spaceId: "home", channelId: null };
const GENERAL: Where = { spaceId: "home", channelId: "!general:example.org" };
const TECH: Where = { spaceId: "home", channelId: "!tech:example.org" };

describe("useHistory", () => {
  it("starts where it is told to", () => {
    const { result } = renderHook(() => useHistory(NOWHERE));

    expect(result.current[0]).toEqual(NOWHERE);
  });

  it("goes where it is sent", () => {
    const { result } = renderHook(() => useHistory(NOWHERE));

    act(() => result.current[1](GENERAL));

    expect(result.current[0]).toEqual(GENERAL);
  });

  it("comes back to the place it left", async () => {
    const { result } = renderHook(() => useHistory(NOWHERE));

    act(() => result.current[1](GENERAL));
    act(() => result.current[1](TECH));
    await goBack();

    expect(result.current[0]).toEqual(GENERAL);
  });

  it("goes forward again to the place it came back from", async () => {
    const { result } = renderHook(() => useHistory(NOWHERE));

    act(() => result.current[1](GENERAL));
    act(() => result.current[1](TECH));
    await goBack();
    await goForward();

    expect(result.current[0]).toEqual(TECH);
  });

  it("comes back past the first room to where it started", async () => {
    const { result } = renderHook(() => useHistory(NOWHERE));

    act(() => result.current[1](GENERAL));
    await goBack();

    expect(result.current[0]).toEqual(NOWHERE);
  });

  it("does not stack the place it is already on", async () => {
    // Clicking the channel you are reading is not somewhere new, and an entry
    // for it would make Back a button that does nothing the first time.
    const { result } = renderHook(() => useHistory(NOWHERE));

    act(() => result.current[1](GENERAL));
    act(() => result.current[1](GENERAL));
    await goBack();

    expect(result.current[0]).toEqual(NOWHERE);
  });

  it("stays put when the entry behind is not one of ours", async () => {
    const { result } = renderHook(() => useHistory(NOWHERE));

    act(() => window.history.pushState({ someoneElse: true }, ""));
    act(() => result.current[1](GENERAL));
    await goBack();

    expect(result.current[0]).toEqual(GENERAL);
  });

  it("comes back one place when the mouse's back button is pressed", async () => {
    // The button the whole thing exists for, and two places behind it rather
    // than one, so that a press worth two entries is a failure here and not
    // an answer that happens to be right.
    const { result } = renderHook(() => useHistory(NOWHERE));

    act(() => result.current[1](GENERAL));
    act(() => result.current[1](TECH));
    await pressBack();

    expect(result.current[0]).toEqual(GENERAL);
  });

  it("goes forward one place when the mouse's forward button is pressed", async () => {
    const { result } = renderHook(() => useHistory(NOWHERE));

    act(() => result.current[1](GENERAL));
    act(() => result.current[1](TECH));
    await goBack();
    await goBack();
    await pressForward();

    expect(result.current[0]).toEqual(GENERAL);
  });

  it("leaves an ordinary click where it is", async () => {
    const { result } = renderHook(() => useHistory(NOWHERE));

    act(() => result.current[1](GENERAL));
    await pressPrimary();

    expect(result.current[0]).toEqual(GENERAL);
  });
});
