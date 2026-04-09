import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ThemeToggle } from "@/components/providers/theme-toggle";
import { APP_THEME_STORAGE_KEY } from "@/lib/app-theme";

describe("ThemeToggle", () => {
  beforeEach(() => {
    window.localStorage.clear();
    delete document.documentElement.dataset.theme;
    document.documentElement.style.colorScheme = "";
  });

  it("loads the saved theme and toggles the document theme", async () => {
    window.localStorage.setItem(APP_THEME_STORAGE_KEY, "light");
    render(<ThemeToggle />);

    expect(await screen.findByRole("button", { name: "Switch to dark mode" })).toBeInTheDocument();
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(document.documentElement.style.colorScheme).toBe("light");

    await userEvent.click(screen.getByRole("button", { name: "Switch to dark mode" }));

    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(document.documentElement.style.colorScheme).toBe("dark");
    expect(window.localStorage.getItem(APP_THEME_STORAGE_KEY)).toBe("dark");
  });
});
