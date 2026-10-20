import { type ReactNode, useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";

export function Section({ title, desc, children }: { title: string; desc?: string; children: ReactNode }) {
  return (
    <section className="space-y-4">
      <div className="border-b border-neutral-200 pb-2">
        <div className="text-xs font-medium uppercase tracking-wider text-neutral-400">{title}</div>
        {desc && <div className="mt-1 text-xs text-neutral-400">{desc}</div>}
      </div>
      <div className="space-y-4">{children}</div>
    </section>
  );
}

/** Collapsed-by-default block for settings most people never touch. */
export function Advanced({ children, label = "高级设置" }: { children: ReactNode; label?: string }) {
  const [open, setOpen] = useState(false);
  return (
    <div>
      <button
        onClick={() => setOpen(!open)}
        className="inline-flex items-center gap-1 px-1 py-1 text-xs text-neutral-400 transition-colors hover:bg-neutral-200 hover:text-neutral-700"
      >
        {open ? <ChevronDown className="h-3.5 w-3.5" /> : <ChevronRight className="h-3.5 w-3.5" />}
        {label}
      </button>
      {open && <div className="mt-3 space-y-4 border-l border-neutral-200 pl-4">{children}</div>}
    </div>
  );
}

export function Field({
  label,
  hint,
  value,
  onChange,
  type = "text",
  placeholder,
}: {
  label: string;
  hint?: string;
  value: string;
  onChange: (value: string) => void;
  type?: string;
  placeholder?: string;
}) {
  return (
    <div>
      <label className="mb-1 block text-xs text-neutral-400">{label}</label>
      <input
        type={type}
        value={value}
        placeholder={placeholder}
        onChange={(event) => onChange(event.target.value)}
        className="input-underline w-full py-2 text-sm text-neutral-900"
      />
      {hint && <div className="mt-1 text-xs text-neutral-400">{hint}</div>}
    </div>
  );
}

export function NumberField({
  label,
  hint,
  value,
  min,
  max,
  onChange,
}: {
  label: string;
  hint?: string;
  value: number;
  min: number;
  max: number;
  onChange: (value: number) => void;
}) {
  return (
    <div>
      <label className="mb-1 block text-xs text-neutral-400">{label}</label>
      <input
        type="number"
        min={min}
        max={max}
        value={value}
        onChange={(event) => onChange(Math.min(max, Math.max(min, parseInt(event.target.value) || min)))}
        className="input-underline w-32 py-2 text-sm text-neutral-900"
      />
      {hint && <div className="mt-1 text-xs text-neutral-400">{hint}</div>}
    </div>
  );
}

export function Area({
  label,
  hint,
  value,
  onChange,
  rows = 3,
  placeholder,
}: {
  label: string;
  hint?: string;
  value: string;
  onChange: (value: string) => void;
  rows?: number;
  placeholder?: string;
}) {
  return (
    <div>
      <label className="mb-1 block text-xs text-neutral-400">{label}</label>
      <textarea
        rows={rows}
        value={value}
        placeholder={placeholder}
        onChange={(event) => onChange(event.target.value)}
        className="w-full resize-none bg-neutral-50 px-3 py-2 text-sm text-neutral-900 outline-none transition-colors placeholder:text-neutral-300 focus:bg-white focus:shadow-[inset_0_0_0_1px_#d4d4d4]"
      />
      {hint && <div className="mt-1 text-xs text-neutral-400">{hint}</div>}
    </div>
  );
}

export function ToggleRow({
  title,
  desc,
  active,
  onToggle,
}: {
  title: string;
  desc?: string;
  active: boolean;
  onToggle: () => void;
}) {
  return (
    <button onClick={onToggle} className="flex w-full items-start justify-between gap-4 py-2 text-left">
      <div>
        <div className="text-sm text-neutral-900">{title}</div>
        {desc && <div className="mt-0.5 text-xs text-neutral-400">{desc}</div>}
      </div>
      <div className="relative mt-1 h-4 w-7 flex-shrink-0">
        <div className="absolute left-0.5 right-0.5 top-1/2 h-0.5 -translate-y-1/2 bg-neutral-200" />
        <div
          className={`absolute left-0.5 top-1/2 h-0.5 -translate-y-1/2 bg-neutral-900 transition-all duration-200 ease-out ${
            active ? "w-6" : "w-0"
          }`}
        />
        <div
          className={`absolute top-0 h-4 w-4 transition-all duration-200 ease-out ${
            active ? "left-3.5 bg-neutral-900" : "left-0 border-2 border-neutral-300 bg-neutral-50"
          }`}
        />
      </div>
    </button>
  );
}

/** Large selectable option: a title plus one line explaining the trade-off. */
export function ChoiceCard({
  title,
  desc,
  active,
  onClick,
}: {
  title: string;
  desc: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={`flex-1 border px-4 py-3 text-left transition-colors ${
        active ? "border-neutral-900 bg-white" : "border-neutral-200 hover:bg-neutral-100"
      }`}
    >
      <div className="flex items-center gap-2 text-sm text-neutral-900">
        <span
          className={`h-2 w-2 rounded-full ${active ? "bg-chinese-indigo" : "border border-neutral-300"}`}
        />
        {title}
      </div>
      <div className="mt-1 pl-4 text-xs leading-5 text-neutral-500">{desc}</div>
    </button>
  );
}

export function Chip({ label, active, onClick }: { label: string; active: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className={`px-3 py-1.5 text-sm transition-colors ${
        active ? "bg-neutral-900 text-neutral-50" : "bg-neutral-100 text-neutral-500 hover:bg-neutral-200 hover:text-neutral-700"
      }`}
    >
      {label}
    </button>
  );
}

export function ActionButton({
  children,
  onClick,
  disabled = false,
  title,
}: {
  children: ReactNode;
  onClick: () => void;
  disabled?: boolean;
  title?: string;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      title={title}
      className="inline-flex items-center gap-1.5 px-2 py-1 text-sm text-neutral-500 transition-colors hover:bg-neutral-200 hover:text-neutral-900 disabled:cursor-not-allowed disabled:opacity-40"
    >
      {children}
    </button>
  );
}

export function PrimaryButton({
  children,
  onClick,
  disabled = false,
}: {
  children: ReactNode;
  onClick: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className="inline-flex items-center justify-center gap-2 bg-neutral-900 px-3 py-1.5 text-sm font-medium text-neutral-50 transition-opacity hover:opacity-70 disabled:cursor-not-allowed disabled:opacity-40"
    >
      {children}
    </button>
  );
}

export function ProgressBar({ value }: { value: number }) {
  return (
    <div className="h-1.5 overflow-hidden rounded-full bg-neutral-200">
      <div
        className="h-full rounded-full bg-chinese-indigo transition-all"
        style={{ width: `${Math.round(Math.min(1, value) * 100)}%` }}
      />
    </div>
  );
}

export function Notice({
  tone = "neutral",
  children,
}: {
  tone?: "neutral" | "amber" | "red" | "emerald";
  children: ReactNode;
}) {
  const border = {
    neutral: "border-neutral-300",
    amber: "border-amber-500",
    red: "border-red-500",
    emerald: "border-emerald-500",
  }[tone];
  return <div className={`border-l-2 ${border} bg-white px-3 py-2 text-xs leading-5 text-neutral-600`}>{children}</div>;
}

export function Key({ children }: { children: ReactNode }) {
  return (
    <span className="inline-flex items-center border border-neutral-300 bg-white px-1.5 py-0.5 text-xs text-neutral-700">
      {children}
    </span>
  );
}
