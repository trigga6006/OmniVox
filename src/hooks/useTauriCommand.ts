import { useCallback, useEffect, useRef, useState } from "react";

interface UseTauriCommandResult<T> {
  data: T | null;
  error: string | null;
  isLoading: boolean;
  refetch: () => void;
}

export function useTauriCommand<T>(
  commandFn: () => Promise<T>,
  immediate = true
): UseTauriCommandResult<T> {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(immediate);
  const isMountedRef = useRef(true);
  const requestIdRef = useRef(0);

  const execute = useCallback(async () => {
    if (!isMountedRef.current) return;
    const requestId = ++requestIdRef.current;
    setIsLoading(true);
    setError(null);
    try {
      const result = await commandFn();
      if (!isMountedRef.current || requestId !== requestIdRef.current) return;
      setData(result);
    } catch (err) {
      if (!isMountedRef.current || requestId !== requestIdRef.current) return;
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      if (!isMountedRef.current || requestId !== requestIdRef.current) return;
      setIsLoading(false);
    }
  }, [commandFn]);

  useEffect(() => {
    isMountedRef.current = true;
    return () => {
      isMountedRef.current = false;
      requestIdRef.current += 1;
    };
  }, []);

  useEffect(() => {
    if (immediate) {
      execute();
    }
  }, [execute, immediate]);

  return { data, error, isLoading, refetch: execute };
}
