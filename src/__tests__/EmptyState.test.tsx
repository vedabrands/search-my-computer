import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { EmptyState } from "../EmptyState";

describe("EmptyState Component", () => {
  it("renders zero folders state and allows adding a folder path", () => {
    const onAddFolder = vi.fn();
    render(<EmptyState folderCount={0} onAddFolder={onAddFolder} />);

    expect(screen.getByText("No folders indexed yet")).toBeInTheDocument();
    const input = screen.getByPlaceholderText(/C:\\Users\\...\\Documents/i);
    fireEvent.change(input, { target: { value: "C:\\Projects" } });

    const addBtn = screen.getByRole("button", { name: /Add Folder/i });
    fireEvent.click(addBtn);

    expect(onAddFolder).toHaveBeenCalledWith("C:\\Projects");
  });

  it("renders no-matches-found state with troubleshooting tips when query is active", () => {
    const onOpenActions = vi.fn();
    const onOpenSettings = vi.fn();
    render(
      <EmptyState
        folderCount={2}
        query="nonexistent_xyz_file"
        onAddFolder={vi.fn()}
        onOpenActions={onOpenActions}
        onOpenSettings={onOpenSettings}
      />
    );

    expect(screen.getByText(/No matches found for/i)).toBeInTheDocument();
    expect(screen.getByText(/nonexistent_xyz_file/i)).toBeInTheDocument();

    const actionsBtn = screen.getByRole("button", { name: /Actions Menu \(Ctrl\+K\)/i });
    fireEvent.click(actionsBtn);
    expect(onOpenActions).toHaveBeenCalledTimes(1);

    const settingsBtn = screen.getByRole("button", { name: /Settings \(Ctrl\+,\)/i });
    fireEvent.click(settingsBtn);
    expect(onOpenSettings).toHaveBeenCalledTimes(1);
  });

  it("renders recent searches and query suggestion chips in idle ready state", () => {
    const onSelectQuery = vi.fn();
    const onRemoveRecentSearch = vi.fn();
    const onClearRecentSearches = vi.fn();

    render(
      <EmptyState
        folderCount={2}
        query=""
        recentSearches={["rust async", "quarterly budget"]}
        onAddFolder={vi.fn()}
        onSelectQuery={onSelectQuery}
        onRemoveRecentSearch={onRemoveRecentSearch}
        onClearRecentSearches={onClearRecentSearches}
      />
    );

    expect(screen.getByText(/Recent Searches/i)).toBeInTheDocument();
    expect(screen.getByText("rust async")).toBeInTheDocument();
    expect(screen.getByText("quarterly budget")).toBeInTheDocument();

    // Click suggestion query
    const chip = screen.getByText("where is my NutriGrade project");
    fireEvent.click(chip);
    expect(onSelectQuery).toHaveBeenCalledWith("where is my NutriGrade project");

    // Clear history
    const clearBtn = screen.getByRole("button", { name: /Clear/i });
    fireEvent.click(clearBtn);
    expect(onClearRecentSearches).toHaveBeenCalledTimes(1);
  });
});
