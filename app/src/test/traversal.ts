/**
 * Going back and forward the way the buttons on a mouse do.
 *
 * `popstate` arrives on a later task, in jsdom as in a browser, so a `back()`
 * has not reached anything by the time the call returns. Waiting on turns of
 * the task queue rather than on a delay, which would be a number tuned on one
 * machine and flaky on another.
 */
import { act } from "@testing-library/react";

async function traversed(): Promise<void> {
  await act(async () => {
    for (let turn = 0; turn < 5; turn += 1) {
      await new Promise((settle) => setTimeout(settle, 0));
    }
  });
}

export async function goBack(): Promise<void> {
  act(() => window.history.back());
  await traversed();
}

export async function goForward(): Promise<void> {
  act(() => window.history.forward());
  await traversed();
}

/**
 * The extra buttons on a mouse, as the DOM numbers them.
 *
 * 3 and 4 are what the UI Events specification calls the fourth and fifth
 * buttons, which every mouse that has them ships as Back and Forward.
 */
const BACK = 3;
const FORWARD = 4;

/**
 * A press of one of those, the way the webview host delivers it.
 *
 * wry inhibits buttons 8 and 9 at the GTK level and dispatches a `mousedown`
 * and a `mouseup` of its own in their place, then traverses on the `mouseup`
 * unless the page cancelled that event. Modelled here rather than shortened
 * to a bare dispatch, because a harness that only fires the event cannot fail
 * on a page that traverses as well, and two moves per press was the fault.
 *
 * The wait between press and release is the length of a press. Without it
 * both moves resolve against the same entry and land one step together, which
 * is why a quick synthetic click hid this and a finger on the button did not.
 */
async function press(button: number): Promise<void> {
  act(() => {
    window.dispatchEvent(
      new MouseEvent("mousedown", { button, bubbles: true, cancelable: true }),
    );
  });
  await traversed();

  const release = new MouseEvent("mouseup", {
    button,
    bubbles: true,
    cancelable: true,
  });
  act(() => {
    window.dispatchEvent(release);
    if (release.defaultPrevented) return;
    if (button === BACK) window.history.back();
    if (button === FORWARD) window.history.forward();
  });
  await traversed();
}

export async function pressBack(): Promise<void> {
  await press(BACK);
}

export async function pressForward(): Promise<void> {
  await press(FORWARD);
}

/** An ordinary left click, which must not be mistaken for either of those. */
export async function pressPrimary(): Promise<void> {
  await press(0);
}
