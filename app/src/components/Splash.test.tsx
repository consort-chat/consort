import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { Splash } from "./Splash";

describe("Splash", () => {
  it("says what is happening rather than showing a bare spinner", () => {
    render(<Splash />);

    expect(screen.getByText(/signing you in/i)).toBeVisible();
  });

  it("hides the decorative mark from assistive technology", () => {
    const { container } = render(<Splash />);

    expect(container.querySelector(".splash__mark")).toHaveAttribute(
      "aria-hidden",
      "true",
    );
  });
});
