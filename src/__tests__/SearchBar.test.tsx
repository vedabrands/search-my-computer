import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import React from "react";
import { SearchBar } from "../SearchBar";
import { ParsedQuery } from "../types";

describe("SearchBar Component", () => {
  it("renders input, handles typing, and triggers onChange callback", () => {
    const onChange = vi.fn();
    const onKeyDown = vi.fn();
    const inputRef = React.createRef<HTMLInputElement>();

    render(
      <SearchBar
        query="rust project"
        onChange={onChange}
        onKeyDown={onKeyDown}
        inputRef={inputRef}
      />
    );

    const input = screen.getByPlaceholderText(/Search files, code, docs/i);
    expect(input).toBeInTheDocument();
    expect(input).toHaveValue("rust project");

    fireEvent.change(input, { target: { value: "invoice pdf" } });
    expect(onChange).toHaveBeenCalledWith("invoice pdf");

    fireEvent.keyDown(input, { key: "Enter", code: "Enter" });
    expect(onKeyDown).toHaveBeenCalledTimes(1);
  });

  it("renders clear button when query is non-empty and clears on click", () => {
    const onChange = vi.fn();
    const onKeyDown = vi.fn();
    const inputRef = React.createRef<HTMLInputElement>();

    const { rerender } = render(
      <SearchBar
        query=""
        onChange={onChange}
        onKeyDown={onKeyDown}
        inputRef={inputRef}
      />
    );

    expect(screen.queryByTitle("Clear search")).not.toBeInTheDocument();

    rerender(
      <SearchBar
        query="tax forms"
        onChange={onChange}
        onKeyDown={onKeyDown}
        inputRef={inputRef}
      />
    );

    const clearBtn = screen.getByTitle("Clear search");
    expect(clearBtn).toBeInTheDocument();
    fireEvent.click(clearBtn);
    expect(onChange).toHaveBeenCalledWith("");
  });

  it("renders parsed query filter chips for file types, locations, and intents", () => {
    const onChange = vi.fn();
    const onKeyDown = vi.fn();
    const inputRef = React.createRef<HTMLInputElement>();

    const parsedQuery: ParsedQuery = {
      raw: "pdf in downloads rust project",
      text: "rust",
      file_types: ["pdf"],
      location_hints: ["downloads"],
      after: undefined,
      before: undefined,
      is_screenshot_query: false,
      is_project_query: true,
      use_ctime: false,
    };

    render(
      <SearchBar
        query="pdf in downloads rust project"
        parsedQuery={parsedQuery}
        onChange={onChange}
        onKeyDown={onKeyDown}
        inputRef={inputRef}
      />
    );

    expect(screen.getByText("PDF")).toBeInTheDocument();
    expect(screen.getByText("in downloads")).toBeInTheDocument();
    expect(screen.getByText("Projects")).toBeInTheDocument();
  });
});
