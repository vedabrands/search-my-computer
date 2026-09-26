import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { SearchResult, ProjectRecord, ParsedQuery } from "../types";

export function useSearch(query: string, limit = 20) {
  const [results, setResults] = useState<SearchResult[]>([]);
  const [projectResults, setProjectResults] = useState<ProjectRecord[]>([]);
  const [parsedQuery, setParsedQuery] = useState<ParsedQuery | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const debounceTimer = useRef<number | null>(null);
  const querySequence = useRef<number>(0);

  useEffect(() => {
    const trimmed = query.trim();
    if (!trimmed) {
      setResults([]);
      setProjectResults([]);
      setParsedQuery(null);
      setLoading(false);
      return;
    }

    setLoading(true);
    setError(null);

    if (debounceTimer.current) {
      window.clearTimeout(debounceTimer.current);
    }

    const currentSeq = ++querySequence.current;

    debounceTimer.current = window.setTimeout(async () => {
      try {
        const [searchRes, parsed] = await Promise.all([
          invoke<SearchResult[]>("search", {
            query: trimmed,
            limit,
          }),
          invoke<ParsedQuery>("parse_nlq", {
            query: trimmed,
          }),
        ]);

        let prjRes: ProjectRecord[] = [];
        const projectQueryText = parsed.text || trimmed;
        if (projectQueryText.length > 0) {
          try {
            prjRes = await invoke<ProjectRecord[]>("search_projects", {
              query: projectQueryText,
              limit: 5,
            });
          } catch (e) {
            console.warn("search_projects error:", e);
          }
        }

        if (currentSeq === querySequence.current) {
          setResults(searchRes);
          setParsedQuery(parsed);
          setProjectResults(prjRes);
        }
      } catch (err) {
        if (currentSeq === querySequence.current) {
          setError(String(err));
          setResults([]);
          setProjectResults([]);
          setParsedQuery(null);
        }
      } finally {
        if (currentSeq === querySequence.current) {
          setLoading(false);
        }
      }
    }, 80); // ~80ms debounce per spec

    return () => {
      if (debounceTimer.current) {
        window.clearTimeout(debounceTimer.current);
      }
    };
  }, [query, limit]);

  return { results, projectResults, parsedQuery, loading, error };
}
