import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, X } from "lucide-react";

export function TitleBar() {
  const minimize = async () => {
    await getCurrentWindow().minimize();
  };

  const close = async () => {
    await getCurrentWindow().close();
  };

  return (
    <div className="pointer-events-none fixed inset-x-0 top-0 z-50 h-0">
      <div data-tauri-drag-region className="pointer-events-auto absolute inset-x-0 top-0 h-6" />

      <div className="pointer-events-auto absolute right-4 top-4 flex items-center gap-2">
        <button
          onClick={minimize}
          className="inline-flex h-8 w-8 items-center justify-center text-neutral-400 transition-colors hover:bg-neutral-200 hover:text-neutral-600"
        >
          <Minus className="h-4 w-4" />
        </button>
        <button
          onClick={close}
          className="inline-flex h-8 w-8 items-center justify-center text-neutral-400 transition-colors hover:bg-red-50 hover:text-red-500"
        >
          <X className="h-4 w-4" />
        </button>
      </div>
    </div>
  );
}
